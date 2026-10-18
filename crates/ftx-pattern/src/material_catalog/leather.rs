//! Numerical observations from Kopitar et al. (2022), DOI 10.1177/15280837221077048,
//! Tables 4–5. Tanning/finish and neck/back sampling remain distinct material identities.
//! Reported variability is not an uncertainty bound, temperature law or compression law.

use std::collections::BTreeMap;

use crate::materials::{Evidence, Form, Material, Quantity, Source};

const ARTICLE: &str = "https://journals.sagepub.com/doi/full/10.1177/15280837221077048";

struct Row {
    slug: &'static str,
    mark: &'static str,
    region: &'static str,
    tanning: &'static str,
    finish: &'static str,
    // Original table triplets: mean, standard deviation, coefficient of variation in percent.
    thickness_mm: [f64; 3],
    resistance_m2_k_w: [f64; 3],
    conductivity_w_m_k: f64,
    vapor_m2_pa_w: [f64; 3],
}

const ROWS: [Row; 10] = [
    Row {
        slug: "syn-d-neck",
        mark: "Syn-D",
        region: "neck",
        tanning: "synthetic",
        finish: "dyed",
        thickness_mm: [1.57, 0.13, 8.52],
        resistance_m2_k_w: [0.034, 0.003, 9.1],
        conductivity_w_m_k: 0.046,
        vapor_m2_pa_w: [15.83, 0.49, 3.1],
    },
    Row {
        slug: "syn-d-h-neck",
        mark: "Syn-D-H",
        region: "neck",
        tanning: "synthetic",
        finish: "dyed and hydrophobic",
        thickness_mm: [2.01, 0.06, 2.76],
        resistance_m2_k_w: [0.031, 0.003, 10.3],
        conductivity_w_m_k: 0.065,
        vapor_m2_pa_w: [30.38, 3.55, 11.7],
    },
    Row {
        slug: "syn-d-h-back",
        mark: "Syn-D-H",
        region: "back",
        tanning: "synthetic",
        finish: "dyed and hydrophobic",
        thickness_mm: [2.22, 0.17, 7.71],
        resistance_m2_k_w: [0.025, 0.001, 5.5],
        conductivity_w_m_k: 0.089,
        vapor_m2_pa_w: [26.96, 0.43, 1.6],
    },
    Row {
        slug: "syn-d-cf-neck",
        mark: "Syn-D-CF",
        region: "neck",
        tanning: "synthetic",
        finish: "dyed with completed face finish",
        thickness_mm: [1.51, 0.11, 7.37],
        resistance_m2_k_w: [0.027, 0.001, 4.7],
        conductivity_w_m_k: 0.056,
        vapor_m2_pa_w: [21.77, 2.68, 12.3],
    },
    Row {
        slug: "syn-d-cf-back",
        mark: "Syn-D-CF",
        region: "back",
        tanning: "synthetic",
        finish: "dyed with completed face finish",
        thickness_mm: [1.57, 0.04, 2.50],
        resistance_m2_k_w: [0.022, 0.002, 7.5],
        conductivity_w_m_k: 0.071,
        vapor_m2_pa_w: [22.26, 2.46, 11.1],
    },
    Row {
        slug: "cr-neck",
        mark: "Cr",
        region: "neck",
        tanning: "chrome",
        finish: "split leather",
        thickness_mm: [1.02, 0.05, 4.97],
        resistance_m2_k_w: [0.015, 0.002, 12.7],
        conductivity_w_m_k: 0.068,
        vapor_m2_pa_w: [11.27, 0.99, 8.9],
    },
    Row {
        slug: "cr-h-neck",
        mark: "Cr-H",
        region: "neck",
        tanning: "chrome",
        finish: "hydrophobic",
        thickness_mm: [1.40, 0.14, 10.08],
        resistance_m2_k_w: [0.023, 0.004, 17.5],
        conductivity_w_m_k: 0.061,
        vapor_m2_pa_w: [20.41, 2.03, 9.9],
    },
    Row {
        slug: "cr-h-back",
        mark: "Cr-H",
        region: "back",
        tanning: "chrome",
        finish: "hydrophobic",
        thickness_mm: [1.44, 0.11, 7.88],
        resistance_m2_k_w: [0.019, 0.003, 15.7],
        conductivity_w_m_k: 0.076,
        vapor_m2_pa_w: [18.93, 1.94, 10.2],
    },
    Row {
        slug: "cr-d-h-neck",
        mark: "Cr-D-H",
        region: "neck",
        tanning: "chrome",
        finish: "dyed and hydrophobic",
        thickness_mm: [1.57, 0.11, 6.86],
        resistance_m2_k_w: [0.025, 0.005, 21.2],
        conductivity_w_m_k: 0.063,
        vapor_m2_pa_w: [25.27, 1.84, 7.3],
    },
    Row {
        slug: "cr-d-h-back",
        mark: "Cr-D-H",
        region: "back",
        tanning: "chrome",
        finish: "dyed and hydrophobic",
        thickness_mm: [1.71, 0.05, 2.71],
        resistance_m2_k_w: [0.019, 0.001, 1.8],
        conductivity_w_m_k: 0.090,
        vapor_m2_pa_w: [25.15, 1.08, 4.3],
    },
];

fn variability(quantity: &mut Quantity, observation: [f64; 3]) {
    quantity.conditions.insert(
        "reported_standard_deviation".into(),
        format!("{} {}", observation[1], quantity.unit),
    );
    quantity.conditions.insert(
        "reported_coefficient_of_variation_percent".into(),
        observation[2].to_string(),
    );
    quantity.conditions.insert(
        "variability_scope".into(),
        "Reported SD and CV retained independently with source rounding; neither is a tolerance, confidence interval or propagated uncertainty".into(),
    );
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(|row| {
        let specimen = format!("Kopitar2022:{}:{}", row.mark, row.region);
        let q = |value, unit: &str, evidence, method: &str| Quantity {
            value, unit: unit.into(), evidence, source: "paper".into(), method: method.into(),
            conditions: BTreeMap::from([
                ("specimen".into(), specimen.clone()),
                ("sampling_region".into(), row.region.into()),
                ("conditioning".into(), "Preconditioning duration and specimen moisture content not reported; test air conditions recorded separately".into()),
            ]),
            bounds: None,
        };
        let mut thickness = q(row.thickness_mm[0], "mm", Evidence::Measured,
            "ISO 2589:2016; Table 4 reported thickness mean");
        variability(&mut thickness, row.thickness_mm);
        thickness.conditions.extend([
            ("sample_count".into(), "not reported for thickness".into()),
            ("measurement_pressure".into(), "ISO 2589:2016 cited; pressure not numerically reported in article".into()),
            ("applicability".into(), "Protocol thickness; not unloaded loft or a presser-foot compression law".into()),
        ]);
        let mut resistance = q(row.resistance_m2_k_w[0], "m2*K/W", Evidence::Measured,
            "ISO 11092:2014 sweating guarded hotplate; Table 5 Rct mean");
        variability(&mut resistance, row.resistance_m2_k_w);
        resistance.conditions.extend([
            ("sample_count".into(), "3".into()),
            ("aggregation".into(), "arithmetic mean of three individual specimens".into()),
            ("air_temperature_c".into(), "20".into()),
            ("relative_humidity_percent".into(), "65".into()),
            ("air_speed_m_s".into(), "1".into()),
            ("reported_unit".into(), "m2*degC/W (temperature difference)".into()),
            ("temperature_scope".into(), "Single test protocol; no temperature-dependent law or validity interval reported".into()),
        ]);
        let mut conductivity = q(row.conductivity_w_m_k, "W/(m*K)", Evidence::Published,
            "Table 5 reported effective conductivity; no independent conductivity test specified");
        conductivity.conditions.extend([
            ("air_temperature_c".into(), "20".into()),
            ("relative_humidity_percent".into(), "65".into()),
            ("air_speed_m_s".into(), "1".into()),
            ("reported_unit".into(), "W/(m*degC) (temperature difference)".into()),
            ("applicability".into(), "Rounded tabulated conductivity; measured Rct takes precedence in thermal consumers".into()),
        ]);
        let mut vapor = q(row.vapor_m2_pa_w[0], "m2*Pa/W", Evidence::Measured,
            "ISO 11092:2014 sweating guarded hotplate; Table 5 Ret mean");
        variability(&mut vapor, row.vapor_m2_pa_w);
        vapor.conditions.extend([
            ("sample_count".into(), "3".into()),
            ("aggregation".into(), "arithmetic mean of three individual specimens".into()),
            ("air_temperature_c".into(), "35".into()),
            ("relative_humidity_percent".into(), "40".into()),
            ("air_speed_m_s".into(), "1".into()),
        ]);
        Material {
            id: format!("zagreb/leather-2022/{}", row.slug),
            name: format!("Bovine leather {}: {} sample", row.mark, row.region),
            form: Form::Hide,
            composition: Vec::new(), composition_complete: false,
            structure: BTreeMap::from([
                ("specimen".into(), specimen),
                ("source_sample_mark".into(), row.mark.into()),
                ("animal".into(), "bovine".into()),
                ("sampling_region".into(), row.region.into()),
                ("tanning".into(), row.tanning.into()),
                ("finish".into(), row.finish.into()),
                ("identity_scope".into(), "Study sample group; supplier, hide identity and lot are unreported; not a retail product specification".into()),
                ("composition_scope".into(), "Tanning and finish are treatments of animal hide; synthetic tanning does not mean synthetic leather; mass fractions unreported".into()),
                ("thermal_model_scope".into(), "One hotplate protocol; existing scalar consumers use constant-property semantics, not a measured temperature range or wearer comfort model".into()),
                ("retention".into(), "Attributed numerical facts only; no article text, images or source archive redistributed; no general redistribution license asserted".into()),
            ]),
            properties: BTreeMap::from([
                ("thickness".into(), thickness),
                ("thermal_resistance".into(), resistance),
                ("thermal_conductivity".into(), conductivity),
                ("vapor_resistance".into(), vapor),
            ]),
            membrane_response: None, membrane_relaxation: None, thermal_response: None,
            sources: BTreeMap::from([("paper".into(), Source {
                reference: ARTICLE.into(), accessed_on: "2026-10-18".into(),
                note: "Kopitar, Zuvela Bosnjak, Akalovic and Skenderi (2022), Thermophysiological properties of bovine leather in dependence on the sampling point, tanning and finishing agents. Journal of Industrial Textiles 51(5S), 8906S–8924S. DOI 10.1177/15280837221077048. Tables 1, 4–5 and Test methods; verified against publisher PDF pages 10–11. Source rounding and independently printed SD/CV retained without reconciliation. Thickness sample count, test-pressure value and specimen preconditioning not reported; thermal and vapor test climates differ.".into(),
            })]),
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_catalog::Selection;
    use crate::material_thermal::{ConductionLaw, ThermalLaw};
    use crate::materials::{Catalog, ThermalStack};

    #[test]
    fn leather_records_qualify_steady_transport_without_inventing_storage_or_mechanics() {
        let catalog = Catalog::builtin().unwrap();
        let rows: Vec<_> = catalog
            .materials
            .iter()
            .filter(|m| m.id.starts_with("zagreb/leather-2022/"))
            .collect();
        assert_eq!(rows.len(), 10);
        assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 40);
        for row in rows {
            let resolved = catalog
                .resolve(&Selection {
                    id: row.id.clone(),
                    ..Selection::default()
                })
                .unwrap();
            let material = resolved.material();
            assert_eq!(material.form, Form::Hide);
            assert!(material.composition.is_empty() && !material.composition_complete);
            assert!(material.properties.values().all(|q| q.bounds.is_none()));
            assert_eq!(
                material.properties["thermal_resistance"].evidence,
                Evidence::Measured
            );
            assert_eq!(
                material.properties["thermal_conductivity"].evidence,
                Evidence::Published
            );
            assert!(ConductionLaw::from_material(material, false).is_ok());
            assert!(ThermalLaw::from_material(material, false).is_err());
            assert!(resolved.mechanics().is_err());
            assert!(!material.properties.contains_key("areal_density"));
            assert!(!material.properties.contains_key("specific_heat"));
            assert_eq!(
                catalog
                    .resolve(&resolved.frozen_selection())
                    .unwrap()
                    .sha256(),
                resolved.sha256()
            );
        }
    }

    #[test]
    fn leather_normalization_preserves_distinct_protocols_and_reported_variability() {
        let catalog = Catalog::builtin().unwrap();
        let resolved = catalog
            .resolve(&Selection {
                id: "zagreb/leather-2022/cr-d-h-back".into(),
                ..Selection::default()
            })
            .unwrap();
        let material = resolved.material();
        let thickness = &material.properties["thickness"];
        assert_eq!(resolved.authored().properties["thickness"].unit, "mm");
        assert!((thickness.value - 0.00171).abs() < 1e-15);
        assert_eq!(
            thickness.conditions["reported_standard_deviation"],
            "0.05 mm"
        );
        assert_eq!(
            thickness.conditions["sample_count"],
            "not reported for thickness"
        );
        let dry = &material.properties["thermal_resistance"].conditions;
        let vapor = &material.properties["vapor_resistance"].conditions;
        assert_eq!(dry["air_temperature_c"], "20");
        assert_eq!(vapor["air_temperature_c"], "35");
        assert_eq!(dry["relative_humidity_percent"], "65");
        assert_eq!(vapor["relative_humidity_percent"], "40");
        assert_eq!(dry["sample_count"], "3");
        // The printed SD and CV need not reproduce one another after source rounding.
        assert_eq!(dry["reported_standard_deviation"], "0.001 m2*K/W");
        assert_eq!(dry["reported_coefficient_of_variation_percent"], "1.8");
        let mut tampered = resolved.frozen_selection();
        tampered
            .record
            .as_mut()
            .unwrap()
            .structure
            .insert("sampling_region".into(), "neck".into());
        assert!(catalog.resolve(&tampered).is_err());
    }

    #[test]
    fn leather_substitution_changes_existing_steady_conduction_at_fixed_area_and_boundaries() {
        let catalog = Catalog::builtin().unwrap();
        let resolve = |slug: &str| {
            catalog
                .resolve(&Selection {
                    id: format!("zagreb/leather-2022/{slug}"),
                    ..Selection::default()
                })
                .unwrap()
        };
        let synthetic_neck = resolve("syn-d-h-neck");
        let chrome_neck = resolve("cr-d-h-neck");
        let chrome_back = resolve("cr-d-h-back");
        let solve = |m| ThermalStack::evaluate(&[m], 0.6, 35.0, 20.0, 0.08, false).unwrap();
        let a = solve(synthetic_neck.material());
        let b = solve(chrome_neck.material());
        let c = solve(chrome_back.material());
        assert!((a.heat_flow_w - 9.0 / 0.111).abs() < 1e-10);
        assert!((b.heat_flow_w - 9.0 / 0.105).abs() < 1e-10);
        assert!((c.heat_flow_w - 9.0 / 0.099).abs() < 1e-10);
        assert!(a.heat_flow_w < b.heat_flow_w && b.heat_flow_w < c.heat_flow_w);
        let law = ConductionLaw::from_material(chrome_back.material(), false).unwrap();
        // Use directly measured resistance, not a reconstruction from rounded thickness / lambda.
        assert_eq!(law.resistance_at(25.0).unwrap(), 0.019);
        assert!((law.flux_between(35.0, 20.0).unwrap() - 15.0 / 0.019).abs() < 1e-10);
        assert_ne!(synthetic_neck.sha256(), chrome_neck.sha256());
        assert_ne!(chrome_neck.sha256(), chrome_back.sha256());
    }
}

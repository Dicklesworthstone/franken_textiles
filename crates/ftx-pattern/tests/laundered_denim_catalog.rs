//! Independent transcription of the primary tables and their canonical treatment/thermal consumers.
use ftx_pattern::{
    material_catalog::{Selection, laundered_denim::treatment_request},
    material_thermal::ThermalLaw,
    material_treatment::{self, Input},
    materials::{Catalog, Evidence, Form, ThermalStack},
};

#[test]
fn paired_tables_keep_original_units_statistics_and_unqualified_properties() {
    let catalog = Catalog::builtin().unwrap();
    assert_eq!(
        catalog
            .materials
            .iter()
            .filter(|m| m.id.starts_with("kanat-ozdil-2026/"))
            .count(),
        10
    );
    // Table 3 then Table 5: code, BW/AW, gsm, mm, cover, R, SD(R), air, SD(air), RWVP, SD(RWVP).
    // Table 4 is transcribed independently in the transition test below (weft column precedes warp).
    for (code, washed, gsm, mm, cover, r, r_sd, air, air_sd, rwvp, rwvp_sd) in [
        (
            1, false, 339.93, 0.84, 0.8813, 0.0152, 0.00066, 25.77, 0.88, 50.67, 3.18,
        ),
        (
            2, false, 339.83, 0.83, 0.8738, 0.0148, 0.00082, 33.01, 3.66, 51.30, 1.10,
        ),
        (
            3, false, 339.70, 0.81, 0.8725, 0.0139, 0.00073, 34.30, 3.48, 51.60, 1.10,
        ),
        (
            4, false, 339.35, 0.77, 0.8716, 0.0128, 0.00025, 35.89, 2.91, 52.40, 1.10,
        ),
        (
            5, false, 339.08, 0.75, 0.8651, 0.0124, 0.00032, 44.21, 2.11, 53.35, 0.45,
        ),
        (
            1, true, 370.10, 1.23, 0.8976, 0.0245, 0.00094, 19.41, 1.21, 52.79, 3.31,
        ),
        (
            2, true, 364.80, 1.20, 0.8923, 0.0220, 0.00130, 21.83, 1.06, 53.45, 1.15,
        ),
        (
            3, true, 361.30, 1.12, 0.8920, 0.0214, 0.00066, 22.89, 0.73, 53.76, 1.15,
        ),
        (
            4, true, 355.60, 1.08, 0.8870, 0.0195, 0.00129, 27.03, 1.23, 54.60, 1.15,
        ),
        (
            5, true, 344.70, 1.01, 0.8792, 0.0196, 0.00069, 32.67, 2.60, 55.59, 0.47,
        ),
    ] {
        let state = if washed {
            "after-5-washes"
        } else {
            "before-washing"
        };
        let material = catalog
            .resolve(&Selection {
                id: format!("kanat-ozdil-2026/denim-{code}/{state}"),
                ..Selection::default()
            })
            .unwrap();
        let authored = material.authored();
        let normalized = material.material();
        assert_eq!(authored.form, Form::Woven);
        assert!(!authored.composition_complete);
        assert!(authored.composition.is_empty());
        assert_eq!(authored.structure["weave"], "2/1 Z twill denim");
        assert_eq!(
            authored.sources["paper"].reference,
            "https://doi.org/10.3390/ma19153264"
        );
        assert!(
            authored.sources["paper"]
                .note
                .contains("0d9fc029c60b79745b2c123a4d4d9bc34fa69734b511cf20d1bccc94fef01e2c")
        );
        for (key, value, unit) in [
            ("areal_density", gsm, "g/m2"),
            ("reported_thickness", mm, "mm"),
            ("reported_fractional_cover", cover, "1"),
            ("thermal_resistance", r, "m2*K/W"),
            ("reported_thermal_resistance_sd", r_sd, "m2*K/W"),
            ("reported_air_permeability", air, "L/(m2*s)"),
            ("reported_air_permeability_sd", air_sd, "L/(m2*s)"),
            ("reported_relative_water_vapor_permeability", rwvp, "%"),
            (
                "reported_relative_water_vapor_permeability_sd",
                rwvp_sd,
                "%",
            ),
        ] {
            let q = &authored.properties[key];
            assert_eq!(q.value, value, "{code} {state} {key}");
            assert_eq!(q.unit, unit);
            assert_eq!(
                q.conditions["specimen"],
                format!("KanatOzdil2026:fabric-{code}")
            );
            assert_eq!(
                q.conditions["source_row"],
                format!("{code}-{}", if washed { "AW" } else { "BW" })
            );
            assert!(q.conditions["conditioning"].contains("65 +/- 4 percent RH"));
            assert!(q.bounds.is_none(), "An SD is not a bound");
            assert_ne!(q.evidence, Evidence::Estimate);
        }
        assert!(
            (normalized.require("areal_density", "kg/m2", false).unwrap() - gsm * 0.001).abs()
                < 1e-15
        );
        assert!(
            (normalized
                .require("reported_thickness", "m", false)
                .unwrap()
                - mm * 0.001)
                .abs()
                < 1e-15
        );
        assert_eq!(material.thermal_resistance().unwrap(), r);
        assert_eq!(
            authored.properties.contains_key("thread_density_warp"),
            !washed
        );
        if !washed {
            assert_eq!(
                normalized
                    .require("thread_density_warp", "1/m", false)
                    .unwrap(),
                2700.0
            );
            assert_eq!(
                normalized
                    .require("thread_density_weft", "1/m", false)
                    .unwrap(),
                2000.0
            );
        }
        for absent in [
            "thickness",
            "thermal_conductivity",
            "specific_heat",
            "vapor_resistance",
            "warp_stiffness",
            "friction_skin",
            "friction_self",
        ] {
            assert!(
                !normalized.properties.contains_key(absent),
                "Must not invent {absent}"
            );
        }
        assert!(material.mechanics().is_err());
        assert!(ThermalLaw::from_material(normalized, false).is_err());
        assert!(normalized.thermal_response.is_none());
    }
}

#[test]
fn measured_five_cycle_changes_reach_geometry_mass_and_steady_heat_without_invariants() {
    let catalog = Catalog::builtin().unwrap();
    // Table 4 source columns are weft then warp; the API is warp then weft.
    for (code, warp, weft, before_mass, after_mass, before_r, after_r) in [
        (1, 4.83, 5.50, 0.33993, 0.37010, 0.0152, 0.0245),
        (2, 3.60, 5.30, 0.33983, 0.36480, 0.0148, 0.0220),
        (3, 3.50, 5.30, 0.33970, 0.36130, 0.0139, 0.0214),
        (4, 1.83, 4.60, 0.33935, 0.35560, 0.0128, 0.0195),
        (5, 1.60, 3.00, 0.33908, 0.34470, 0.0124, 0.0196),
    ] {
        let input = treatment_request(code).unwrap();
        assert_eq!(input.cycles, 5);
        assert!(input.process.contains("30 degrees C"));
        assert!(input.process.contains("40 minutes"));
        assert!(input.process.contains("100 g standard 3-ECE"));
        assert_eq!(input.shrinkage_warp.value, warp);
        assert_eq!(input.shrinkage_weft.value, weft);
        assert_eq!(input.shrinkage_warp.unit, "%");
        let report = material_treatment::evaluate(&input).unwrap();
        assert!((report.stretch[0] - (1.0 - warp / 100.0)).abs() < 1e-15);
        assert!((report.stretch[1] - (1.0 - weft / 100.0)).abs() < 1e-15);
        assert!((report.area_scale - (1.0 - warp / 100.0) * (1.0 - weft / 100.0)).abs() < 1e-15);
        assert_eq!(
            report.properties_no_longer_available,
            ["thread_density_warp", "thread_density_weft"]
        );
        let before = catalog.resolve(&report.before).unwrap();
        let after = catalog.resolve(&report.after).unwrap();
        assert!(
            (before
                .material()
                .require("areal_density", "kg/m2", false)
                .unwrap()
                - before_mass)
                .abs()
                < 1e-15
        );
        assert!(
            (after
                .material()
                .require("areal_density", "kg/m2", false)
                .unwrap()
                - after_mass)
                .abs()
                < 1e-15
        );
        // Actual observed gsm is authoritative; shrinking geometry does not enforce mass invariance.
        assert!((after_mass * report.area_scale - before_mass).abs() > 0.005);
        let a = ThermalStack::evaluate(&[before.material()], 0.6, 35.0, 20.0, 0.08, false).unwrap();
        let b = ThermalStack::evaluate(&[after.material()], 0.6, 35.0, 20.0, 0.08, false).unwrap();
        // This numerical comparison fixes area and boundaries; it is not a wearer-comfort test.
        assert!((a.heat_flow_w - 9.0 / (0.08 + before_r)).abs() < 1e-10);
        assert!((b.heat_flow_w - 9.0 / (0.08 + after_r)).abs() < 1e-10);
        assert!(b.heat_flow_w < a.heat_flow_w);
        assert_ne!(before.sha256(), after.sha256());
        let encoded = serde_json::to_string(&input).unwrap();
        let decoded: Input = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            material_treatment::evaluate(&decoded).unwrap().input_sha256,
            report.input_sha256
        );
    }
}

#[test]
fn frozen_source_requests_refuse_relabeling_and_modified_material_data() {
    assert!(treatment_request(0).is_err());
    assert!(treatment_request(6).is_err());
    let original = treatment_request(1).unwrap();
    let mut changed_cycles = original.clone();
    changed_cycles.cycles = 1;
    assert!(material_treatment::evaluate(&changed_cycles).is_err());
    let mut changed_state = original;
    changed_state
        .after
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .value = 0.03;
    assert!(material_treatment::evaluate(&changed_state).is_err());
}

#[test]
fn portable_measured_requests_replay_against_the_canonical_source_adapter() {
    for (code, json) in [
        (
            1,
            include_str!(
                "../../../tests/fixtures/materials/kanat-ozdil-2026-denim-1-treatment.json"
            ),
        ),
        (
            5,
            include_str!(
                "../../../tests/fixtures/materials/kanat-ozdil-2026-denim-5-treatment.json"
            ),
        ),
    ] {
        let saved: Input = serde_json::from_str(json).unwrap();
        let replay = material_treatment::evaluate(&saved).unwrap();
        let current = material_treatment::evaluate(&treatment_request(code).unwrap()).unwrap();
        assert_eq!(replay.input_sha256, current.input_sha256);
        assert_eq!(replay.before_sha256, current.before_sha256);
        assert_eq!(replay.after_sha256, current.after_sha256);
        assert_eq!(saved.specimen, format!("KanatOzdil2026:fabric-{code}"));
        assert_eq!(saved.cycles, 5);
    }
}

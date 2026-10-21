//! Source-table and existing-consumer checks for 33 measured neoprene specimen groups.
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection, property_unit},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, ThermalStack},
};

fn specimen(catalog: &Catalog, mark: &str) -> ResolvedMaterial {
    catalog
        .resolve(&Selection {
            id: format!("zagreb/neoprene-2025/{}", mark.to_ascii_lowercase()),
            ..Selection::default()
        })
        .unwrap()
}

#[test]
fn source_groups_keep_separate_test_protocols_and_unqualified_properties_absent() {
    let catalog = Catalog::builtin().unwrap();
    let entries: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("zagreb/neoprene-2025/"))
        .collect();
    assert_eq!(entries.len(), 33);
    assert_eq!(entries.iter().filter(|m| m.form == Form::Sheet).count(), 15);
    for authored in entries {
        let resolved = specimen(&catalog, &authored.structure["source_sample_mark"]);
        let m = resolved.material();
        assert_eq!(m.properties.len(), 7);
        assert!(m.composition.is_empty() && !m.composition_complete);
        assert!(m.properties.values().all(|q| {
            q.evidence == Evidence::Measured && q.bounds.is_none() && q.source == "paper"
        }));
        assert_eq!(
            m.sources["paper"].reference,
            "https://www.mdpi.com/2073-4360/17/21/2820"
        );
        assert_eq!(m.sources["paper"].accessed_on, "2026-10-19");
        let thickness = &m.properties["thickness"];
        assert_eq!(thickness.conditions["pressure_pa"], "1000");
        assert_eq!(thickness.conditions["sample_area_cm2"], "20");
        assert_eq!(thickness.conditions["measurement_count"], "10");
        let thermal = &m.properties["thermal_resistance"];
        assert_eq!(thermal.conditions["ambient_temperature_c"], "20");
        assert_eq!(thermal.conditions["relative_humidity_percent"], "65 +/- 1");
        assert_eq!(thermal.conditions["air_speed_m_s"], "1");
        assert_eq!(thermal.conditions["plate_temperature_c"], "Not reported");
        assert!(!thermal.conditions.contains_key("pressure_pa"));
        for (kpa, pa) in [
            (50, "50000"),
            (100, "100000"),
            (150, "150000"),
            (200, "200000"),
        ] {
            let key = format!("compression_displacement_{kpa}kpa");
            let q = &m.properties[&key];
            assert_eq!(q.unit, "mm");
            assert_eq!(q.conditions["pressure_pa"], pa);
            assert_eq!(q.conditions["measurement_count"], "5");
            assert!(q.conditions["displacement_reference"].contains("zero not specified"));
            assert_eq!(
                q.conditions["specimen_group"],
                thermal.conditions["specimen_group"]
            );
            assert_eq!(property_unit(&key), None);
        }
        assert_eq!(
            m.form,
            if m.structure["source_sample_mark"].contains("NO") {
                Form::Sheet
            } else {
                Form::Laminate
            }
        );
        assert!(!m.structure.contains_key("compression_model"));
        assert!(ConductionLaw::from_material(m, false).is_ok());
        assert!(ThermalLaw::from_material(m, true).is_err());
        assert!(m.mechanics(true).is_err());
        assert!(m.membrane_response.is_none());
        assert!(m.membrane_relaxation.is_none());
        assert!(m.thermal_response.is_none());
        assert!(m.optical_faces.is_none());
        for absent in [
            "specific_heat",
            "thermal_conductivity",
            "emissivity",
            "warp_stiffness",
        ] {
            assert!(!m.properties.contains_key(absent));
        }
    }
}

#[test]
fn independently_transcribed_rows_retain_original_units_and_pressure_displacements() {
    let catalog = Catalog::builtin().unwrap();
    for (mark, thickness, mass, resistance, displacements) in [
        (
            "M1OSa3",
            3.5900,
            826.61,
            0.0575,
            [0.9480, 1.4424, 1.7454, 2.2409],
        ),
        (
            "M3NOb5",
            5.5167,
            1288.65,
            0.1720,
            [2.1198, 3.0483, 3.5628, 3.7701],
        ),
        (
            "M4BS7",
            7.7367,
            1538.36,
            0.2055,
            [2.9671, 4.2378, 4.9239, 5.2902],
        ),
        (
            "M4OS5",
            5.5733,
            1093.46,
            0.1770,
            [1.8857, 2.7433, 3.2482, 3.6741],
        ),
    ] {
        let selected = specimen(&catalog, mark);
        let authored = selected.authored();
        let normalized = selected.material();
        assert_eq!(authored.properties["thickness"].unit, "mm");
        assert_eq!(authored.properties["thickness"].value, thickness);
        assert_eq!(authored.properties["areal_density"].unit, "g/m2");
        assert_eq!(authored.properties["areal_density"].value, mass);
        assert!(
            (normalized.require("thickness", "m", false).unwrap() - thickness * 0.001).abs()
                < 1e-15
        );
        assert!(
            (normalized.require("areal_density", "kg/m2", false).unwrap() - mass * 0.001).abs()
                < 1e-15
        );
        assert_eq!(normalized.thermal_resistance(false).unwrap(), resistance);
        for (kpa, displacement) in [50, 100, 150, 200].into_iter().zip(displacements) {
            let key = format!("compression_displacement_{kpa}kpa");
            assert_eq!(authored.properties[&key].value, displacement);
            assert_eq!(normalized.properties[&key].value, displacement);
            assert_eq!(normalized.properties[&key].unit, "mm");
        }
    }
    // Table 1 is authoritative here; the narrative's 0.052 is not the table's value.
    let discrepant = specimen(&catalog, "M3NOa3");
    assert_eq!(
        discrepant.material().thermal_resistance(false).unwrap(),
        0.0535
    );
    assert!(
        discrepant.material().sources["paper"]
            .note
            .contains("narrative gives 0.052")
    );
}

#[test]
fn substituting_nominal_five_mm_neoprene_changes_the_existing_steady_solution() {
    let catalog = Catalog::builtin().unwrap();
    let lining = specimen(&catalog, "M1NO3");
    let mut powers = Vec::new();
    // Independently transcribed Table 1 resistances. These fixed area/film/temperature values
    // are synthetic scenario inputs, not a recreation of the source apparatus or diver exposure.
    for (mark, resistance) in [("M1NO5", 0.0915), ("M3NOb5", 0.1720), ("M4OS5", 0.1770)] {
        let selected = specimen(&catalog, mark);
        let solution = ThermalStack::evaluate(
            &[lining.material(), selected.material()],
            0.6,
            35.0,
            20.0,
            0.08,
            false,
        )
        .unwrap();
        assert!((solution.heat_flow_w - 9.0 / (0.08 + 0.0765 + resistance)).abs() < 1e-10);
        let law = ConductionLaw::from_material(selected.material(), false).unwrap();
        assert_eq!(law.resistance_at(25.0).unwrap(), resistance);
        assert!((law.flux_between(35.0, 20.0).unwrap() - 15.0 / resistance).abs() < 1e-10);
        powers.push(solution.heat_flow_w);
    }
    assert!(powers[0] > powers[1] && powers[1] > powers[2]);
}

#[test]
fn offline_snapshot_locks_separate_pressure_facts_without_creating_a_pressure_law() {
    let catalog = Catalog::builtin().unwrap();
    let selected = specimen(&catalog, "M3NOb5");
    let frozen = selected.frozen_selection();
    let offline = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    assert_eq!(
        offline.resolve(&frozen).unwrap().sha256(),
        selected.sha256()
    );
    let mut altered = frozen;
    altered
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("compression_displacement_50kpa")
        .unwrap()
        .conditions
        .insert("pressure_pa".into(), "1000".into());
    assert!(offline.resolve(&altered).is_err());
}

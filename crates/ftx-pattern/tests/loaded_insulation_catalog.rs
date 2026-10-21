//! Source-table regression and existing-consumer checks for fixed-force insulation states.
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form, ThermalStack},
};

fn state(catalog: &Catalog, specimen: &str, force: &str) -> ResolvedMaterial {
    catalog
        .resolve(&Selection {
            id: format!("zagreb/rogale-2023/{specimen}/force-{force}n"),
            ..Selection::default()
        })
        .unwrap()
}

#[test]
fn published_states_qualify_conduction_without_inventing_heat_storage_or_mechanics() {
    let catalog = Catalog::builtin().unwrap();
    let states: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("zagreb/rogale-2023/"))
        .collect();
    assert_eq!(states.len(), 6);
    for authored in states {
        let resolved = catalog
            .resolve(&Selection {
                id: authored.id.clone(),
                ..Selection::default()
            })
            .unwrap();
        let m = resolved.material();
        assert_eq!(m.form, Form::Sheet);
        assert!(m.properties.values().all(|q| q.bounds.is_none()));
        assert_eq!(
            m.properties["thermal_resistance"].evidence,
            Evidence::Measured
        );
        assert_eq!(m.properties["areal_density"].evidence, Evidence::Published);
        assert!(ConductionLaw::from_material(m, false).is_ok());
        assert!(ThermalLaw::from_material(m, true).is_err());
        assert!(m.mechanics(true).is_err());
        for absent in [
            "specific_heat",
            "thermal_conductivity",
            "pressure_pa",
            "warp_stiffness",
        ] {
            assert!(!m.properties.contains_key(absent));
        }
        assert!(m.thermal_response.is_none());
        assert_eq!(
            m.sources["paper"].reference,
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC10224179/"
        );
        assert_eq!(m.sources["paper"].accessed_on, "2026-10-18");
        for q in m
            .properties
            .values()
            .filter(|q| q.conditions.contains_key("measurement_force_n"))
        {
            assert!(
                q.conditions["loaded_contact_area"].contains("cannot be converted to pressure")
            );
        }
        assert_eq!(
            catalog
                .resolve(&resolved.frozen_selection())
                .unwrap()
                .sha256(),
            resolved.sha256()
        );
        if m.structure["source_sample_mark"] == "M4" {
            assert!(m.composition.is_empty() && !m.composition_complete);
            assert!(m.structure["layer_composition_and_mass"].contains("polypropylene"));
        } else {
            assert!(m.composition_complete);
            assert_eq!(m.composition[0].fiber, "polyester");
            assert_eq!(m.composition[0].fraction, 1.0);
        }
    }
}

#[test]
fn measured_reference_and_displacement_survive_estimated_loaded_thickness() {
    let catalog = Catalog::builtin().unwrap();
    for (id, mass, initial_mm, displacement_mm, loaded_m) in [
        ("m1", 0.2988, 2.92, 0.67, 0.00225),
        ("m2", 0.3159, 3.28, 0.69, 0.00259),
        ("m4", 0.2783, 5.36, 3.11, 0.00225),
    ] {
        let initial = state(&catalog, id, "0.95");
        let compressed = state(&catalog, id, "9.5");
        let a = initial.material();
        let b = compressed.material();
        assert!((a.require("areal_density", "kg/m2", false).unwrap() - mass).abs() < 1e-15);
        assert_eq!(
            a.properties["areal_density"].value,
            b.properties["areal_density"].value
        );
        assert_eq!(initial.authored().properties["areal_density"].unit, "g/m2");
        assert_eq!(initial.authored().properties["thickness"].unit, "mm");
        assert!((a.require("thickness", "m", false).unwrap() - initial_mm * 0.001).abs() < 1e-15);
        assert!(b.require("thickness", "m", false).is_err());
        assert!((b.require("thickness", "m", true).unwrap() - loaded_m).abs() < 1e-15);
        assert_eq!(b.properties["thickness"].evidence, Evidence::Estimate);
        for (key, value, force) in [
            ("reference_thickness_0p95n", initial_mm, "0.95"),
            ("compression_displacement", displacement_mm, "9.5"),
        ] {
            let observation = &b.properties[key];
            assert_eq!(observation.value, value);
            assert_eq!(observation.unit, "mm");
            assert_eq!(observation.evidence, Evidence::Measured);
            assert_eq!(observation.conditions["measurement_force_n"], force);
        }
        assert_eq!(
            b.properties["thickness"].conditions["reference_force_n"],
            "0.95"
        );
        assert_ne!(initial.sha256(), compressed.sha256());
        let mut tampered = compressed.frozen_selection();
        tampered
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("thermal_resistance")
            .unwrap()
            .conditions
            .insert("measurement_force_n".into(), "0.95".into());
        assert!(catalog.resolve(&tampered).is_err());
    }
}

#[test]
fn loaded_state_marker_is_frozen_without_promoting_force_to_pressure() {
    let catalog = Catalog::builtin().unwrap();
    for id in ["m1", "m2", "m4"] {
        let initial = state(&catalog, id, "0.95");
        let compressed = state(&catalog, id, "9.5");
        assert!(
            !initial
                .material()
                .structure
                .contains_key("compression_model")
        );
        for material in [compressed.authored(), compressed.material()] {
            assert_eq!(
                material.structure["compression_model"],
                "fixed_force_observation/1"
            );
            assert_eq!(material.structure["selected_force_n"], "9.5");
            assert!(!material.structure.contains_key("compression_pressure_pa"));
        }
        let frozen = compressed.frozen_selection();
        assert_eq!(
            catalog.resolve(&frozen).unwrap().sha256(),
            compressed.sha256()
        );
        // Removing the manufacturing restriction changes the source-locked material identity.
        let mut tampered = frozen;
        tampered
            .record
            .as_mut()
            .unwrap()
            .structure
            .remove("compression_model");
        assert!(catalog.resolve(&tampered).is_err());
    }
}

#[test]
fn measured_load_state_substitution_changes_existing_heat_flow_at_fixed_boundaries() {
    let catalog = Catalog::builtin().unwrap();
    // Independent Table 3 Rct values. The area and temperatures below are test boundary choices,
    // not the article's unreported loaded area or test temperatures.
    for (id, r_initial, r_compressed) in [
        ("m1", 0.0172, 0.0164),
        ("m2", 0.1045, 0.0853),
        ("m4", 0.2229, 0.1095),
    ] {
        let initial = state(&catalog, id, "0.95");
        let compressed = state(&catalog, id, "9.5");
        let solve = |m| ThermalStack::evaluate(&[m], 0.6, 35.0, 20.0, 0.08, false).unwrap();
        let a = solve(initial.material());
        let b = solve(compressed.material());
        assert!((a.heat_flow_w - 9.0 / (0.08 + r_initial)).abs() < 1e-10);
        assert!((b.heat_flow_w - 9.0 / (0.08 + r_compressed)).abs() < 1e-10);
        assert!(b.heat_flow_w > a.heat_flow_w);
        // Directly observed R remains usable without permission for unrelated estimated thickness.
        let law = ConductionLaw::from_material(compressed.material(), false).unwrap();
        assert_eq!(law.resistance_at(25.0).unwrap(), r_compressed);
        assert!((law.flux_between(35.0, 20.0).unwrap() - 15.0 / r_compressed).abs() < 1e-10);
    }
}

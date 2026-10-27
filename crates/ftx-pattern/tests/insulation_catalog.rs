//! Primary-source import and shared-consumer regression, not new empirical measurements.

use ftx_pattern::{
    material_catalog::Selection,
    materials::{Evidence, ThermalStack},
    textile_import::catalog,
};

#[test]
fn primary_supplier_rows_resolve_with_their_original_conditions_and_missing_data() {
    let db = catalog().unwrap();
    for (id, mass, cm, resistance) in [
        ("c100", 0.105, 0.70, 0.20),
        ("c200", 0.210, 1.4, 0.39),
        ("ec80", 0.080, 1.1, 0.25),
        ("ec150", 0.150, 1.8, 0.39),
        ("fx70", 0.074, 0.50, 0.12),
        ("fx100", 0.105, 0.55, 0.14),
        ("g60", 0.060, 0.8, 0.22),
        ("g80", 0.080, 1.1, 0.29),
        ("g100", 0.100, 1.3, 0.34),
        ("g120", 0.120, 1.5, 0.38),
        ("g150", 0.150, 1.7, 0.43),
        ("g200", 0.200, 2.0, 0.51),
        ("b100", 0.105, 0.20, 0.061),
        ("b200", 0.210, 0.40, 0.14),
        ("b400", 0.420, 0.80, 0.28),
        ("b600", 0.620, 1.20, 0.42),
    ] {
        let m = db
            .resolve(&Selection {
                id: format!("3m_thinsulate_{id}_2020"),
                ..Selection::default()
            })
            .unwrap();
        assert_eq!(m.thermal_resistance().unwrap(), resistance);
        assert!((m.material().properties["areal_density"].value - mass).abs() < 1e-12);
        assert!((m.material().properties["thickness"].value - cm * 0.01).abs() < 1e-12);
        assert_eq!(m.authored().properties["thickness"].unit, "cm");
        assert!(
            m.authored().properties["thickness"].conditions["thickness_test_pressure"]
                .contains("0.002 psi")
        );
        assert!(
            !m.authored().properties["thermal_resistance"]
                .conditions
                .contains_key("thickness_test_pressure")
        );
        assert_eq!(
            m.authored().properties["thermal_resistance"].evidence,
            Evidence::Published
        );
        assert!(!m.material().properties.contains_key("specific_heat"));
        assert!(!m.material().properties.contains_key("warp_stiffness"));
        assert!(m.mechanics().is_err());
        assert_eq!(
            db.resolve(&m.frozen_selection()).unwrap().sha256(),
            m.sha256()
        );
    }
}

#[test]
fn mid_loft_and_precompressed_insulations_change_shared_heat_results_and_mass_observations() {
    let db = catalog().unwrap();
    let resolve = |grade| {
        db.resolve(&Selection {
            id: format!("3m_thinsulate_{grade}_2020"),
            ..Selection::default()
        })
        .unwrap()
    };
    let mid_loft = resolve("g100");
    let footwear = resolve("b100");
    let solve = |m| ThermalStack::evaluate(&[m], 1.2, 30.0, 0.0, 0.10, false).unwrap();
    let mid_loft_heat = solve(mid_loft.material());
    let footwear_heat = solve(footwear.material());
    assert!((mid_loft_heat.heat_flow_w - 36.0 / 0.44).abs() < 1e-10);
    assert!((footwear_heat.heat_flow_w - 36.0 / 0.161).abs() < 1e-10);
    assert!(mid_loft_heat.heat_flow_w < footwear_heat.heat_flow_w);
    assert!(
        mid_loft.material().properties["areal_density"].value
            < footwear.material().properties["areal_density"].value
    );
    assert_eq!(mid_loft.material().properties["width"].value, 1.52);
    assert_eq!(footwear.material().properties["width"].value, 1.52);
    assert_eq!(mid_loft.authored().composition[0].fiber, "polyester");
    assert_eq!(mid_loft.authored().composition[0].fraction, 1.0);
    assert!(mid_loft.authored().composition_complete);
    // The 2020 B source supplies no composition: do not borrow the older B/BQ/BZ table.
    assert!(footwear.authored().composition.is_empty());
    assert!(!footwear.authored().composition_complete);
    assert_eq!(
        footwear.authored().properties["thickness"].conditions["material_state"],
        "pre-compressed Type B product; not an uncompressed precursor"
    );
    for material in [&mid_loft, &footwear] {
        assert!(material.authored().properties["thermal_resistance"].bounds.is_none());
        assert!(!material.material().properties.contains_key("thermal_conductivity"));
        assert!(!material.material().properties.contains_key("specific_heat"));
        let source_id = &material.authored().properties["thermal_resistance"].source;
        let source = &material.authored().sources[source_id];
        assert_eq!(source.accessed_on, "2026-10-26");
        assert!(source.note.contains("SHA-256"));
    }
}

#[test]
fn actual_supplier_material_swap_changes_the_existing_static_thermal_consumer() {
    let db = catalog().unwrap();
    let a = db
        .resolve(&Selection {
            id: "3m_thinsulate_c100_2020".into(),
            ..Selection::default()
        })
        .unwrap();
    let b = db
        .resolve(&Selection {
            id: "3m_thinsulate_c200_2020".into(),
            ..Selection::default()
        })
        .unwrap();
    let solve = |m| ThermalStack::evaluate(&[m], 1.2, 30.0, 0.0, 0.10, false).unwrap();
    let qa = solve(a.material());
    let qb = solve(b.material());
    assert!((qa.heat_flow_w - 120.0).abs() < 1e-10);
    assert!((qb.heat_flow_w - 36.0 / 0.49).abs() < 1e-10);
    assert!(qb.heat_flow_w < qa.heat_flow_w);
    // Recycled-content marketing does not fill in the polymer mixture, heat capacity or elasticity.
    let ec = db.get("3m_thinsulate_ec80_2020").unwrap();
    assert!(ec.composition.is_empty());
    assert!(!ec.composition_complete);
}

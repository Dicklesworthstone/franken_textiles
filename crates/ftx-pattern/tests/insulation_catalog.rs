//! Primary-source import and shared-consumer regression, not new empirical measurements.

use ftx_pattern::{
    material_catalog::Selection,
    materials::{Evidence, ThermalStack},
    textile_import::catalog,
};

#[test]
fn six_primary_supplier_rows_resolve_with_their_original_conditions_and_missing_data() {
    let db = catalog().unwrap();
    for (id, mass, cm, resistance) in [
        ("c100", 0.105, 0.70, 0.20),
        ("c200", 0.210, 1.4, 0.39),
        ("ec80", 0.080, 1.1, 0.25),
        ("ec150", 0.150, 1.8, 0.39),
        ("fx70", 0.074, 0.50, 0.12),
        ("fx100", 0.105, 0.55, 0.14),
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

//! Independently transcribed table answers, not regenerated from the built-in record builder.
use ftx_pattern::{
    material_catalog::Selection,
    material_thermal::ThermalLaw,
    materials::{Catalog, Evidence, Form},
};

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10 * (1.0 + b.abs()), "{a} != {b}");
}

#[test]
fn nist_rows_preserve_geometry_and_celsius_equations_in_si() {
    let db = Catalog::builtin().unwrap();
    let rows = [
        ("aralite", 3.59, 74.2, 0.0454, Some(1693.0)),
        ("breathe-tex", 1.22, 120.7, 0.0432, Some(2218.6)),
        ("breathe-tex-plus", 1.12, 179.4, 0.0432, Some(2082.3)),
        ("cotton-duck", 1.32, 518.9, 0.0990, None),
        ("nomex-e89-crosstech", 0.96, 143.1, 0.0449, None),
        ("neo-guard", 0.55, 597.4, 0.1041, Some(1471.3)),
        ("nomex-iii-defender", 0.82, 316.9, 0.0651, Some(1507.2)),
        (
            "nomex-iiia-pajama-crosstech",
            0.52,
            316.8,
            0.0587,
            Some(1975.2),
        ),
        ("pbi-kevlar-kombat", 0.80, 321.8, 0.0704, Some(946.2)),
        ("scotchlite", 0.75, 81.6, 0.1184, Some(961.9)),
    ];
    let mut admitted = 0;
    for (slug, mm, rho, k50, cp50) in rows {
        let record = db
            .resolve(&Selection {
                id: format!("nist/7282/{slug}"),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let m = record.material();
        near(record.authored().properties["thickness"].value, mm);
        assert_eq!(record.authored().properties["thickness"].unit, "mm");
        near(m.properties["thickness"].value, mm / 1000.0);
        near(m.properties["bulk_density"].value, rho);
        let response = m.thermal_response.as_ref().unwrap();
        let k = response
            .conductivity
            .as_ref()
            .unwrap()
            .compile("W/(m*K)", &m.sources, true)
            .unwrap();
        near(k.at(50.0).unwrap(), k50);
        assert_eq!(k.domain(), [20.0, 70.0]);
        match cp50 {
            Some(cp50) => {
                let cp = response
                    .specific_heat
                    .as_ref()
                    .unwrap()
                    .compile("J/(kg*K)", &m.sources, true)
                    .unwrap();
                near(cp.at(50.0).unwrap(), cp50);
                assert_eq!(cp.domain(), [0.0, 100.0]);
            }
            None => assert!(response.specific_heat.is_none()),
        }
        if let Ok(law) = ThermalLaw::from_material(m, true) {
            admitted += 1;
            near(law.resistance_at(50.0).unwrap(), mm * 0.001 / k50);
            near(
                law.capacity_at(50.0).unwrap(),
                mm * 0.001 * rho * cp50.unwrap(),
            );
            assert_eq!(law.domain(), Some([20.0, 70.0]));
            assert!(law.check_temperature(19.0).is_err());
            assert!(law.check_temperature(71.0).is_err());
        }
        assert!(record.mechanics().is_err());
        assert!(m.composition.is_empty());
    }
    assert_eq!(admitted, 7);
}

#[test]
fn sourced_estimates_and_unknown_construction_do_not_claim_measured_coefficients() {
    let db = Catalog::builtin().unwrap();
    for m in db
        .materials
        .iter()
        .filter(|m| m.id.starts_with("nist/7282/"))
    {
        assert_eq!(m.sources["nist7282"].accessed_on, "2026-10-14");
        if m.id.ends_with("cotton-duck") {
            assert_eq!(m.form, Form::Woven);
        } else {
            assert_eq!(m.form, Form::Sheet);
        }
        if let Some(mass) = m.properties.get("areal_density") {
            assert_eq!(mass.evidence, Evidence::Estimate);
            assert!(mass.method.contains("Product of Table 2"));
            assert!(mass.bounds.is_none()); // no invented covariance-derived uncertainty
        }
        let r = m.thermal_response.as_ref().unwrap();
        for s in [&r.conductivity, &r.specific_heat].into_iter().flatten() {
            assert_eq!(s.evidence, Evidence::Published);
            assert!(s.points.is_empty());
            assert!(s.polynomial.is_some());
            assert!(s.compile(&s.unit, &m.sources, false).is_err());
            assert!(s.conditions["uncertainty_scope"].contains("not uncertainty"));
        }
        assert!(ThermalLaw::from_material(m, false).is_err());
    }
}

#[test]
fn source_conflicts_are_retained_but_not_consumed_even_with_estimate_permission() {
    let db = Catalog::builtin().unwrap();
    for slug in ["cotton-duck", "nomex-e89-crosstech", "scotchlite"] {
        let r = db
            .resolve(&Selection {
                id: format!("nist/7282/{slug}"),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        let m = r.material();
        assert!(ThermalLaw::from_material(m, true).is_err());
        assert!(
            m.structure
                .contains_key("specific_heat_main_table_coefficients_j_g_k")
        );
        if slug == "scotchlite" {
            assert!(m.structure["source_conflict_areal_mass"].contains("inconsistent"));
            assert!(!m.properties.contains_key("areal_density"));
            assert_eq!(m.properties["bulk_density"].value, 81.6);
        } else {
            assert!(m.structure["source_conflict_specific_heat"].starts_with("Withheld"));
            assert!(m.thermal_response.as_ref().unwrap().specific_heat.is_none());
        }
    }
}

#[test]
fn nist_selection_replay_locks_original_coefficients_and_derived_mass() {
    let db = Catalog::builtin().unwrap();
    let r = db
        .resolve(&Selection {
            id: "nist/7282/aralite".into(),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    let frozen = r.frozen_selection();
    let decoded: Selection =
        serde_json::from_str(&serde_json::to_string(&frozen).unwrap()).unwrap();
    assert_eq!(db.resolve(&decoded).unwrap().sha256(), r.sha256());
    let mut changed = decoded;
    changed
        .record
        .as_mut()
        .unwrap()
        .thermal_response
        .as_mut()
        .unwrap()
        .specific_heat
        .as_mut()
        .unwrap()
        .polynomial
        .as_mut()
        .unwrap()
        .coefficients[0] += 1.0;
    assert!(db.resolve(&changed).is_err());
    let mut changed = frozen;
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("areal_density")
        .unwrap()
        .value *= 2.0;
    assert!(db.resolve(&changed).is_err());
}

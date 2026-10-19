//! Primary table transcriptions and existing consumer propagation, not garment validation.
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::{Basis, Capability, Request},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Evidence, Form, ThermalStack},
};
use serde_json::json;

fn resolve(catalog: &Catalog, slug: &str) -> ftx_pattern::material_catalog::ResolvedMaterial {
    catalog
        .resolve(&Selection {
            id: format!("demey/2025/{slug}"),
            ..Selection::default()
        })
        .unwrap()
}

#[test]
fn primary_table_means_reach_the_existing_sparse_catalog() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("demey/2025/"))
        .collect();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows.iter().map(|m| m.properties.len()).sum::<usize>(), 24);
    // Table 1, figure columns a..h; compared in SI independently of the private adapter.
    for (slug, mass, thickness, resistance, form) in [
        ("angora-jersey", 0.305, 0.00165, 0.0413, Form::Knit),
        ("outlast-pet-jersey", 0.180, 0.00109, 0.0259, Form::Knit),
        ("cotton-lycra-jersey", 0.255, 0.00250, 0.0350, Form::Knit),
        ("cotton-jersey", 0.225, 0.00150, 0.0368, Form::Knit),
        (
            "linen65-cotton35-woven",
            0.354,
            0.00090,
            0.0164,
            Form::Woven,
        ),
        (
            "wool70-linen20-cotton10-woven",
            0.559,
            0.00146,
            0.0289,
            Form::Woven,
        ),
        (
            "linen60-cotton40-woven",
            0.400,
            0.00075,
            0.0145,
            Form::Woven,
        ),
        (
            "cotton70-linen30-chenille",
            0.466,
            0.00125,
            0.0235,
            Form::Woven,
        ),
    ] {
        let snapshot = resolve(&catalog, slug);
        let material = snapshot.material();
        assert_eq!(material.form, form);
        assert!((material.properties["areal_density"].value - mass).abs() < 1e-15);
        assert!((material.properties["thickness"].value - thickness).abs() < 1e-15);
        assert_eq!(snapshot.thermal_resistance().unwrap(), resistance);
        assert_eq!(snapshot.authored().properties["areal_density"].unit, "g/m2");
        assert_eq!(snapshot.authored().properties["thickness"].unit, "mm");
        assert!(
            material
                .properties
                .values()
                .all(|q| q.evidence == Evidence::Measured)
        );
        assert!(material.properties.values().all(|q| q.bounds.is_none()));
        assert!((material.composition.iter().map(|c| c.fraction).sum::<f64>() - 1.0).abs() < 1e-15);
        assert!(material.composition_complete);
        assert!(ConductionLaw::from_material(material, false).is_ok());
        assert!(ThermalLaw::from_material(material, true).is_err());
        assert!(snapshot.mechanics().is_err());
        for missing in [
            "specific_heat",
            "thermal_conductivity",
            "friction_skin",
            "warp_stiffness",
        ] {
            assert!(!material.properties.contains_key(missing));
        }
        assert!(material.thermal_response.is_none());
    }
}

#[test]
fn hotplate_and_gauge_protocols_stay_distinct_in_locked_replay() {
    let catalog = Catalog::builtin().unwrap();
    let snapshot = resolve(&catalog, "outlast-pet-jersey");
    let material = snapshot.material();
    let mass = &material.properties["areal_density"];
    let thickness = &material.properties["thickness"];
    let resistance = &material.properties["thermal_resistance"];
    assert_eq!(mass.conditions["sample_count"], "5");
    assert_eq!(thickness.conditions["pressure_kpa"], "0.5");
    assert_eq!(thickness.conditions["presser_foot_diameter_mm"], "10");
    assert_eq!(resistance.conditions["sample_count"], "3");
    assert_eq!(
        resistance.conditions["warm_plate_temperature_c"],
        "35 +/- 0.1"
    );
    assert_eq!(
        resistance.conditions["cold_plate_temperature_c"],
        "25 +/- 0.1"
    );
    assert!(!resistance.conditions.contains_key("pressure_kpa"));
    assert!(material.structure["finish_scope"].contains("latent heat are unreported"));
    assert!(
        material.sources["paper"]
            .reference
            .ends_with("textiles5040066")
    );
    let frozen = snapshot.frozen_selection();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(empty.resolve(&frozen).unwrap().sha256(), snapshot.sha256());
    let mut tampered = frozen;
    tampered
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .conditions
        .insert("warm_plate_temperature_c".into(), "45 +/- 0.1".into());
    assert!(empty.resolve(&tampered).is_err());
    let chenille = resolve(&catalog, "cotton70-linen30-chenille");
    assert!(
        chenille.material().structure["fabric_counts_reported"]
            .contains("duplicate cotton label unresolved")
    );
}

#[test]
fn replacing_real_linen_and_knit_records_changes_steady_transport_without_other_inputs() {
    let catalog = Catalog::builtin().unwrap();
    let linen = resolve(&catalog, "linen65-cotton35-woven");
    let jersey = resolve(&catalog, "cotton-jersey");
    // Same illustrative area, exact reported 35/25 C plate temperatures and zero added film.
    // No held-out heat flux was supplied by the source; these are consumer arithmetic checks.
    let solve =
        |material| ThermalStack::evaluate(&[material], 0.2, 35.0, 25.0, 0.0, false).unwrap();
    let a = solve(linen.material());
    let b = solve(jersey.material());
    assert!((a.heat_flow_w - 2.0 / 0.0164).abs() < 1e-12);
    assert!((b.heat_flow_w - 2.0 / 0.0368).abs() < 1e-12);
    assert!(a.heat_flow_w > b.heat_flow_w);
    let law = ConductionLaw::from_material(jersey.material(), false).unwrap();
    assert!((law.flux_between(35.0, 25.0).unwrap() - 10.0 / 0.0368).abs() < 1e-12);
    assert!(law.domain().is_none()); // Single-protocol scalar semantics, not a measured range.
    assert_ne!(linen.sha256(), jersey.sha256());
}

#[test]
fn existing_screen_uses_measured_resistance_but_refuses_unreported_bounds_or_storage() {
    let catalog = Catalog::builtin().unwrap();
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":"demey/2025/",
        "constraints":[{"attribute":{"key":"thermal_resistance","unit":"m2*K/W",
            "conditions":{"warm_plate_temperature_c":"35 +/- 0.1","cold_plate_temperature_c":"25 +/- 0.1"}},
            "minimum":0.03}],
        "required_capabilities":["steady_conduction"]
    })).unwrap();
    let report = request.evaluate(&catalog).unwrap();
    assert_eq!(report.frontier.len(), 3);
    assert!(
        report
            .frontier
            .contains(&"demey/2025/angora-jersey".to_string())
    );
    assert!(
        report
            .frontier
            .contains(&"demey/2025/cotton-jersey".to_string())
    );
    assert!(
        report
            .frontier
            .contains(&"demey/2025/cotton-lycra-jersey".to_string())
    );
    assert!(!report.physical_validation_claim);
    request.basis = Basis::ReportedBounds;
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.basis = Basis::Nominal;
    request.required_capabilities = vec![Capability::TransientThermal];
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
    request.required_capabilities = vec![Capability::SteadyConduction];
    request.constraints[0]
        .attribute
        .conditions
        .insert("warm_plate_temperature_c".into(), "5".into());
    assert!(request.evaluate(&catalog).unwrap().frontier.is_empty());
}

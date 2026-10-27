//! Primary supplier observations: identity, units and honest capability gaps.

use ftx_pattern::{
    material_catalog::Selection,
    materials::{Catalog, Evidence, Form},
};

fn supplier_catalog() -> Catalog {
    Catalog::from_json(include_str!(
        "../src/material_catalog/supplier-apparel.json"
    ))
    .unwrap()
}

#[test]
fn supplier_records_enter_the_shared_catalog_and_freeze_without_invented_physics() {
    let source = supplier_catalog();
    let builtin = Catalog::builtin().unwrap();
    assert_eq!(source.materials.len(), 13);
    for record in &source.materials {
        let selected = builtin
            .resolve(&Selection {
                id: record.id.clone(),
                ..Selection::default()
            })
            .unwrap();
        assert_eq!(selected.authored().name, record.name);
        assert!(selected.mechanics().is_err(), "{}", record.id);
        assert!(selected.thermal_resistance().is_err(), "{}", record.id);
        assert_eq!(
            builtin
                .resolve(&selected.frozen_selection())
                .unwrap()
                .sha256(),
            selected.sha256()
        );
        for observation in record.properties.values() {
            assert_eq!(observation.evidence, Evidence::Published);
            assert!(!observation.conditions.is_empty());
            assert_eq!(
                record.sources[&observation.source].accessed_on,
                if record.id == "fabricmart/DCC-3844" {
                    "2026-10-27"
                } else {
                    "2026-10-18"
                }
            );
        }
    }
}

#[test]
fn fabricmart_quarter_yard_manifest_locks_exact_knit_without_inventing_composition_or_usable_width()
{
    let catalog = supplier_catalog();
    let material = catalog
        .resolve(&Selection {
            id: "fabricmart/DCC-3844".into(),
            ..Selection::default()
        })
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/examples/fabricmart-stock-refresh.json"
    ))
    .unwrap();
    let offer = &manifest["offers"][0];
    assert_eq!(offer["material_sha256"], material.sha256());
    assert_eq!(offer["sku"], material.authored().structure["supplier_sku"]);
    assert_eq!(offer["quantity"], 0.25);
    assert_eq!(offer["minimum_quantity"], 1);
    assert_eq!(offer["order_increment"], 0.25);
    assert_eq!(offer["inventory_basis"], "displayed_yards");
    assert_eq!(material.material().form, Form::Knit);
    assert!(material.material().composition.is_empty());
    assert!(!material.material().composition_complete);
    assert_eq!(material.authored().properties["width"].value, 58.0);
    assert_eq!(material.material().properties["width"].value, 58.0 * 0.0254);
    assert_eq!(material.material().properties.len(), 1);
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(
        empty
            .resolve(&material.frozen_selection())
            .unwrap()
            .sha256(),
        material.sha256()
    );
}

#[test]
fn exact_style_widths_and_original_mass_units_reach_existing_consumers() {
    let catalog = supplier_catalog();
    for (id, mass, width) in [
        ("blackbird/FL9910", 0.210, 1.30),
        ("blackbird/FNOVAWHT", 0.110, 1.45),
        ("blackbird/FT4148999", 0.200, 1.52),
        ("organiccottonplus/INT-SOLIDS-BLACK", 0.260, 60.0 * 0.0254),
        (
            "discovery/power-stretch-94010-olive-mesh",
            0.203,
            66.0 * 0.0254,
        ),
        (
            "discovery/power-stretch-94065-black-mesh",
            0.203,
            60.0 * 0.0254,
        ),
    ] {
        let selected = catalog
            .resolve(&Selection {
                id: id.into(),
                ..Selection::default()
            })
            .unwrap();
        assert!((selected.material().properties["areal_density"].value - mass).abs() < 1e-12);
        assert!((selected.material().properties["width"].value - width).abs() < 1e-12);
        assert_eq!(selected.authored().properties["areal_density"].unit, "g/m2");
    }
    let nylon = catalog
        .get("discovery/power-stretch-94010-olive-mesh")
        .unwrap();
    let polyester = catalog
        .get("discovery/power-stretch-94065-black-mesh")
        .unwrap();
    assert_eq!(nylon.composition[0].fiber, "nylon");
    assert_eq!(polyester.composition[0].fiber, "polyester");
    let bamboo = catalog.get("blackbird/FT4148999").unwrap();
    assert_eq!(bamboo.composition[0].fiber, "bamboo-viscose");
    assert_eq!(bamboo.composition[0].fraction, 0.66);
}

#[test]
fn yarn_felt_fill_and_silk_do_not_gain_missing_dimensions_or_constitutive_laws() {
    let catalog = supplier_catalog();
    let resolve = |id: &str| {
        catalog
            .resolve(&Selection {
                id: id.into(),
                ..Selection::default()
            })
            .unwrap()
    };
    let thread = resolve("organiccottonplus/THR4800-5000WHT");
    assert_eq!(thread.material().form, Form::Yarn);
    assert_eq!(
        thread.authored().properties["yarn_linear_density"].unit,
        "tex"
    );
    assert!((thread.material().properties["yarn_linear_density"].value - 35e-6).abs() < 1e-15);
    let felt = resolve("organiccottonplus/FELT400");
    assert_eq!(felt.material().form, Form::Nonwoven);
    assert_eq!(felt.material().properties["thickness"].value, 0.001);
    assert!(!felt.material().properties.contains_key("areal_density"));
    let batting = resolve("organiccottonplus/ORBY-96");
    assert_eq!(batting.material().form, Form::Fill);
    assert!(
        (batting.material().properties["areal_density"].value
            - 3.7 * 0.028_349_523_125 / (0.9144 * 0.9144))
            .abs()
            < 1e-12
    );
    assert!(!batting.material().properties.contains_key("thickness"));
    assert!(
        batting
            .authored()
            .structure
            .contains_key("thickness_reported_range")
    );
    let down = resolve("rbtr/hyperdry-850-goose-down");
    assert_eq!(down.material().form, Form::Fill);
    assert_eq!(down.material().properties.len(), 1);
    assert_eq!(down.material().properties["supplier_fill_power"].unit, "FP");
    let silk = resolve("nydesigner/CM-065");
    assert!(!silk.material().properties.contains_key("thickness"));
    assert!(!silk.material().properties.contains_key("areal_density"));
}

#[test]
fn reviewed_supplier_manifest_binds_only_exact_catalog_products_and_explicit_sale_bases() {
    let catalog = supplier_catalog();
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/examples/textile-supplier-refresh.json"
    ))
    .unwrap();
    let offers = manifest["offers"].as_array().unwrap();
    assert_eq!(offers.len(), 4);
    for offer in offers {
        let record = catalog.get(offer["material_id"].as_str().unwrap()).unwrap();
        assert_eq!(record.structure["supplier_sku"], offer["sku"]);
        assert_eq!(offer["adapter"], "jsonld_v1");
        assert_eq!(offer["basis_reviewed_on"], "2026-10-18");
        assert!(
            record
                .sources
                .values()
                .any(|s| s.reference == offer["source_url"])
        );
        assert!(offer.get("price").is_none());
        assert!(offer.get("cart_url").is_none());
        if offer["supplier"] == "Blackbird Fabrics" {
            assert_eq!(offer["quantity"], 0.5);
            assert_eq!(offer["minimum_quantity"], 0.5);
            assert_eq!(offer["order_increment"], 0.5);
            assert_eq!(offer["unit"], "linear_meter");
            assert_eq!(offer["currency"], "CAD");
            assert_eq!(offer["region"], "CA");
            assert_eq!(
                offer["required_page_text"][0],
                "All fabric is sold by the half meter."
            );
        } else {
            assert_eq!(offer["unit"], "linear_yard");
            assert_eq!(offer["quantity"], 1);
            assert_eq!(offer["currency"], "USD");
            assert_eq!(offer["region"], "US");
        }
    }
}

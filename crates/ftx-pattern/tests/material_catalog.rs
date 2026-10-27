//! Source-preserving catalog selection extends the existing material model, not a second physics API.
use ftx_pattern::{
    material_catalog::{Selection, normalize},
    materials::{Catalog, Evidence, Form},
};
use serde_json::{Value, json};

fn full() -> Catalog {
    let mut raw = json!({"schema":"ftx-textile-catalog/1","materials":[{"id":"test","name":"Artificial sheet","form":"woven","sources":{"test":{"reference":"local:synthetic-test","accessed_on":"2026-10-13","note":"Synthetic known answers, not empirical fabric measurements"}},"properties":{}}]});
    for (key, value, unit) in [
        ("areal_density", 200., "g/m2"),
        ("thickness", 1., "mm"),
        ("warp_stiffness", 8000., "N/m"),
        ("weft_stiffness", 4000., "N/m"),
        ("shear_stiffness", 600., "N/m"),
        ("bending_rigidity", 3e-5, "N*m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("friction_skin", 0.4, "1"),
        ("friction_self", 0.3, "1"),
    ] {
        raw["materials"][0]["properties"][key] = json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"Synthetic fixture chosen for tests, not calibration","conditions":{"scope":"Artificial dry sheet, no empirical validity claim"}});
    }
    Catalog::from_json(&raw.to_string()).unwrap()
}
fn selection() -> Selection {
    Selection {
        id: "test".into(),
        allow_estimates: true,
        ..Selection::default()
    }
}

#[test]
fn factual_seed_does_not_invent_physics_or_midpoints() {
    let db = Catalog::builtin().unwrap();
    assert_eq!(db.materials.len(), 275);
    for id in [
        "fsri/hfm/cotton-sheet/dry/220330-r1",
        "fsri/hfm/cotton-sheet/wet/220318-r1",
    ] {
        let run = db.get(id).unwrap();
        assert_eq!(run.properties.len(), 1);
        assert!(run.properties.contains_key("thickness"));
        assert!(
            run.thermal_response
                .as_ref()
                .unwrap()
                .specific_heat
                .is_none()
        );
        assert!(run.mechanics(true).is_err());
    }
    let linen = db.get("fs/il019-dusty-lotus-softened").unwrap();
    assert!(
        (linen.require("areal_density", "kg/m2", false).unwrap()
            - 5.3 * 0.028_349_523_125 / (0.9144 * 0.9144))
            .abs()
            < 1e-15
    );
    assert!(linen.mechanics(false).is_err());
    let leather = db.get("tandy/9114-84").unwrap();
    assert!(leather.require("thickness", "m", false).is_err());
    assert!(leather.structure.contains_key("thickness_reported_range"));
    assert!(
        !db.get("dharma/silk-charmeuse-19.5-momme")
            .unwrap()
            .properties
            .contains_key("thickness")
    );
}

#[test]
fn normalized_units_feed_existing_mechanics_and_thermal_consumers() {
    let mut db = full();
    let mut s = selection();
    s.allow_estimates = false;
    assert!(db.resolve(&s).unwrap().mechanics().is_err());
    s.allow_estimates = true;
    let m = db.resolve(&s).unwrap().mechanics().unwrap();
    assert_eq!(
        (m.areal_kg_m2, m.thickness_m, m.warp_n_m, m.weft_n_m),
        (0.2, 0.001, 8000., 4000.)
    );
    let mut q = db.materials[0].properties["thickness"].clone();
    q.value = 1.;
    q.unit = "clo".into();
    db.materials[0]
        .properties
        .insert("thermal_resistance".into(), q);
    assert_eq!(db.resolve(&s).unwrap().thermal_resistance().unwrap(), 0.155);
}

#[test]
fn overrides_preserve_originals_and_fingerprint_conditions_sources_and_policy() {
    let db = full();
    let before = serde_json::to_value(&db).unwrap();
    let s = selection();
    let a = db.resolve(&s).unwrap();
    let mut s2 = s.clone();
    let mut d = db.materials[0].properties["weft_stiffness"].clone();
    d.value = 2000.;
    s2.overrides.insert("weft_stiffness".into(), d);
    let b = db.resolve(&s2).unwrap();
    assert_ne!(a.sha256(), b.sha256());
    assert_eq!(serde_json::to_value(&db).unwrap(), before);
    assert_eq!(b.mechanics().unwrap().weft_n_m, 2000.);
    s2.overrides
        .get_mut("weft_stiffness")
        .unwrap()
        .conditions
        .insert("humidity".into(), "Different RH".into());
    assert_ne!(b.sha256(), db.resolve(&s2).unwrap().sha256());
    assert_eq!(a.sha256(), db.resolve(&s).unwrap().sha256());
    s2.sources
        .insert("test".into(), db.materials[0].sources["test"].clone());
    s2.sources.get_mut("test").unwrap().reference = "local:other".into();
    assert!(db.resolve(&s2).is_err());
}

#[test]
fn snapshot_roundtrip_is_locked_and_not_dependent_on_mutable_catalog() {
    let mut db = full();
    let r = db.resolve(&selection()).unwrap();
    let frozen = r.frozen_selection();
    let encoded = serde_json::to_string(&frozen).unwrap();
    let frozen: Selection = serde_json::from_str(&encoded).unwrap();
    db.materials[0]
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 500.;
    assert_eq!(r.sha256(), db.resolve(&frozen).unwrap().sha256());
    let mut changed = frozen.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .value = 2.;
    assert!(db.resolve(&changed).is_err());
    changed = frozen;
    changed.id = "other".into();
    assert!(db.resolve(&changed).is_err());
}

#[test]
fn wrong_dimensions_nonfinite_overflow_and_invalid_bounds_refuse() {
    for mutation in 0..5 {
        let mut db = full();
        let q = db.materials[0]
            .properties
            .get_mut("warp_stiffness")
            .unwrap();
        match mutation {
            0 => q.unit = "Pa".into(),
            1 => q.value = f64::INFINITY,
            2 => q.value = f64::NAN,
            3 => {
                q.value = f64::MAX;
                q.unit = "kN/m".into()
            }
            _ => q.bounds = Some([9000., 10000.]),
        }
        assert!(db.resolve(&selection()).is_err(), "mutation {mutation}");
    }
}

#[test]
fn unit_bounds_normalize_without_losing_original_observation() {
    let mut db = full();
    db.materials[0]
        .properties
        .get_mut("thickness")
        .unwrap()
        .bounds = Some([0.8, 1.2]);
    let r = db.resolve(&selection()).unwrap();
    assert_eq!(r.authored().properties["thickness"].unit, "mm");
    assert_eq!(
        r.authored().properties["thickness"].bounds,
        Some([0.8, 1.2])
    );
    assert_eq!(
        r.material().properties["thickness"].bounds,
        Some([0.0008, 0.0012])
    );
    let normalized = normalize(r.material()).unwrap();
    assert_eq!(
        serde_json::to_value(normalized).unwrap(),
        serde_json::to_value(r.material()).unwrap()
    );
}

#[test]
fn unknown_extensions_are_preserved_without_defaulting_missing_required_fields() {
    let mut raw = serde_json::to_value(full()).unwrap();
    let q = raw["materials"][0]["properties"]["weft_stiffness"].take();
    raw["materials"][0]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("weft_stiffness");
    raw["materials"][0]["properties"]["x-custom"] = q;
    let db = Catalog::from_json(&raw.to_string()).unwrap();
    let r = db.resolve(&selection()).unwrap();
    assert!(r.material().properties.contains_key("x-custom"));
    assert!(r.mechanics().is_err());
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v["sha256"].is_string());
}

#[test]
fn supplier_variants_preserve_distinct_construction_without_inventing_stiffness() {
    let catalog = Catalog::builtin().unwrap();
    for (id, width_in, gsm) in [
        (
            "rbtr/1.1-black-uncalendered",
            60.,
            1.1 * 33.905_747_474_882_3,
        ),
        ("rbtr/1.1-black-calendered", 58., 1.1 * 33.905_747_474_882_3),
        ("discovery/power-stretch-94010-olive-mesh", 66., 203.),
        ("discovery/power-stretch-94016-grey", 54., 203.),
        ("discovery/power-stretch-94021-a-maze", 60., 237.),
    ] {
        let resolved = catalog
            .resolve(&Selection {
                id: id.into(),
                ..Selection::default()
            })
            .unwrap();
        let material = resolved.material();
        assert!((material.require("width", "m", false).unwrap() - width_in * 0.0254).abs() < 1e-14);
        assert!(
            (material.require("areal_density", "kg/m2", false).unwrap() - gsm / 1000.).abs()
                < 1e-14
        );
        assert!(resolved.mechanics().is_err());
        assert!(!material.properties.contains_key("thickness"));
        assert!(!material.properties.contains_key("bending_rigidity"));
        assert!(!material.properties.contains_key("thermal_resistance"));
        if id.starts_with("rbtr/") {
            assert!(
                (material
                    .require("yarn_linear_density", "kg/m", false)
                    .unwrap()
                    - 20. / 9_000_000.)
                    .abs()
                    < 1e-18
            );
        } else {
            assert!(material.composition_complete);
            if id != "discovery/power-stretch-94010-olive-mesh" {
                assert!(
                    material
                        .properties
                        .contains_key("supplier_stretch_lengthwise")
                );
            }
            assert!(!material.properties.contains_key("elongation_break_warp"));
        }
    }
    let olive = catalog
        .get("discovery/power-stretch-94010-olive-mesh")
        .unwrap();
    let grey = catalog.get("discovery/power-stretch-94016-grey").unwrap();
    let maze = catalog.get("discovery/power-stretch-94021-a-maze").unwrap();
    assert_eq!(olive.composition.len(), 2);
    assert_eq!(olive.structure["record_revision"], "2026-10-18");
    assert_eq!(grey.composition.len(), 3);
    assert_eq!(maze.composition[1].fraction, 0.25);
}

#[test]
fn observed_apex_offer_retains_its_five_yard_continuous_limit_independently_of_inventory() {
    use ftx_pattern::material_cost::{Basis, ExclusionReason, QuoteBook, Scope};
    let book: QuoteBook = serde_json::from_str(include_str!(
        "../../../docs/examples/textile-continuous-cut-quotes-2026-10-21.json"
    ))
    .unwrap();
    assert_eq!(book.quotes.len(), 1);
    let q = &book.quotes[0];
    assert_eq!(q.maximum_continuous_quantity, Some(5.0));
    let material = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: q.material_id.clone(),
            ..Selection::default()
        })
        .unwrap();
    assert_eq!(q.material_sha256.as_deref(), Some(material.sha256()));
    let width = material.material().require("width", "m", false).unwrap();
    let mut scope = Scope {
        currency: q.currency.clone(),
        region: q.region.clone(),
        quality: q.quality.clone(),
        terms: q.terms.clone(),
        as_of: q.observed_on.clone(),
        as_of_at: q.observed_at.clone(),
        max_age_days: 0,
        requirement: width * 5.0,
        basis: Basis::Area,
        require_observed_quantity: true,
        continuous_length_m: Some(5.0),
        require_observed_continuous_length: true,
    };
    let too_long = book.evaluate(&material, &scope).unwrap();
    assert!(too_long.offers.is_empty());
    assert_eq!(
        too_long.excluded_offers[0].reason,
        ExclusionReason::InsufficientContinuousLength
    );
    scope.continuous_length_m = Some(4.572);
    scope.requirement = width * 4.572;
    let admitted = book.evaluate(&material, &scope).unwrap();
    assert_eq!(
        admitted.offers[0].within_observed_continuous_limit,
        Some(true)
    );
    assert_eq!(admitted.offers[0].quote.source_sha256, q.source_sha256);
}

#[test]
fn observed_supplier_offers_reach_shared_price_capacity_and_revision_consumers() {
    use ftx_pattern::material_cost::{Basis, ExclusionReason, QuoteBook, Scope};
    let catalog = Catalog::builtin().unwrap();
    let book: QuoteBook = serde_json::from_str(include_str!(
        "../../../docs/examples/textile-public-product-quotes-2026-10-20.json"
    ))
    .unwrap();
    assert_eq!(book.schema, "ftx-textile-quotes/2");
    assert_eq!(book.quotes.len(), 6);
    for quote in &book.quotes {
        let selection = Selection {
            id: quote.material_id.clone(),
            ..Selection::default()
        };
        let resolved = catalog.resolve(&selection).unwrap();
        assert_eq!(quote.material_sha256.as_deref(), Some(resolved.sha256()));
        let mut scope = Scope {
            currency: quote.currency.clone(),
            region: quote.region.clone(),
            quality: quote.quality.clone(),
            terms: quote.terms.clone(),
            as_of: "2026-10-20".into(),
            as_of_at: Some("2026-10-20T23:59:59.999999999Z".into()),
            max_age_days: 30,
            requirement: 1.0,
            basis: Basis::Area,
            require_observed_quantity: false,
            continuous_length_m: None,
            require_observed_continuous_length: false,
        };
        let sample = book.evaluate(&resolved, &scope).unwrap();
        assert_eq!(sample.offers.len(), 1);
        assert!(!sample.population_claim);
        let offer = &sample.offers[0];
        let width = resolved.material().require("width", "m", false).unwrap();
        assert!((offer.rate - quote.price / (0.9144 * width)).abs() < 1e-12);
        assert_eq!(offer.order_quantity, 1.0);
        assert_eq!(offer.order_cost, quote.price);
        scope.require_observed_quantity = true;
        let strict = book.evaluate(&resolved, &scope).unwrap();
        if let Some(capacity) = quote.available_quantity {
            assert!(strict.offers[0].quantity_sufficiency_observed);
            scope.requirement = (capacity + 0.1) * 0.9144 * width;
            let depleted = book.evaluate(&resolved, &scope).unwrap();
            assert!(depleted.offers.is_empty());
            assert_eq!(
                depleted.excluded_offers[0].reason,
                ExclusionReason::InsufficientQuantity
            );
            assert_eq!(
                depleted.excluded_offers[0].required_order_quantity,
                Some(capacity + 1.0)
            );
        } else {
            assert!(strict.offers.is_empty());
            assert_eq!(
                strict.excluded_offers[0].reason,
                ExclusionReason::UnknownQuantity
            );
        }
        scope.requirement = 1.0;
        scope.require_observed_quantity = false;
        let mut changed = selection;
        let mut width = resolved.authored().properties["width"].clone();
        width.value += 0.25;
        changed.overrides.insert("width".into(), width);
        let changed = catalog.resolve(&changed).unwrap();
        let rebound = book.evaluate(&changed, &scope).unwrap();
        assert!(rebound.offers.is_empty());
        assert_eq!(
            rebound.excluded_offers[0].reason,
            ExclusionReason::MaterialSnapshotMismatch
        );
        scope.currency = if quote.currency == "USD" {
            "CAD"
        } else {
            "USD"
        }
        .into();
        assert!(book.evaluate(&resolved, &scope).unwrap().offers.is_empty());
        scope.currency = quote.currency.clone();
        scope.as_of = "2026-10-31".into();
        scope.as_of_at = Some("2026-10-31T12:00:00Z".into());
        if quote.valid_through.is_some() {
            let expired = book.evaluate(&resolved, &scope).unwrap();
            assert!(expired.offers.is_empty());
            assert_eq!(expired.excluded_offers[0].reason, ExclusionReason::Expired);
        }
    }
}

#[test]
fn supplier_sewing_threads_normalize_tex_without_reinterpreting_strength_or_ranges() {
    let catalog = Catalog::builtin().unwrap();
    for (article, tex, strength_cn) in [
        ("2994120", 24., 1190.),
        ("2994100", 30., 1490.),
        ("2994080", 40., 1960.),
        ("2994050", 60., 3040.),
    ] {
        let resolved = catalog
            .resolve(&Selection {
                id: format!("coats/epic-{article}-2026"),
                ..Selection::default()
            })
            .unwrap();
        let authored = resolved.authored();
        let material = resolved.material();
        assert_eq!(material.form, Form::Yarn);
        assert_eq!(authored.properties["yarn_linear_density"].unit, "tex");
        assert_eq!(authored.properties["yarn_linear_density"].value, tex);
        assert!(
            (material
                .require("yarn_linear_density", "kg/m", false)
                .unwrap()
                - tex * 1e-6)
                .abs()
                < 1e-18
        );
        assert_eq!(
            material.require("put_up_length", "m", false).unwrap(),
            5000.
        );
        assert_eq!(
            material
                .require("supplier_average_thread_strength", "cN", false)
                .unwrap(),
            strength_cn
        );
        // A source-defined average has no shell-force meaning or automatic unit conversion.
        assert!(
            material
                .require("supplier_average_thread_strength", "N", false)
                .is_err()
        );
        assert!(!material.properties.contains_key("breaking_force_warp"));
        assert!(!material.properties.contains_key("breaking_force_weft"));
        assert!(!material.properties.contains_key("yarn_elongation_break"));
        assert!(material.structure.contains_key("elongation_reported_range"));
        assert!(material.composition.is_empty());
        assert!(!material.composition_complete);
        assert!(resolved.mechanics().is_err());
        assert!(resolved.thermal_resistance().is_err());
        for observation in material.properties.values() {
            assert_eq!(observation.evidence, Evidence::Published);
            assert!(observation.bounds.is_none());
            assert_eq!(observation.source, "coats-epic-2026");
            assert!(observation.conditions["source_table"].contains("page 2; August 2026"));
        }
        let source = &authored.sources["coats-epic-2026"];
        assert_eq!(source.accessed_on, "2026-10-26");
        assert!(
            source
                .note
                .contains("b08de6e730a7f3993f21219ce189b2da0038091b9976f53fe0a154687bed0ca6")
        );
        assert_eq!(
            catalog
                .resolve(&resolved.frozen_selection())
                .unwrap()
                .sha256(),
            resolved.sha256()
        );
    }
}

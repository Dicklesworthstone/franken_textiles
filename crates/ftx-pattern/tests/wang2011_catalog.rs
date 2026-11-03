//! Primary origin-table transcription, preserved scope and existing frozen-selection authority.
use ftx_pattern::{
    material_catalog::Selection,
    materials::{Catalog, Evidence},
};

#[test]
fn named_source_origins_preserve_units_composition_and_negative_entries() {
    let catalog = Catalog::builtin().unwrap();
    let records: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("wang/2011/"))
        .collect();
    assert_eq!(records.len(), 10);
    assert_eq!(
        records.iter().map(|m| m.properties.len()).sum::<usize>(),
        50
    );
    // Independently checked original Origin columns on supplementary PDF pages 1, 2 and 3.
    for (slug, mass, origin) in [
        (
            "ivory-rib-knit",
            0.276,
            [42.871021, -0.234556, 65.166023, 83.175644],
        ),
        (
            "gray-interlock",
            0.187,
            [16.593832, -14.695990, 34.477123, 36.860302],
        ),
        (
            "11oz-black-denim",
            0.324,
            [205.352005, 10.234077, 1013.88629, 53.387184],
        ),
        (
            "white-swim-solid",
            0.204,
            [47.971176, 6.834521, 104.615456, 60.773663],
        ),
        (
            "royal-target",
            0.220,
            [2016.55456, 9.170511, 2034.97644, 104.759674],
        ),
    ] {
        let m = catalog.get(&format!("wang/2011/{slug}")).unwrap();
        assert_eq!(m.require("areal_density", "kg/m2", false).unwrap(), mass);
        for (name, expected) in ["c11", "c12", "c22", "c33"].into_iter().zip(origin) {
            let q = &m.properties[&format!("wang2011_origin_{name}")];
            assert_eq!(q.value, expected);
            assert_eq!(q.unit, "N/m");
            assert_eq!(q.conditions["strain_basis"], "green_lagrange_tensor_shear");
            assert_eq!(q.conditions["source_node"], "lambda_max=0");
            assert_eq!(q.conditions["specimen"], m.id);
            assert!(q.conditions["conditioning"].contains("not reported"));
        }
    }
    for m in records {
        assert!(m.composition_complete);
        assert!((m.composition.iter().map(|c| c.fraction).sum::<f64>() - 1.0).abs() < 1e-15);
        assert!(
            m.properties
                .values()
                .all(|q| q.evidence == Evidence::Published && q.bounds.is_none())
        );
        assert!(
            m.sources["paper"]
                .reference
                .ends_with("material_parameters.pdf")
        );
        assert!(m.structure["rights"].contains("author notification"));
        for key in [
            "warp_stiffness",
            "weft_stiffness",
            "shear_stiffness",
            "poisson_warp_weft",
            "thickness",
            "bending_rigidity",
            "friction_skin",
            "friction_self",
        ] {
            assert!(
                !m.properties.contains_key(key),
                "{} unexpectedly supplies {key}",
                m.id
            );
        }
        assert!(m.membrane_response.is_none());
        assert!(m.mechanics(true).is_err());
    }
    let navy = catalog.get("wang/2011/navy-sparkle-sweat").unwrap();
    let c = |name| navy.properties[&format!("wang2011_origin_{name}")].value;
    assert!(c("c11") * c("c22") - c("c12") * c("c12") < 0.0);
    assert!(navy.structure["origin_admissibility"].contains("never project or clamp"));
}

#[test]
fn origin_evidence_and_units_survive_portable_freezing_and_tamper_checks() {
    let catalog = Catalog::builtin().unwrap();
    let selected = catalog
        .resolve(&Selection {
            id: "wang/2011/gray-interlock".into(),
            ..Selection::default()
        })
        .unwrap();
    let frozen = selected.frozen_selection();
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(empty.resolve(&frozen).unwrap().sha256(), selected.sha256());
    for change in 0..3 {
        let mut altered = frozen.clone();
        let q = altered
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("wang2011_origin_c33")
            .unwrap();
        match change {
            0 => q.value *= 0.5,
            1 => q.unit = "Pa".into(),
            _ => {
                q.conditions
                    .insert("strain_basis".into(), "engineering_shear".into());
            }
        }
        assert!(empty.resolve(&altered).is_err());
    }
}

//! Permanent history admission follows valid reference geometry and effective piece selection.
//! All geometry and material observations here are synthetic software fixtures.
use ftx_core::{Vec2, Vec3};
use ftx_drape::{ClothModel, DrapeOptions, Membrane, Solver, material};
use ftx_pattern::Pattern;
use ftx_pattern::material_membrane_plasticity::{AXES, PROPERTY_FAMILIES, PlasticMembrane};
use serde_json::{Value, json};

fn law() -> PlasticMembrane {
    PlasticMembrane::new(
        Membrane::orthotropic(1000., 600., 0., 300., 1.).q,
        [20., 30., 10.],
        [100., 80., 40.],
        [0.4; 3],
        [0.4; 3],
    )
    .unwrap()
}

fn bare() -> Solver {
    let model = ClothModel::new(
        vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)],
        &[[0, 1, 2]],
        Membrane::orthotropic(1000., 600., 0., 300., 1.),
        0.,
        0.2,
    )
    .unwrap();
    Solver::new(
        model,
        vec![Vec3::ZERO, Vec3::new(0.11, 0., 0.), Vec3::new(0., 0.1, 0.)],
        &[[true; 3]; 3],
        Vec3::ZERO,
    )
}

#[test]
fn invalid_reference_area_or_gradient_cannot_acquire_permanent_history() {
    let source = bare();
    let laws = [Some(law())];
    let mut valid = source.clone().with_membrane_plasticity(&laws).unwrap();
    assert!(valid.membrane_energy().unwrap() > 0.);
    valid.step(0.001).unwrap();
    let accepted = valid.membrane_plasticity_report().unwrap().unwrap();
    assert_eq!(accepted["accepted_steps"], 1);
    assert!(accepted["plastic_dissipation_j"].as_f64().unwrap() > 0.);

    for area in [0., -0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = source.clone();
        invalid.model.membranes[0].area = area;
        let refusal = invalid
            .with_membrane_plasticity(&laws)
            .err()
            .expect("invalid reference area must refuse before binding");
        assert!(refusal.message.contains("reference triangle"));
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (vertex, axis) in [(0, 0), (2, 1)] {
            let mut invalid = source.clone();
            invalid.model.membranes[0].g[vertex][axis] = value;
            let refusal = invalid
                .with_membrane_plasticity(&laws)
                .err()
                .expect("invalid reference gradient must refuse before binding");
            assert!(refusal.message.contains("reference triangle"));
        }
    }
    assert!(source.membrane_plasticity_report().unwrap().is_none());
}

fn selection() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
    ))
    .unwrap()
}

fn elastic_selection() -> Value {
    let mut selected = selection();
    selected["id"] = json!("synthetic-elastic-admission-control");
    selected["record"]["id"] = selected["id"].clone();
    selected["record"]["structure"]
        .as_object_mut()
        .unwrap()
        .remove("membrane_plasticity_model");
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            selected["record"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove(&format!("{family}_{axis}"));
        }
    }
    selected
}

fn pattern() -> Value {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    raw["fabric"]["material"] = selection();
    raw
}

fn refuses_softened_compression(raw: &Value) {
    let pattern = Pattern::new(raw.clone());
    assert!(material::resolve(&pattern, &DrapeOptions::default()).is_ok());
    let options = DrapeOptions {
        compression: 0.1,
        ..Default::default()
    };
    let refusal = material::resolve(&pattern, &options)
        .err()
        .expect("selected permanent material requires unsoftened compression");
    assert!(refusal.message.contains("compression factor 1"));
    assert_eq!(pattern.raw(), raw);
}

#[test]
fn permanent_default_refuses_global_compression_before_meshing() {
    refuses_softened_compression(&pattern());
}

#[test]
fn effective_piece_overrides_control_global_compression_admission() {
    let mut raw = pattern();
    let elastic = elastic_selection();
    raw["pieces"][0]["fabric"] = json!({"material":elastic.clone(),"role":"lining"});
    // Back still inherits the permanent default.
    refuses_softened_compression(&raw);

    raw["pieces"][1]["fabric"] = json!({"material":elastic.clone()});
    let all_elastic = Pattern::new(raw.clone());
    for compression in [1., 0.1] {
        let options = DrapeOptions {
            compression,
            ..Default::default()
        };
        let resolved = material::resolve(&all_elastic, &options).unwrap();
        assert_eq!(resolved.options.compression, compression);
        assert!(resolved.report["membrane_plasticity_piece_count"].is_null());
    }
    assert_eq!(all_elastic.raw(), &raw);

    // A permanent piece also requires unit compression when the default is elastic.
    raw["fabric"]["material"] = elastic;
    raw["pieces"][0]["fabric"] = json!({"material":selection()});
    refuses_softened_compression(&raw);
}

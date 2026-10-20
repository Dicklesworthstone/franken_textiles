//! Independent series-compression/work answers; every acquisition is explicitly synthetic.
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_compression::{self, stack};
use ftx_pattern::materials::{Catalog, ThermalStack};

fn input() -> stack::Input {
    let mut a: material_compression::Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    a.pressure_pa = 0.0;
    let mut b = a.clone();
    b.material.id = "synthetic-softer-stack-layer".into();
    // A: 20 -> 10 mm over 0 -> 500 Pa; B: 10 -> 6 mm over the same pressures.
    // Existing rows deliberately use different authored units (mm, cm).
    b.samples[0].properties.get_mut("thickness").unwrap().value = 10.0;
    b.samples[1].properties.get_mut("thickness").unwrap().value = 0.6;
    for row in &mut b.samples {
        row.properties.get_mut("thickness").unwrap().bounds = None;
        row.properties.get_mut("thermal_resistance").unwrap().value *= 0.5;
    }
    stack::Input {
        schema: stack::SCHEMA.into(),
        gap_m: 0.023,
        reference_area_m2: 0.25,
        direction: stack::Direction::Loading,
        layers: vec![
            stack::Layer {
                id: "outer".into(),
                preparation: a,
            },
            stack::Layer {
                id: "inner".into(),
                preparation: b,
            },
        ],
        allow_estimates: true,
    }
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-11 * b.abs().max(1.0), "{a} != {b}");
}

#[test]
fn unequal_layers_close_at_common_pressure_with_conserved_mass_and_exact_path_work() {
    let input = input();
    let original = serde_json::to_value(&input).unwrap();
    let report = stack::close(&input).unwrap();
    // Solve .030 - (2e-5 + 8e-6)*p = .023 independently of the implementation.
    near(report.pressure_pa, 250.0);
    near(report.layers[0].thickness_m, 0.015);
    near(report.layers[1].thickness_m, 0.008);
    near(report.normal_force_n, 62.5);
    near(report.reference_mass_kg.unwrap(), 0.05);
    near(report.external_work_j, 0.5 * 250.0 * 0.007 * 0.25);
    near(report.layers[0].external_work_j_m2, 0.625);
    near(report.layers[1].external_work_j_m2, 0.25);
    near(report.closure_residual_m, 0.0);
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    assert_eq!(serde_json::to_value(&report.input).unwrap(), original);
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(stack::close(&input).unwrap()).unwrap()
    );

    // Consume the actual returned, hash-locked selections, not independently reconstructed values.
    let catalog = Catalog::builtin().unwrap();
    let resolved: Vec<_> = report
        .layers
        .iter()
        .map(|layer| {
            let selected: Selection =
                serde_json::from_str(&layer.conditioned.selection_json).unwrap();
            let material = catalog.resolve(&selected).unwrap();
            near(material.mechanics().unwrap().thickness_m, layer.thickness_m);
            near(material.material().properties["areal_density"].value, 0.1);
            assert!(!material.material().properties.contains_key("bulk_density"));
            assert!(
                material
                    .authored()
                    .sources
                    .contains_key(&format!("compression-stack/{}", report.input_sha256))
            );
            material
        })
        .collect();
    let materials: Vec<_> = resolved.iter().map(|r| r.material()).collect();
    let heat = ThermalStack::evaluate(&materials, 0.25, 30.0, 10.0, 0.0, true).unwrap();
    near(report.material_resistance_m2_k_w.unwrap(), 0.5625);
    near(heat.heat_flow_w, 0.25 * 20.0 / 0.5625);
}

#[test]
fn mismatched_knots_are_integrated_piecewise_not_by_endpoint_trapezoids() {
    let mut input = input();
    let a = &mut input.layers[0].preparation;
    let mut middle = a.samples[0].clone();
    middle.pressure_pa = 100.0;
    for q in middle.properties.values_mut() {
        q.conditions.insert("pressure_pa".into(), "100".into());
        q.bounds = None;
    }
    middle.properties.get_mut("thickness").unwrap().value = 17.0;
    middle
        .properties
        .get_mut("thermal_resistance")
        .unwrap()
        .value = 0.45;
    a.samples.insert(1, middle);
    input.gap_m = 0.014375 + 0.008; // exact branch thicknesses at 250 Pa
    let report = stack::close(&input).unwrap();
    near(report.pressure_pa, 250.0);
    assert_eq!(report.bracket_pa, [100.0, 500.0]);
    // A: 50*.003 + 175*.002625; B: 125*.002. Both are work per reference area.
    near(report.layers[0].external_work_j_m2, 0.609375);
    near(report.external_work_j, (0.609375 + 0.25) * 0.25);
    assert!((report.external_work_j - 125.0 * (0.03 - input.gap_m) * 0.25).abs() > 0.01);

    // A nonzero initial preload excludes all work before that state.
    for layer in &mut input.layers {
        layer.preparation.pressure_pa = 100.0;
    }
    let preloaded = stack::close(&input).unwrap();
    near(
        preloaded.external_work_j,
        (175.0 * 0.002625 + 175.0 * 0.0012) * 0.25,
    );
}

#[test]
fn unloading_returns_signed_external_work_not_a_heat_or_dissipation_claim() {
    let mut input = input();
    input.direction = stack::Direction::Unloading;
    for layer in &mut input.layers {
        layer.preparation.pressure_pa = 500.0;
    }
    // Synthetic branch happens to be reversible; this test does not infer that for other inputs.
    let report = stack::close(&input).unwrap();
    near(report.pressure_pa, 250.0);
    near(report.external_work_j, -375.0 * 0.007 * 0.25);
    assert!(report.layers.iter().all(|r| r.closure_m < 0.0));
    assert!(report.no_claim.contains("recoverable/dissipated"));
    input.direction = stack::Direction::Loading;
    assert!(stack::close(&input).is_err());
}

#[test]
fn exact_knots_and_zero_travel_keep_observations_without_interpolation() {
    let mut input = input();
    input.gap_m = 0.01 + 0.006;
    let report = stack::close(&input).unwrap();
    assert_eq!(report.pressure_pa, 500.0);
    assert_eq!(report.bracket_pa, [500.0; 2]);
    assert!(report.layers.iter().all(|r| !r.conditioned.interpolated));
    for (layer, result) in input.layers.iter().zip(&report.layers) {
        assert_eq!(
            serde_json::to_value(&result.conditioned.material.properties["thickness"]).unwrap(),
            serde_json::to_value(&layer.preparation.samples[1].properties["thickness"]).unwrap()
        );
    }
    for layer in &mut input.layers {
        layer.preparation.pressure_pa = 500.0;
    }
    let unchanged = stack::close(&input).unwrap();
    assert_eq!(unchanged.external_work_j, 0.0);
    assert!(unchanged.layers.iter().all(|r| r.closure_m == 0.0));
}

#[test]
fn pressure_plateaus_are_ambiguous_even_at_the_first_matching_knot() {
    let mut input = input();
    for layer in &mut input.layers {
        let end = &mut layer.preparation.samples[1].properties;
        end.get_mut("thickness").unwrap().value = 2.0; // cm, same as first layer's 20 mm
        end.get_mut("thickness").unwrap().bounds = None;
        let first = layer.preparation.samples[0]
            .properties
            .get_mut("thickness")
            .unwrap();
        first.value = 20.0;
        first.bounds = None;
    }
    input.gap_m = 0.04;
    let refusal = stack::close(&input).unwrap_err();
    assert!(refusal.to_string().contains("nonunique"));
    // A stiff layer does not prevent inversion if a second layer supplies actual compliance.
    input.layers[1] = self::input().layers[1].clone();
    input.gap_m = 0.028;
    near(stack::close(&input).unwrap().pressure_pa, 250.0);
}

#[test]
fn unqualified_optional_capabilities_remain_unknown_without_blocking_closure() {
    let mut input = input();
    for row in &mut input.layers[0].preparation.samples {
        row.properties.remove("thermal_resistance");
    }
    input.layers[1]
        .preparation
        .invariants
        .remove("areal_density");
    let report = stack::close(&input).unwrap();
    near(report.pressure_pa, 250.0);
    assert!(report.material_resistance_m2_k_w.is_none());
    assert!(report.reference_mass_kg.is_none());
    assert!(report.layers[0].material_resistance_m2_k_w.is_none());
    assert!(report.layers[1].reference_mass_kg.is_none());
    assert!(report.layers.iter().all(|l| !l.unavailable.is_empty()));
}

#[test]
fn invalid_inputs_extrapolation_and_missing_source_permission_are_refused_atomically() {
    for change in [
        |i: &mut stack::Input| i.gap_m = 0.031,
        |i: &mut stack::Input| i.gap_m = 0.015,
        |i: &mut stack::Input| i.gap_m = f64::NAN,
        |i: &mut stack::Input| i.reference_area_m2 = 0.0,
        |i: &mut stack::Input| i.reference_area_m2 = f64::MAX,
        |i: &mut stack::Input| i.allow_estimates = false,
        |i: &mut stack::Input| i.layers[1].preparation.allow_estimates = false,
        |i: &mut stack::Input| i.layers[1].preparation.pressure_pa = 10.0,
        |i: &mut stack::Input| i.layers[1].preparation.material.sources.clear(),
        |i: &mut stack::Input| i.layers[1].preparation.samples.clear(),
        |i: &mut stack::Input| i.layers[1].id = i.layers[0].id.clone(),
        |i: &mut stack::Input| i.layers.clear(),
        |i: &mut stack::Input| i.layers.resize(33, i.layers[0].clone()),
    ] {
        let mut input = input();
        change(&mut input);
        assert!(stack::close(&input).is_err(), "accepted invalid case");
    }
    let mut value = serde_json::to_value(input()).unwrap();
    value["gap_mm"] = serde_json::json!(23.0);
    assert!(serde_json::from_value::<stack::Input>(value).is_err());
}

#[test]
fn area_changes_force_and_work_not_pressure_and_receipts_invalidate_stale_selections() {
    let mut input = input();
    let a = stack::close(&input).unwrap();
    input.reference_area_m2 *= 3.0;
    let b = stack::close(&input).unwrap();
    assert_eq!(a.pressure_pa, b.pressure_pa);
    near(b.external_work_j, 3.0 * a.external_work_j);
    near(b.normal_force_n, 3.0 * a.normal_force_n);
    near(
        b.reference_mass_kg.unwrap(),
        3.0 * a.reference_mass_kg.unwrap(),
    );
    assert_ne!(a.input_sha256, b.input_sha256);
    assert_ne!(
        a.layers[0].conditioned.selection.expected_sha256,
        b.layers[0].conditioned.selection.expected_sha256
    );
    input.layers.reverse();
    let reversed = stack::close(&input).unwrap();
    near(reversed.pressure_pa, b.pressure_pa);
    near(reversed.external_work_j, b.external_work_j);
    assert_eq!(reversed.layers[0].id, "inner");
}

#[test]
fn nonzero_work_cannot_silently_underflow_to_zero() {
    let mut input = input();
    input.reference_area_m2 = f64::from_bits(1);
    // Remove optional mass to isolate work representability, not a mass refusal.
    for layer in &mut input.layers {
        layer.preparation.invariants.remove("areal_density");
    }
    // Small but resolvable positive compression: pressure/area is representable,
    // while the much smaller integrated work/area product underflows.
    input.gap_m = 0.029;
    let error = stack::close(&input).unwrap_err();
    assert!(error.to_string().contains("force/work"), "{error}");
}

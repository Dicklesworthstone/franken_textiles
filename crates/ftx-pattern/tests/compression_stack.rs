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

fn bounded_input() -> stack::Input {
    let mut i = input();
    i.layers.truncate(1);
    i.gap_m = 0.015;
    i
}

#[test]
fn reported_thickness_intervals_bound_pressure_and_force_without_changing_nominal_work() {
    let mut input = bounded_input();
    let r = stack::close(&input).unwrap();
    // Lower curve: 18 -> 9 mm; upper: 22 -> 11 mm, on 0..500 Pa.
    let bounds = r.reported_bounds.unwrap();
    near(bounds.pressure_pa[0], 500.0 / 3.0);
    near(bounds.pressure_pa[1], 3500.0 / 11.0);
    near(bounds.normal_force_n[0], 125.0 / 3.0);
    near(bounds.normal_force_n[1], 875.0 / 11.0);
    near(r.pressure_pa, 250.0);
    near(r.external_work_j, 0.15625);
    assert!(r.bounds_unavailable.is_empty());
    input.reference_area_m2 *= 2.0;
    let doubled = stack::close(&input).unwrap().reported_bounds.unwrap();
    assert_eq!(doubled.pressure_pa, bounds.pressure_pa);
    assert_eq!(
        doubled.normal_force_n,
        bounds.normal_force_n.map(|f| 2.0 * f)
    );
}

#[test]
fn missing_or_domain_limited_bounds_remain_unknown_not_nominal_or_clipped() {
    let mut missing = bounded_input();
    missing.layers[0].preparation.samples[1]
        .properties
        .get_mut("thickness")
        .unwrap()
        .bounds = None;
    let r = stack::close(&missing).unwrap();
    near(r.pressure_pa, 250.0);
    assert!(r.reported_bounds.is_none());
    assert!(r.bounds_unavailable[0].contains("outer"));
    // Nominally supported, but part of the reported interval lies outside the branch or the
    // declared loading direction. Neither acquisition endpoints nor preload may clip the answer.
    for (gap, initial, direction) in [
        (0.019, 0.0, stack::Direction::Loading),
        (0.010, 0.0, stack::Direction::Loading),
        (0.015, 200.0, stack::Direction::Loading),
        (0.015, 300.0, stack::Direction::Unloading),
    ] {
        let mut input = bounded_input();
        input.gap_m = gap;
        input.layers[0].preparation.pressure_pa = initial;
        input.direction = direction;
        let r = stack::close(&input).unwrap();
        assert!(r.reported_bounds.is_none());
        assert!(r.bounds_unavailable[0].contains("no extrapolation or truncated bounds"));
    }
    let mut unloading = bounded_input();
    unloading.direction = stack::Direction::Unloading;
    unloading.layers[0].preparation.pressure_pa = 350.0;
    let r = stack::close(&unloading).unwrap();
    assert!(r.external_work_j < 0.0);
    near(r.reported_bounds.unwrap().pressure_pa[1], 3500.0 / 11.0);
}

fn replace_thickness_knots(input: &mut stack::Input, points: &[[f64; 4]]) {
    let p = &mut input.layers[0].preparation;
    let template = p.samples[0].clone();
    p.samples = points
        .iter()
        .map(|&[pressure, value, low, high]| {
            let mut row = template.clone();
            row.pressure_pa = pressure;
            for q in row.properties.values_mut() {
                q.conditions
                    .insert("pressure_pa".into(), pressure.to_string());
            }
            let q = row.properties.get_mut("thickness").unwrap();
            q.value = value;
            q.unit = "mm".into();
            q.bounds = Some([low, high]);
            row
        })
        .collect();
}

#[test]
fn monotone_envelopes_use_all_knots_and_include_independent_admissible_curves() {
    let mut input = bounded_input();
    replace_thickness_knots(
        &mut input,
        &[
            [0.0, 20.0, 18.0, 22.0],
            [100.0, 17.0, 12.0, 21.0],
            [300.0, 14.0, 13.0, 22.0],
            [500.0, 10.0, 8.0, 12.0],
        ],
    );
    // Enforce monotonicity, not monotonicity of the printed error bars:
    // lower = [18,13,13,8]; upper = [22,21,21,12] mm.
    let r = stack::close(&input).unwrap();
    let bounds = r.reported_bounds.unwrap().pressure_pa;
    near(bounds[0], 60.0);
    near(bounds[1], 1300.0 / 3.0);
    let mut checked = 0;
    // Enumerate independently chosen knot values; retain only monotone branches. This exercises
    // interior combinations too, not just a replay of the two envelopes used by the implementation.
    for code in 0..256 {
        let mut n = code;
        let values: Vec<f64> = input.layers[0]
            .preparation
            .samples
            .iter()
            .map(|row| {
                let [lo, hi] = row.properties["thickness"].bounds.unwrap();
                let value = lo + (hi - lo) * f64::from(n % 4) / 3.0;
                n /= 4;
                value
            })
            .collect();
        if values.windows(2).any(|p| p[0] < p[1]) {
            continue;
        }
        let mut candidate = input.clone();
        for (row, v) in candidate.layers[0]
            .preparation
            .samples
            .iter_mut()
            .zip(values)
        {
            row.properties.get_mut("thickness").unwrap().value = v;
        }
        let p = stack::close(&candidate).unwrap().pressure_pa;
        assert!(
            p >= bounds[0] - 1e-10 && p <= bounds[1] + 1e-10,
            "{p} outside {bounds:?}"
        );
        checked += 1;
    }
    assert!(checked > 50);
}

#[test]
fn envelope_plateaus_keep_earliest_lower_and_latest_upper_crossings() {
    let mut input = bounded_input();
    replace_thickness_knots(
        &mut input,
        &[
            [0.0, 20.0, 18.0, 22.0],
            [100.0, 17.0, 15.0, 19.0],
            [200.0, 15.0, 15.0, 15.0],
            [300.0, 13.0, 12.0, 15.0],
            [500.0, 10.0, 8.0, 12.0],
        ],
    );
    let r = stack::close(&input).unwrap();
    near(r.pressure_pa, 200.0);
    assert_eq!(r.reported_bounds.unwrap().pressure_pa, [100.0, 300.0]);
}

#[test]
fn unequal_layer_knots_and_repeated_source_plies_do_not_assume_independent_noise() {
    let mut input = bounded_input();
    let mut second = input.layers[0].clone();
    second.id = "second physical ply".into();
    input.layers.push(second);
    input.gap_m = 0.030;
    let r = stack::close(&input).unwrap();
    // Duplicating the SAME source does not shrink its interval by sqrt(2).
    let b = r.reported_bounds.unwrap();
    near(b.pressure_pa[0], 500.0 / 3.0);
    near(b.pressure_pa[1], 3500.0 / 11.0);
    // Add a consistent knot in only one layer, preserving the underlying envelopes.
    let p = &mut input.layers[1].preparation;
    let mut row = p.samples[0].clone();
    row.pressure_pa = 100.0;
    for q in row.properties.values_mut() {
        q.conditions.insert("pressure_pa".into(), "100".into());
    }
    let q = row.properties.get_mut("thickness").unwrap();
    q.value = 18.0;
    q.bounds = Some([16.2, 19.8]);
    p.samples.insert(1, row);
    let r = stack::close(&input).unwrap();
    near(
        r.reported_bounds.as_ref().unwrap().pressure_pa[0],
        b.pressure_pa[0],
    );
    near(r.reported_bounds.unwrap().pressure_pa[1], b.pressure_pa[1]);
}

#[test]
fn exact_reported_values_collapse_bounds_and_unrepresentable_force_stays_unknown() {
    let mut input = bounded_input();
    for row in &mut input.layers[0].preparation.samples {
        let q = row.properties.get_mut("thickness").unwrap();
        q.bounds = Some([q.value; 2]);
    }
    let r = stack::close(&input).unwrap();
    let b = r.reported_bounds.unwrap();
    assert_eq!(b.pressure_pa, [r.pressure_pa; 2]);
    assert_eq!(b.normal_force_n, [r.normal_force_n; 2]);
    let mut input = bounded_input();
    input.reference_area_m2 = 6e305;
    let r = stack::close(&input).unwrap();
    assert!(r.normal_force_n.is_finite());
    assert!(r.reported_bounds.is_none());
    assert!(r.bounds_unavailable[0].contains("cannot be represented"));
}

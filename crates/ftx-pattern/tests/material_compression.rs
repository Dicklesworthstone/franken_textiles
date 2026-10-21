//! Synthetic source/pressure arithmetic and the actual shared material consumers; no empirical claim.
use ftx_pattern::{
    material_catalog::Selection,
    material_compression::{Input, MODEL, MODEL_KEY, PRESSURE_KEY, condition},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, ThermalStack},
};
use serde_json::json;

fn fixture() -> Input {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap()
}

fn catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

#[test]
fn pressure_reaches_existing_mechanics_and_heat_without_changing_reference_mass() {
    let mut input = fixture();
    let original = serde_json::to_value(&input).unwrap();
    let base = ftx_pattern::material_catalog::normalize(&input.material)
        .unwrap()
        .mechanics(true)
        .unwrap();
    for (pressure, thickness, resistance) in
        [(0., 0.02, 0.5), (250., 0.015, 0.375), (500., 0.01, 0.25)]
    {
        input.pressure_pa = pressure;
        let report = condition(&input).unwrap();
        let resolved = catalog().resolve(&report.selection).unwrap();
        let material = resolved.material();
        assert_eq!(
            resolved.mechanics().unwrap(),
            ftx_pattern::materials::Mechanics {
                thickness_m: thickness,
                ..base
            }
        );
        let heat = ThermalLaw::from_material(material, true).unwrap();
        close(heat.capacity_at(20.).unwrap(), 100.);
        close(heat.sensible_change(20., 30.).unwrap(), 1000.);
        close(heat.resistance_at(20.).unwrap(), resistance);
        close(material.require("width", "m", true).unwrap(), 1.5);
        let steady = ConductionLaw::from_material(material, true).unwrap();
        close(steady.flux_between(40., 20.).unwrap(), 20. / resistance);
        let stack = ThermalStack::evaluate(&[material], 2., 40., 20., 0., true).unwrap();
        close(stack.heat_flow_w, 40. / resistance);
        assert_eq!(report.material.structure[MODEL_KEY], MODEL);
        assert_eq!(
            report.material.structure[PRESSURE_KEY],
            pressure.to_string()
        );
        let retained_mass = &report.material.properties["areal_density"];
        let base_mass = &input.material.properties["areal_density"];
        assert_eq!(
            (
                retained_mass.value,
                &retained_mass.unit,
                retained_mass.bounds
            ),
            (base_mass.value, &base_mass.unit, base_mass.bounds)
        );
        assert_eq!(
            report.discarded_properties,
            ["bulk_density", "thermal_conductivity"]
        );
        assert!(!material.properties.contains_key("bulk_density"));
    }
    input.pressure_pa = 250.;
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    let report = condition(&input).unwrap();
    assert!(report.interpolated);
    assert_eq!(report.bracket_pa, [0., 500.]);
    assert_eq!(report.pressure_range_pa, [0., 500.]);
    let loft = &report.material.properties["thickness"];
    assert_eq!(loft.unit, "m");
    assert_eq!(loft.evidence, Evidence::Estimate);
    let [lo, hi] = loft.bounds.unwrap();
    close(lo, 0.0135);
    close(hi, 0.0165);
}

#[test]
fn endpoints_preserve_authored_observations_and_exact_selection_text_replays() {
    let mut input = fixture();
    let mut signed = input.material.properties["bending_rigidity"].clone();
    signed.value = -0.0;
    input
        .material
        .properties
        .insert("bending_coupling".into(), signed);
    input.invariants.insert(
        "bending_coupling".into(),
        input.invariants["bending_rigidity"].clone(),
    );
    for index in 0..2 {
        input.pressure_pa = input.samples[index].pressure_pa;
        let report = condition(&input).unwrap();
        assert!(!report.interpolated);
        assert_eq!(report.bracket_pa, [input.pressure_pa; 2]);
        for (key, q) in &input.samples[index].properties {
            assert_eq!(
                serde_json::to_value(&report.material.properties[key]).unwrap(),
                serde_json::to_value(q).unwrap()
            );
        }
        assert!(report.selection_json.contains("\"value\":-0.0"));
        let selection: Selection = serde_json::from_str(&report.selection_json).unwrap();
        let replay = catalog().resolve(&selection).unwrap();
        assert_eq!(
            Some(replay.sha256()),
            report.selection.expected_sha256.as_deref()
        );
        assert_eq!(
            serde_json::to_value(replay.authored()).unwrap(),
            serde_json::to_value(&report.material).unwrap()
        );
        let conditions = &replay.authored().properties["areal_density"].conditions;
        assert_eq!(conditions["compression_invariance_source"], "synthetic");
        assert_eq!(conditions["compression_invariance_range_pa"], "[0.0,500.0]");
        let mut tampered = selection.clone();
        tampered
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("thickness")
            .unwrap()
            .value *= 0.5;
        assert!(catalog().resolve(&tampered).is_err());
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(condition(&input).unwrap()).unwrap()
        );
    }
}

#[test]
fn prescribed_pressure_discards_a_permanent_membrane_law_even_with_scalar_invariants() {
    use ftx_pattern::material_membrane_plasticity::{AXES, PROPERTY_FAMILIES};
    let mut input = fixture();
    let selected: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
    ))
    .unwrap();
    let material = selected.record.unwrap();
    input.material.sources.extend(material.sources);
    input.material.structure.insert(
        "membrane_plasticity_model".into(),
        material.structure["membrane_plasticity_model"].clone(),
    );
    let mut conditions = input.material.properties["warp_stiffness"]
        .conditions
        .clone();
    conditions.insert("specimen_id".into(), input.specimen.clone());
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        input.material.properties.get_mut(key).unwrap().conditions = conditions.clone();
        input.invariants.get_mut(key).unwrap().conditions = conditions.clone();
    }
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            let key = format!("{family}_{axis}");
            let mut quantity = material.properties[&key].clone();
            quantity.conditions = conditions.clone();
            input.material.properties.insert(key.clone(), quantity);
            // Individual scalar invariance does not characterize the complete pressure/history law.
            let mut declaration = input.invariants["specific_heat"].clone();
            declaration.conditions = conditions.clone();
            input.invariants.insert(key, declaration);
        }
    }
    assert!(
        input
            .material
            .compiled_membrane_plasticity(true)
            .unwrap()
            .is_some()
    );
    let original = serde_json::to_value(&input).unwrap();
    let report = condition(&input).unwrap();
    assert_eq!(report.discarded_structure, ["membrane_plasticity_model"]);
    for (family, _) in PROPERTY_FAMILIES {
        for axis in AXES {
            let key = format!("{family}_{axis}");
            assert!(!report.material.properties.contains_key(&key));
            assert!(report.discarded_properties.contains(&key));
            assert!(report.input.material.properties.contains_key(&key));
        }
    }
    let resolved = catalog().resolve(&report.selection).unwrap();
    assert!(resolved.membrane_plasticity().unwrap().is_none());
    assert!(resolved.mechanics().is_ok());
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    assert_eq!(serde_json::to_value(&report.input).unwrap(), original);
}

#[test]
fn nonlinear_thermal_and_history_models_cannot_silently_survive_scalar_conditioning() {
    let mut input = fixture();
    let nonlinear: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-response.json"
    ))
    .unwrap();
    input.material.membrane_response = nonlinear.record.unwrap().membrane_response;
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        input.material.properties.remove(key);
        input.invariants.remove(key);
    }
    let relaxation: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation.json"
    ))
    .unwrap();
    let relaxation = relaxation.record.unwrap();
    input.material.membrane_relaxation = relaxation.membrane_relaxation;
    input.material.sources.extend(relaxation.sources);
    input.material.properties.remove("thermal_resistance");
    input.material.properties.remove("thermal_conductivity");
    input.material.thermal_response = Some(serde_json::from_value(json!({
        "specimen":input.specimen, "conditioning":input.conditioning,
        "conductivity":{"unit":"W/(m*K)","evidence":"estimate","source":"synthetic", "method":"Synthetic temperature-dependent law that pressure rows do not qualify",
            "points":[{"temperature_c":0.,"mean":0.04},{"temperature_c":40.,"mean":0.06}]}
    })).unwrap());
    input.material.structure.extend([
        ("bending_model".into(), "orthotropic_normal_patch/1".into()),
        (
            "bending_plasticity_model".into(),
            "orthotropic_curvature_return_map/1".into(),
        ),
        ("future_model".into(), "unsupported/1".into()),
    ]);
    let report = condition(&input).unwrap();
    assert_eq!(
        report.discarded_responses,
        [
            "membrane_response",
            "membrane_relaxation",
            "thermal_response"
        ]
    );
    assert_eq!(
        report.discarded_structure,
        ["bending_model", "bending_plasticity_model", "future_model"]
    );
    assert!(report.material.membrane_response.is_none());
    assert!(report.material.membrane_relaxation.is_none());
    assert!(report.material.thermal_response.is_none());
    assert!(
        catalog()
            .resolve(&report.selection)
            .unwrap()
            .mechanics()
            .is_err()
    );
    assert!(report.input.material.membrane_response.is_some());
    assert!(report.input.material.membrane_relaxation.is_some());
    assert!(report.input.material.thermal_response.is_some());
    input.invariants.insert(
        "membrane_response".into(),
        input.invariants["specific_heat"].clone(),
    );
    assert!(condition(&input).is_err());
}

#[test]
fn pressure_preparation_discards_unqualified_optical_faces_and_keeps_the_original() {
    let mut input = fixture();
    let mut q = input.material.properties["areal_density"].clone();
    q.value = 0.8;
    q.unit = "1".into();
    q.bounds = None;
    q.method = "Synthetic optical observation without pressure qualification".into();
    q.conditions.insert("face".into(), "finish".into());
    q.conditions
        .insert("specimen".into(), input.specimen.clone());
    q.conditions
        .insert("conditioning".into(), input.conditioning.clone());
    input.material.properties.insert("emissivity".into(), q);
    input.material.optical_faces = Some(serde_json::from_value(json!({
        "positive":{"face":"finish", "emissivity_property":"emissivity"},
        "source":"synthetic", "evidence":"estimate", "method":"Synthetic face assignment, not pressure-qualified",
        "specimen":input.specimen, "conditioning":input.conditioning
    })).unwrap());
    let original = serde_json::to_value(&input).unwrap();
    let report = condition(&input).unwrap();
    assert!(report.material.optical_faces.is_none());
    assert!(!report.material.properties.contains_key("emissivity"));
    assert!(
        report
            .discarded_properties
            .iter()
            .any(|key| key == "emissivity")
    );
    assert_eq!(report.discarded_responses, ["optical_faces"]);
    assert!(report.input.material.optical_faces.is_some());
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    let prepared = catalog().resolve(&report.selection).unwrap();
    assert!(prepared.material().optical_faces.is_none());
    assert!(prepared.mechanics().is_ok());
    assert!(prepared.thermal_resistance().is_ok());
}

#[test]
fn domain_sources_conditions_loft_and_dimensions_refuse_without_mutation() {
    for case in 0..19 {
        let mut input = fixture();
        match case {
            0 => input.pressure_pa = -1.,
            1 => input.pressure_pa = 501.,
            2 => input.samples[1].pressure_pa = 0.,
            3 => {
                input.samples[1]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .value = 3.
            }
            4 => {
                input.samples[0]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .unit = "N".into()
            }
            5 => {
                input.samples[0]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .source = "missing".into()
            }
            6 => {
                input.samples[1]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .conditions
                    .insert("humidity".into(), "different RH".into());
            }
            7 => {
                input.samples[1]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .conditions
                    .insert("loading_branch".into(), "unloading".into());
            }
            8 => {
                input.samples[1]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .conditions
                    .insert("pressure_pa".into(), "499".into());
            }
            9 => {
                input.samples[1].properties.remove("thermal_resistance");
            }
            10 => {
                input
                    .invariants
                    .get_mut("specific_heat")
                    .unwrap()
                    .pressure_range_pa = [100., 500.]
            }
            11 => input.invariants.get_mut("specific_heat").unwrap().source = "missing".into(),
            12 => input
                .invariants
                .get_mut("specific_heat")
                .unwrap()
                .method
                .clear(),
            13 => {
                input
                    .invariants
                    .get_mut("specific_heat")
                    .unwrap()
                    .conditions
                    .insert("conditioning".into(), "wet".into());
            }
            14 => {
                input.invariants.insert(
                    "thickness".into(),
                    input.invariants["specific_heat"].clone(),
                );
            }
            15 => {
                input
                    .invariants
                    .insert("unknown".into(), input.invariants["specific_heat"].clone());
            }
            16 => {
                input.samples[0]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .bounds = Some([0., 22.])
            }
            17 => input.material.form = Form::Fiber,
            _ => {
                input.samples[0]
                    .properties
                    .get_mut("thickness")
                    .unwrap()
                    .value = 0.
            }
        }
        let original = serde_json::to_value(&input).unwrap();
        assert!(condition(&input).is_err(), "case {case}");
        assert_eq!(serde_json::to_value(&input).unwrap(), original);
    }
    for value in [f64::NAN, f64::INFINITY] {
        let mut input = fixture();
        input.pressure_pa = value;
        assert!(condition(&input).is_err());
        input.pressure_pa = 250.;
        input.samples[1]
            .properties
            .get_mut("thickness")
            .unwrap()
            .value = value;
        assert!(condition(&input).is_err());
    }
}

#[test]
fn interpolation_and_invariance_never_promote_unapproved_estimates() {
    let mut input = fixture();
    for q in input.material.properties.values_mut() {
        q.evidence = Evidence::Published;
    }
    for row in &mut input.samples {
        for q in row.properties.values_mut() {
            q.evidence = Evidence::Published;
        }
    }
    for declaration in input.invariants.values_mut() {
        declaration.evidence = Evidence::Published;
    }
    input.allow_estimates = false;
    input.pressure_pa = 0.;
    let endpoint = condition(&input).unwrap();
    assert_eq!(
        endpoint.material.properties["thickness"].evidence,
        Evidence::Published
    );
    assert!(
        catalog()
            .resolve(&endpoint.selection)
            .unwrap()
            .mechanics()
            .is_ok()
    );
    input.pressure_pa = 250.;
    assert!(condition(&input).is_err());
    input.allow_estimates = true;
    let interpolated = condition(&input).unwrap();
    assert_eq!(
        interpolated.material.properties["thickness"].evidence,
        Evidence::Estimate
    );
    input.allow_estimates = false;
    input.pressure_pa = 0.;
    input.invariants.get_mut("specific_heat").unwrap().evidence = Evidence::Estimate;
    assert!(condition(&input).is_err());
}

#[test]
fn fixed_mass_transport_authority_and_prior_conditioning_are_not_bypassed() {
    let input = fixture();
    let mut ambiguous = input.clone();
    ambiguous.invariants.insert(
        "thermal_conductivity".into(),
        input.invariants["specific_heat"].clone(),
    );
    assert!(condition(&ambiguous).is_err());
    let mut density = input.clone();
    density.invariants.insert(
        "bulk_density".into(),
        input.invariants["specific_heat"].clone(),
    );
    assert!(condition(&density).is_err());
    let mut varying_mass = input.clone();
    for row in &mut varying_mass.samples {
        row.properties.insert(
            "areal_density".into(),
            input.material.properties["areal_density"].clone(),
        );
    }
    assert!(condition(&varying_mass).is_err());
    let report = condition(&input).unwrap();
    let mut conditioned_again = input.clone();
    conditioned_again.material = report.material;
    assert!(condition(&conditioned_again).is_err());
    for key in [
        MODEL_KEY,
        PRESSURE_KEY,
        "compression_input_sha256",
        "compression_future_state",
    ] {
        let mut marked = input.clone();
        marked
            .material
            .structure
            .insert(key.into(), "preexisting state".into());
        assert!(condition(&marked).is_err());
    }
}

#[test]
fn invariance_must_cover_the_original_pressure_without_conflicting_fixed_conditions() {
    for pressure in ["250", "2000", "nonsense", "NaN"] {
        let mut input = fixture();
        input
            .invariants
            .get_mut("areal_density")
            .unwrap()
            .conditions
            .insert("pressure_pa".into(), pressure.into());
        assert!(
            condition(&input).is_err(),
            "fixed invariant pressure {pressure}"
        );
    }
    for pressure in ["nonsense", "NaN", "inf", "-1", "2000"] {
        let mut input = fixture();
        input
            .material
            .properties
            .get_mut("warp_stiffness")
            .unwrap()
            .conditions
            .insert("pressure_pa".into(), pressure.into());
        let original = serde_json::to_value(&input).unwrap();
        assert!(condition(&input).is_err(), "original pressure {pressure}");
        assert_eq!(serde_json::to_value(&input).unwrap(), original);
    }
    for pressure in ["0", "500", "2000"] {
        let mut input = fixture();
        input
            .material
            .properties
            .get_mut("warp_stiffness")
            .unwrap()
            .conditions
            .insert("pressure_pa".into(), pressure.into());
        // Evidence spanning both the source pressure and the entire requested table permits reuse.
        let range = if pressure == "2000" {
            [0., 2500.]
        } else {
            [0., 500.]
        };
        input
            .invariants
            .get_mut("warp_stiffness")
            .unwrap()
            .pressure_range_pa = range;
        let original = serde_json::to_value(&input).unwrap();
        let report = condition(&input).unwrap();
        let retained = &report.material.properties["warp_stiffness"];
        let source = &input.material.properties["warp_stiffness"];
        assert_eq!(
            (
                retained.value,
                &retained.unit,
                retained.evidence,
                &retained.source
            ),
            (source.value, &source.unit, source.evidence, &source.source)
        );
        assert_eq!(retained.conditions["pressure_pa"], "250");
        assert_eq!(
            retained.conditions["compression_invariance_range_pa"],
            serde_json::to_string(&range).unwrap()
        );
        assert_eq!(
            report.input.material.properties["warp_stiffness"].conditions["pressure_pa"],
            pressure
        );
        assert_eq!(serde_json::to_value(&input).unwrap(), original);
    }
}

#[test]
fn conductivity_and_sparse_fill_keep_the_existing_thermal_capability_boundaries() {
    let mut input = fixture();
    for (row, conductivity) in input.samples.iter_mut().zip([0.04, 0.02]) {
        let mut q = row.properties.remove("thermal_resistance").unwrap();
        q.value = conductivity;
        q.unit = "W/(m*K)".into();
        row.properties.insert("thermal_conductivity".into(), q);
    }
    let report = condition(&input).unwrap();
    let selected = catalog().resolve(&report.selection).unwrap();
    close(
        selected.material().properties["thermal_conductivity"].value,
        0.03,
    );
    close(selected.thermal_resistance().unwrap(), 0.5);
    assert!(
        report
            .discarded_properties
            .contains(&"thermal_resistance".into())
    );
    assert!(
        !report
            .material
            .properties
            .contains_key("thermal_resistance")
    );
    input.material.form = Form::Fill;
    input.invariants.clear();
    let report = condition(&input).unwrap();
    let selected = catalog().resolve(&report.selection).unwrap();
    let steady = ConductionLaw::from_material(selected.material(), true).unwrap();
    close(steady.flux_between(40., 20.).unwrap(), 40.);
    assert!(ThermalLaw::from_material(selected.material(), true).is_err());
    assert!(selected.mechanics().is_err());
    assert!(
        report
            .discarded_properties
            .contains(&"areal_density".into())
    );
    assert!(
        report
            .discarded_properties
            .contains(&"specific_heat".into())
    );
}

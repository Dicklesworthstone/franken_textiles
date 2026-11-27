//! Source/selection regressions for explicit synthetic coupled-knit materials.
//! These tests establish software admission and identity, never measured textile qualification.

use ftx_pattern::{
    material_catalog::Selection,
    material_knit::{MAX_HYSTERESIS_MODES, MODEL},
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, Material},
};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("../../../tests/fixtures/materials/synthetic-coupled-knit.json");

fn selection() -> Selection {
    serde_json::from_str(FIXTURE).unwrap()
}

fn catalog() -> Catalog {
    Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
}

fn rejected(raw: Value) -> String {
    match serde_json::from_value::<Selection>(raw) {
        Ok(selection) => catalog().resolve(&selection).err().unwrap().message,
        Err(error) => error.to_string(),
    }
}

#[test]
fn source_locked_selection_reaches_origin_mechanics_without_losing_the_nonlinear_law() {
    let selected = catalog().resolve(&selection()).unwrap();
    let law = selected.coupled_knit().unwrap().unwrap();
    assert_eq!(law.evidence(), Evidence::Estimate);
    assert_eq!(law.membrane.q11, 180.0);
    assert_eq!(law.membrane.c1122, 180.0);
    assert_eq!(law.bending.unwrap().kappa0, [12.0, -18.0, 0.0]);
    assert_eq!(law.hysteresis.as_ref().unwrap().thresholds, [0.02, 0.08]);
    assert_eq!(law.fingerprint().len(), 64);
    let mechanics = selected.mechanics().unwrap();
    let nu21 = mechanics.poisson * mechanics.weft_n_m / mechanics.warp_n_m;
    let denominator = 1.0 - mechanics.poisson * nu21;
    assert!((mechanics.warp_n_m / denominator - law.membrane.q11).abs() < 1e-12);
    assert!((mechanics.weft_n_m / denominator - law.membrane.q22).abs() < 1e-12);
    assert!(
        (mechanics.poisson * mechanics.weft_n_m / denominator - law.membrane.q12).abs() < 1e-12
    );
    assert_eq!(mechanics.shear_n_m, law.membrane.q66);
    assert_eq!(mechanics.areal_kg_m2, 0.2);
    assert_eq!(mechanics.thickness_m, 0.0006);
    assert_eq!(mechanics.bending_nm, 0.00003);
    assert!(selected.material().missing_mechanics().is_empty());
    assert!(
        selected
            .membrane_response()
            .unwrap_err()
            .message
            .contains("coupled-knit consumer")
    );

    let frozen = selected.frozen_selection();
    let replay = catalog().resolve(&frozen).unwrap();
    assert_eq!(replay.sha256(), selected.sha256());
    assert_eq!(replay.coupled_knit().unwrap().unwrap(), law);
    let mut changed = frozen.clone();
    changed
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .membrane
        .parameters
        .q11 += 1.0;
    assert!(
        catalog()
            .resolve(&changed)
            .unwrap_err()
            .message
            .contains("expected_sha256")
    );
    changed.expected_sha256 = None;
    let changed = catalog().resolve(&changed).unwrap();
    assert_ne!(changed.sha256(), selected.sha256());
    assert_ne!(
        changed.coupled_knit().unwrap().unwrap().fingerprint(),
        law.fingerprint()
    );

    let mut changed_source = frozen;
    changed_source.expected_sha256 = None;
    changed_source
        .record
        .as_mut()
        .unwrap()
        .sources
        .get_mut("synthetic")
        .unwrap()
        .note
        .push_str(" Independent source revision.");
    let changed_source = catalog().resolve(&changed_source).unwrap();
    assert_ne!(changed_source.sha256(), selected.sha256());
    assert_ne!(
        changed_source
            .coupled_knit()
            .unwrap()
            .unwrap()
            .fingerprint(),
        law.fingerprint()
    );
}

#[test]
fn estimate_consent_is_required_for_each_selected_block_and_never_promotes_evidence() {
    let mut denied = selection();
    denied.allow_estimates = false;
    let denied = catalog().resolve(&denied).unwrap();
    assert!(
        denied
            .coupled_knit()
            .unwrap_err()
            .message
            .contains("allow_estimates")
    );
    assert!(denied.mechanics().is_err());

    // Synthetic evidence-policy exercise: the optional estimated block cannot hide behind
    // differently tagged equilibrium coefficients.
    let mut mixed = selection();
    let response = mixed
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap();
    response.membrane.evidence = Evidence::Published;
    response.bending.as_mut().unwrap().evidence = Evidence::Measured;
    let accepted = catalog().resolve(&mixed).unwrap();
    assert_eq!(
        accepted.coupled_knit().unwrap().unwrap().evidence(),
        Evidence::Estimate
    );
    mixed.allow_estimates = false;
    assert!(catalog().resolve(&mixed).unwrap().coupled_knit().is_err());
    let response = mixed
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap();
    response.hysteresis = None;
    assert_eq!(
        catalog()
            .resolve(&mixed)
            .unwrap()
            .coupled_knit()
            .unwrap()
            .unwrap()
            .evidence(),
        Evidence::Published
    );
    // Independent contact/mass/seam observations are still estimates and remain refused.
    assert!(catalog().resolve(&mixed).unwrap().mechanics().is_err());
}

#[test]
fn incomplete_or_conflicting_qualification_refuses_before_material_binding() {
    let raw: Value = serde_json::from_str(FIXTURE).unwrap();
    for block in ["membrane", "bending", "hysteresis"] {
        let mut missing = raw.clone();
        missing["record"]["knit_response"][block]["source"] = json!("absent-source");
        assert!(rejected(missing).contains("absent material source"));
        let mut changed = raw.clone();
        changed["record"]["knit_response"][block]["conditions"]["specimen"] = json!("other-coupon");
        assert!(rejected(changed).contains("specimen"));
        let mut changed = raw.clone();
        changed["record"]["knit_response"][block]["conditions"]["conditioning"] = json!("wet");
        assert!(rejected(changed).contains("conditioning"));
        let mut missing = raw.clone();
        missing["record"]["knit_response"][block]["conditions"]
            .as_object_mut()
            .unwrap()
            .remove("specimen");
        assert!(rejected(missing).contains("specimen"));
    }
    for key in [
        "areal_density",
        "thickness",
        "bending_rigidity",
        "friction_skin",
        "friction_self",
    ] {
        let mut changed = raw.clone();
        changed["record"]["properties"][key]["conditions"]["conditioning"] = json!("other-state");
        assert!(rejected(changed).contains(key));
    }
    let mut unknown = raw.clone();
    unknown["record"]["knit_response"]["model"] = json!("knit-reference-default");
    assert!(rejected(unknown).contains(MODEL));
    let mut unknown = raw.clone();
    unknown["record"]["knit_response"]["membrane"]["parameters"]["invented_modulus"] = json!(1.0);
    assert!(rejected(unknown).contains("unknown field"));
    let mut absent = raw;
    absent["record"]["knit_response"]["membrane"]["parameters"]
        .as_object_mut()
        .unwrap()
        .remove("q12");
    assert!(rejected(absent).contains("q12"));
}

#[test]
fn explicit_sheet_form_and_exclusive_model_selection_prevent_silent_substitution() {
    let raw: Value = serde_json::from_str(FIXTURE).unwrap();
    for form in ["woven", "yarn", "fill", "sheet"] {
        let mut changed = raw.clone();
        changed["record"]["form"] = json!(form);
        assert!(rejected(changed).contains("explicit knit sheet form"));
    }
    for selector in [
        "membrane_model",
        "membrane_plasticity_model",
        "membrane_damage_model",
        "membrane_linearization_model",
        "bending_plasticity_model",
    ] {
        let mut changed = raw.clone();
        changed["record"]["structure"][selector] = json!("some-other-law");
        assert!(rejected(changed).contains("cannot coexist"));
    }
    for property in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        let mut changed = raw.clone();
        let mut observation = changed["record"]["properties"]["friction_skin"].clone();
        observation["value"] = json!(1.0);
        observation["unit"] = json!(if property == "poisson_warp_weft" {
            "1"
        } else {
            "N/m"
        });
        changed["record"]["properties"][property] = observation;
        assert!(rejected(changed).contains("another membrane law"));
    }
    let mut changed = raw;
    changed["record"]["structure"]["bending_model"] = json!("orthotropic_normal_patch/1");
    assert!(rejected(changed).contains("second panel bending"));
}

#[test]
fn missing_mass_contact_or_seam_measurements_stay_missing_after_knit_admission() {
    for key in [
        "areal_density",
        "thickness",
        "bending_rigidity",
        "friction_skin",
    ] {
        let mut selected = selection();
        selected.record.as_mut().unwrap().properties.remove(key);
        let selected = catalog().resolve(&selected).unwrap();
        assert!(selected.coupled_knit().unwrap().is_some());
        assert!(selected.material().missing_mechanics().contains(&key));
        assert!(selected.mechanics().unwrap_err().message.contains(key));
    }
    let mut selected = selection();
    selected
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("friction_self");
    let selected = catalog().resolve(&selected).unwrap();
    assert!(selected.mechanics_for_contacts(true).is_err());
    assert_eq!(
        selected
            .mechanics_for_contacts(false)
            .unwrap()
            .friction_self,
        None
    );
}

#[test]
fn impossible_or_oversized_parameter_data_cannot_enter_the_shared_material_snapshot() {
    let mut unstable = selection();
    unstable
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .membrane
        .parameters
        .q12 = 1000.0;
    assert!(catalog().resolve(&unstable).is_err());
    let mut nonfinite = selection();
    nonfinite
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .membrane
        .parameters
        .q11 = f64::NAN;
    assert!(catalog().resolve(&nonfinite).is_err());
    let mut unstable = selection();
    unstable
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .bending
        .as_mut()
        .unwrap()
        .parameters
        .d12 = 1.0;
    assert!(catalog().resolve(&unstable).is_err());
    let mut oversized = selection();
    let bank = &mut oversized
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .hysteresis
        .as_mut()
        .unwrap()
        .parameters;
    bank.thresholds = vec![0.02; MAX_HYSTERESIS_MODES + 1];
    bank.weights = vec![1.0; MAX_HYSTERESIS_MODES + 1];
    assert!(catalog().resolve(&oversized).is_err());
    let mut empty = selection();
    let bank = &mut empty
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .hysteresis
        .as_mut()
        .unwrap()
        .parameters;
    bank.thresholds.clear();
    bank.weights.clear();
    assert!(catalog().resolve(&empty).is_err());
}

#[test]
fn ordinary_tensor_bending_remains_available_when_no_asymmetric_knit_block_is_selected() {
    let mut selected = selection();
    let material = selected.record.as_mut().unwrap();
    material.knit_response.as_mut().unwrap().bending = None;
    material
        .structure
        .insert("bending_model".into(), "orthotropic_normal_patch/1".into());
    for (key, value) in [
        ("bending_warp", 0.0002),
        ("bending_weft", 0.00005),
        ("bending_coupling", 0.00001),
        ("bending_twist", 0.000025),
    ] {
        let mut quantity = material.properties["bending_rigidity"].clone();
        quantity.value = value;
        material.properties.insert(key.into(), quantity);
    }
    let selected = catalog().resolve(&selected).unwrap();
    assert!(selected.coupled_knit().unwrap().unwrap().bending.is_none());
    assert!(selected.bending_response().unwrap().unwrap().uses_normal());
    assert!(selected.mechanics().is_ok());
}

#[test]
fn existing_knit_catalog_records_do_not_acquire_a_reference_law_or_new_serialized_field() {
    let catalog = Catalog::builtin().unwrap();
    let measured = catalog.get("sperl2022/SJ1").unwrap();
    assert_eq!(measured.form, Form::Knit);
    assert!(measured.knit_response.is_none());
    assert!(measured.compiled_knit(true).unwrap().is_none());
    assert!(measured.mechanics(true).is_err());
    for record in &catalog.materials {
        assert!(record.knit_response.is_none());
        assert!(
            serde_json::to_value(record)
                .unwrap()
                .get("knit_response")
                .is_none()
        );
    }
    let old: Material = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-nonlinear.json"
    ))
    .unwrap();
    assert!(old.knit_response.is_none());
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("knit_response")
            .is_none()
    );
    assert!(old.compiled_membrane(true).unwrap().is_some());
}

#[test]
fn mechanical_screening_admits_explicit_coupled_inputs_without_weakening_estimate_policy() {
    let mut request: ftx_pattern::material_screen::Request = serde_json::from_value(json!({
        "schema": "ftx-material-screen/1",
        "basis": "nominal",
        "candidates": [{"key": "knit", "selection": selection()}],
        "required_capabilities": ["mechanics"]
    }))
    .unwrap();
    let accepted = request.evaluate(&catalog()).unwrap();
    assert_eq!(accepted.frontier, ["knit"]);
    assert!(accepted.alternatives[0].eligible);
    assert!(!accepted.physical_validation_claim);
    request.candidates[0].selection.allow_estimates = false;
    let denied = request.evaluate(&catalog()).unwrap();
    assert!(denied.frontier.is_empty());
    assert!(!denied.alternatives[0].eligible);
    assert!(!denied.alternatives[0].exclusions.is_empty());
}

#[test]
fn wet_knit_response_requires_each_block_at_the_exact_retained_liquid_loading() {
    let mut selected = selection();
    let material = selected.record.as_mut().unwrap();
    material.structure.insert(
        "mechanical_mass_model".into(),
        ftx_pattern::material_mass::RETAINED_LIQUID_MODEL.into(),
    );
    let wet = |conditions: &mut std::collections::BTreeMap<String, String>| {
        conditions.insert("conditioning".into(), "synthetic fixed wet state".into());
        conditions.insert("specimen_id".into(), "synthetic-knit-coupon".into());
        conditions.insert("retained_liquid_kg_m2".into(), "0.05".into());
    };
    for observation in material.properties.values_mut() {
        wet(&mut observation.conditions);
    }
    let response = material.knit_response.as_mut().unwrap();
    wet(&mut response.membrane.conditions);
    wet(&mut response.bending.as_mut().unwrap().conditions);
    wet(&mut response.hysteresis.as_mut().unwrap().conditions);
    let dry = material.properties.get_mut("areal_density").unwrap();
    dry.conditions.insert(
        "mass_basis".into(),
        "dry_textile_excluding_retained_liquid".into(),
    );
    dry.conditions.insert(
        "reference_area_basis".into(),
        "dry_textile_reference_area".into(),
    );
    let mut liquid = dry.clone();
    liquid.value = 0.05;
    liquid
        .conditions
        .insert("mass_basis".into(), "additional_retained_liquid".into());
    liquid.conditions.insert(
        "retention_basis".into(),
        "closed_co_moving_no_exchange".into(),
    );
    liquid.conditions.insert(
        "qualification".into(),
        "Synthetic closed co-moving numerical fixture only.".into(),
    );
    material
        .properties
        .insert("retained_liquid_areal_density".into(), liquid);
    let admitted = catalog().resolve(&selected).unwrap();
    assert!((admitted.mechanics().unwrap().areal_kg_m2 - 0.25).abs() < 1e-15);
    let raw = serde_json::to_value(&selected).unwrap();
    for block in ["membrane", "bending", "hysteresis"] {
        for key in ["retained_liquid_kg_m2", "specimen_id"] {
            let mut missing = raw.clone();
            missing["record"]["knit_response"][block]["conditions"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            let selection: Selection = serde_json::from_value(missing).unwrap();
            let admitted = catalog().resolve(&selection).unwrap();
            assert!(admitted.coupled_knit().unwrap().is_some());
            assert!(
                admitted
                    .mechanics()
                    .unwrap_err()
                    .message
                    .contains("wet mechanics")
            );
        }
        let mut changed = raw.clone();
        changed["record"]["knit_response"][block]["conditions"]["retained_liquid_kg_m2"] =
            json!("0.06");
        let selection: Selection = serde_json::from_value(changed).unwrap();
        assert!(catalog().resolve(&selection).unwrap().mechanics().is_err());
    }
}

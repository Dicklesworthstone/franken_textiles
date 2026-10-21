//! Primary-source transcription checks, not new empirical validation of M1 or another garment.
use ftx_pattern::{
    material_catalog::Selection,
    material_screen::{Basis, Capability, Request},
    materials::{Catalog, Evidence},
};
use serde_json::json;

const ID: &str = "zagreb/polym17152013/M1";

fn resolved() -> ftx_pattern::material_catalog::ResolvedMaterial {
    Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: ID.into(),
            ..Selection::default()
        })
        .unwrap()
}

#[test]
fn one_matched_specimen_retains_construction_units_conditions_and_attribution() {
    let m = resolved();
    let record = m.material();
    assert_eq!(record.structure["weave"], "twill");
    assert!(record.composition_complete);
    assert_eq!(
        record.composition.iter().map(|c| c.fraction).sum::<f64>(),
        1.
    );
    assert!((record.properties["areal_density"].value - 0.22658).abs() < 1e-14);
    assert!((record.properties["thickness"].value - 0.00052).abs() < 1e-14);
    assert_eq!(record.properties["thread_density_warp"].value, 4000.);
    assert_eq!(record.properties["thread_density_weft"].value, 2000.);
    assert_eq!(
        record.properties["thickness"].conditions["pressure_kpa"],
        "1"
    );
    assert_eq!(record.properties["thickness"].method, "HRN ISO 5084:2003");
    assert_eq!(record.properties["kes_emt_warp"].value, 1.730);
    assert_eq!(record.properties["kes_emt_weft"].value, 11.180);
    assert_eq!(record.properties["kes_bending_warp_reported"].unit, "cNcm");
    assert_eq!(
        record.sources["paper"].reference,
        "https://doi.org/10.3390/polym17152013"
    );
    assert!(
        record
            .properties
            .values()
            .all(|p| p.evidence == Evidence::Measured && p.bounds.is_none())
    );
    let frozen = m.frozen_selection();
    let authored = frozen.record.as_ref().unwrap();
    assert_eq!(authored.properties["areal_density"].unit, "g/m2");
    assert_eq!(authored.properties["thickness"].unit, "mm");
    let empty = Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
    assert_eq!(empty.resolve(&frozen).unwrap().sha256(), m.sha256());
}

#[test]
fn unresolved_kes_and_cad_conversions_never_become_a_fabric_law() {
    let m = resolved();
    for missing in [
        "warp_stiffness",
        "weft_stiffness",
        "shear_stiffness",
        "bending_rigidity",
        "bending_warp",
        "elongation_break_warp",
        "width",
        "thermal_resistance",
        "specific_heat",
    ] {
        assert!(!m.material().properties.contains_key(missing), "{missing}");
    }
    assert!(m.mechanics().is_err());
    assert!(m.thermal_resistance().is_err());
}

#[test]
fn measurements_reach_screening_without_inventing_bounds_or_compiler_readiness() {
    let mut request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1","basis":"nominal",
        "candidates":[{"key":"measured-twill","selection":{"id":ID}}],
        "constraints":[{"attribute":{"key":"thickness","unit":"m","conditions":{"pressure_kpa":"1"}},"maximum":0.0006}],
        "objectives":[{"attribute":{"key":"areal_density","unit":"kg/m2"},"direction":"minimize"}]
    })).unwrap();
    let db = Catalog::builtin().unwrap();
    assert_eq!(request.evaluate(&db).unwrap().frontier, ["measured-twill"]);
    request.basis = Basis::ReportedBounds;
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
    request.basis = Basis::Nominal;
    request.required_capabilities = vec![Capability::Mechanics];
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
    request.required_capabilities.clear();
    request.constraints[0]
        .attribute
        .conditions
        .insert("pressure_kpa".into(), "0".into());
    assert!(request.evaluate(&db).unwrap().frontier.is_empty());
}

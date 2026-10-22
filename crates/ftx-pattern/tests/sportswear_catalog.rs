//! Primary-table transcription and real consumer checks; no empirical garment claim.

use ftx_pattern::{
    material_catalog::{Selection, normalize},
    material_screen,
    material_thermal::ThermalLaw,
    material_vapor,
    materials::{Catalog, Evidence, Form, ThermalStack},
};
use serde_json::json;

fn selection(suffix: &str) -> Selection {
    Selection {
        id: format!("zagreb/sportswear-2022/{suffix}"),
        ..Selection::default()
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1e-12),
        "{actual} != {expected}"
    );
}

#[test]
fn measured_yarns_stay_separate_from_the_twenty_five_knit_constructions() {
    let catalog = Catalog::builtin().unwrap();
    let rows: Vec<_> = catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("zagreb/sportswear-2022/"))
        .collect();
    assert_eq!(rows.len(), 30);
    assert_eq!(rows.iter().filter(|m| m.form == Form::Yarn).count(), 5);
    assert_eq!(rows.iter().filter(|m| m.form == Form::Knit).count(), 25);
    for m in rows {
        m.validate().unwrap();
        assert!(
            m.properties
                .values()
                .all(|q| q.evidence == Evidence::Measured && q.bounds.is_none())
        );
        assert!(m.mechanics(true).is_err());
        assert!(!m.properties.contains_key("thickness"));
        assert!(!m.properties.contains_key("specific_heat"));
        assert!(!m.properties.contains_key("thermal_conductivity"));
        if m.form == Form::Knit {
            assert_eq!(
                catalog.get(&m.structure["yarn_record"]).unwrap().form,
                Form::Yarn
            );
        }
    }
    let cotton = catalog.resolve(&selection("y1-yarn")).unwrap();
    close(
        cotton.material().properties["yarn_linear_density"].value,
        29.1e-6,
    );
    close(cotton.material().properties["yarn_tenacity"].value, 139000.);
    close(
        cotton.material().properties["yarn_elongation_break"].value,
        0.052,
    );
    close(cotton.material().properties["yarn_mass_cv"].value, 0.1175);
    assert_eq!(cotton.material().properties["yarn_tenacity"].unit, "N*m/kg");
    assert_eq!(
        cotton.authored().properties["yarn_linear_density"].unit,
        "tex"
    );
    assert_eq!(
        cotton.authored().structure["threads_used_for_knitting"],
        "2"
    );
    let filament = catalog.resolve(&selection("y5-yarn")).unwrap();
    close(
        filament.material().properties["yarn_elongation_break"].value,
        3.0127,
    );
    assert!(!filament.material().properties.contains_key("twist"));
    assert!(!filament.material().properties.contains_key("yarn_mass_cv"));
    assert!(
        !filament
            .material()
            .properties
            .keys()
            .any(|key| key.starts_with("yarn_hair_count"))
    );
}

#[test]
fn construction_changes_real_mass_dry_heat_and_vapor_consumers() {
    let catalog = Catalog::builtin().unwrap();
    // Independently transcribed source tables 4, 5 and 7, selected contrasting structures.
    for (id, mass, rct, ret) in [
        ("y1-sp", 0.26038, 0.0444, 3.7448),
        ("y2-st", 0.28640, 0.0834, 3.5071),
        ("y4-dp", 0.47154, 0.0812, 6.0944),
        ("y5-st", 0.13251, 0.0172, 0.5570),
    ] {
        let resolved = catalog.resolve(&selection(id)).unwrap();
        close(
            resolved
                .material()
                .require("areal_density", "kg/m2", false)
                .unwrap(),
            mass,
        );
        let dry =
            ThermalStack::evaluate(&[resolved.material()], 0.5, 35., 20., 0.1, false).unwrap();
        close(dry.resistance_m2_k_w, rct);
        close(dry.heat_flow_w, 7.5 / (rct + 0.1));
        assert!(ThermalLaw::from_material(resolved.material(), false).is_err());

        // Boundary films, vapor pressures and latent heat are explicit synthetic scenario inputs.
        let input: material_vapor::Request = serde_json::from_value(json!({
            "schema": "ftx-vapor-stack/1", "layers": [{"key":"test", "selection": selection(id)}],
            "area_m2": 0.5, "inside_vapor_pressure_pa": 2000., "outside_vapor_pressure_pa": 1000.,
            "inside_film_resistance_m2_pa_w": 1., "outside_film_resistance_m2_pa_w": 1.,
            "latent_heat_j_kg": 2_500_000.,
            "required_conditions": {"air_temperature_c":"35", "relative_humidity_percent":"40", "air_speed_m_s":"1"}
        })).unwrap();
        let report = input.evaluate(&catalog).unwrap();
        close(report.potential_heat_flux_w_m2, 1000. / (ret + 2.));
        close(
            report.potential_mass_flow_kg_s,
            500. / (ret + 2.) / 2_500_000.,
        );
        assert!(!report.physical_validation_claim);
        let replay: material_vapor::Request =
            serde_json::from_str(&report.replay_request_json).unwrap();
        let empty =
            Catalog::from_json(r#"{"schema":"ftx-textile-catalog/1","materials":[]}"#).unwrap();
        close(
            replay.evaluate(&empty).unwrap().potential_heat_flux_w_m2,
            report.potential_heat_flux_w_m2,
        );
        let mut wrong_conditions = input;
        wrong_conditions
            .required_conditions
            .insert("air_temperature_c".into(), "20".into());
        assert!(wrong_conditions.evaluate(&catalog).is_err());
    }
}

#[test]
fn gauge_and_volume_flow_keep_their_test_basis_and_original_source_discrepancies() {
    let catalog = Catalog::builtin().unwrap();
    let cotton = catalog.resolve(&selection("y1-sp")).unwrap();
    let pa = catalog.resolve(&selection("y5-sp")).unwrap();
    close(
        cotton.material().properties["reported_thickness"].value,
        0.00123,
    );
    close(
        cotton.material().properties["knit_wale_density"].value,
        798.,
    );
    close(
        cotton.material().properties["knit_course_density"].value,
        832.,
    );
    assert_eq!(
        cotton.material().properties["reported_thickness"].conditions["gauge_pressure_reported"],
        "20 g/cm2"
    );
    close(
        cotton.material().properties["air_volume_flow"].value,
        140.12e-3 / 60.,
    );
    close(
        pa.material().properties["air_volume_flow"].value,
        145.88e-3 / 60.,
    );
    assert_eq!(
        cotton.authored().properties["air_volume_flow"].unit,
        "L/min"
    );
    assert_eq!(
        cotton.material().properties["air_volume_flow"].conditions["test_area_cm2"],
        "20"
    );
    assert_eq!(
        pa.material().properties["air_volume_flow"].conditions["test_area_cm2"],
        "5"
    );
    assert!(
        !cotton
            .material()
            .properties
            .contains_key("air_permeability")
    );
    assert!(!pa.material().properties.contains_key("air_permeability"));
    let conflict = catalog.get(&selection("y4-dp").id).unwrap();
    assert_eq!(conflict.properties["vapor_resistance"].value, 6.0944);
    assert!(conflict.structure["source_discrepancy"].contains("5.7542"));

    let mut invalid = cotton.authored().clone();
    invalid.properties.get_mut("air_volume_flow").unwrap().unit = "m/s".into();
    assert!(normalize(&invalid).is_err());
    let mut invalid = catalog.get(&selection("y1-yarn").id).unwrap().clone();
    invalid.properties.get_mut("yarn_tenacity").unwrap().unit = "N/m".into();
    assert!(normalize(&invalid).is_err());
}

#[test]
fn existing_screening_can_require_the_actual_air_test_area() {
    let catalog = Catalog::builtin().unwrap();
    let input: material_screen::Request = serde_json::from_value(json!({
        "schema":"ftx-material-screen/1", "basis":"nominal", "query":"zagreb/sportswear-2022/",
        "forms":["knit"], "constraints":[{
            "attribute":{"key":"air_volume_flow", "unit":"m3/s", "conditions":{"pressure_drop_pa":"50", "test_area_cm2":"20"}},
            "minimum":0., "maximum":null
        }]
    })).unwrap();
    let report = input.evaluate(&catalog).unwrap();
    assert_eq!(
        report.alternatives.iter().filter(|a| a.eligible).count(),
        20
    );
    assert!(
        report
            .alternatives
            .iter()
            .filter(|a| a.material_id.contains("/y5-"))
            .all(|a| !a.eligible)
    );
}

#[test]
fn runnable_source_comparison_preserves_the_mass_transport_tradeoff() {
    let input: material_screen::Request = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/screen-sportswear.json"
    ))
    .unwrap();
    let report = input.evaluate(&Catalog::builtin().unwrap()).unwrap();
    assert_eq!(report.alternatives.len(), 2);
    assert!(report.alternatives.iter().all(|a| a.eligible));
    assert_eq!(report.frontier, ["wool-single-plain", "wool-single-tuck"]);
    let plain = &report.alternatives[0].readings;
    let tuck = &report.alternatives[1].readings;
    assert!(plain["areal_density"].value < tuck["areal_density"].value);
    assert!(plain["thermal_resistance"].value < tuck["thermal_resistance"].value);
    assert!(plain["vapor_resistance"].value > tuck["vapor_resistance"].value);
    assert!(!report.physical_validation_claim);
}

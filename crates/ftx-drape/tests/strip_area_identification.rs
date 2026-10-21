//! Six-coordinate recovery from explicitly synthetic production-membrane observations.
//! These numerical acquisitions establish no measured textile or empirical validation claim.

use std::sync::OnceLock;

use ftx_drape::strip::{
    Request, evaluate,
    fit::{Input, Report, area::Area, calibrate},
};
use ftx_pattern::{
    material_calibration::{Partition, fitted_domain},
    textile_import,
};

const PARAMETERS: [f64; 6] = [1.1, 0.9, 1.2, -80.0, 31.0, 19.0];

fn input() -> Input {
    serde_json::from_str(include_str!(
        "../../../examples/area-response/identify.json"
    ))
    .unwrap()
}

fn baseline() -> &'static Report {
    static REPORT: OnceLock<Report> = OnceLock::new();
    REPORT.get_or_init(|| calibrate(&input()).unwrap())
}

#[test]
fn identifies_six_coefficients_and_replays_the_source_qualified_area_law() {
    let report = baseline();
    assert!(
        report.accepted,
        "{:?}; parameters {:?}; pivot {}; starts {:?}; mesh {}",
        report.reasons,
        report.parameters,
        report.excitation_pivot,
        report.starts,
        report.mesh_change_peak_scaled,
    );
    assert!(report.domain_replay_verified);
    for (actual, known) in report.parameters.iter().zip(PARAMETERS) {
        assert!((actual - known).abs() < 2e-3, "{actual} != {known}");
    }
    let descriptors = report.parameter_descriptors.as_ref().unwrap();
    assert_eq!(descriptors.len(), 6);
    assert_eq!(descriptors[4].id, "area_quadratic");
    assert_eq!(descriptors[5].unit, "N/m");
    assert_eq!(report.parameter_bounds.len(), 6);
    let material = report.material.as_ref().unwrap();
    let compiled = material.compiled_membrane(true).unwrap().unwrap();
    assert_eq!(compiled.area_ratio_range(), report.fitted_area_ratio_range);
    assert_eq!(
        fitted_domain(material).unwrap().unwrap().ranges(),
        report.fitted_green_strain_range
    );
    let response = material.membrane_response.as_ref().unwrap();
    let area = response.area_response.as_ref().unwrap();
    assert!(area.area_ratio_range[0] <= 1.0 && area.area_ratio_range[1] > 1.1);
    let domain_json = serde_json::to_string(&area.area_ratio_range).unwrap();
    for conditions in [
        &response.warp.conditions,
        &response.weft.conditions,
        &response.shear.conditions,
        &response.coupling.conditions,
        &area.quadratic.conditions,
        &area.log_squared.conditions,
        &area.conditions,
    ] {
        assert_eq!(conditions["fitted_area_ratio_range"], domain_json);
    }
    let selected = report.selection.as_ref().unwrap();
    let resolved = textile_import::catalog()
        .unwrap()
        .resolve(selected)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&resolved.frozen_selection()).unwrap(),
        report.selection_json.as_deref().unwrap()
    );
    for experiment in &report.input.experiments {
        let mut fixture = experiment.fixture.clone();
        fixture.cells.iter_mut().for_each(|n| *n *= 2);
        let replay = evaluate(&Request {
            schema: "ftx-strip-experiment/1".into(),
            material: selected.clone(),
            fixture,
            extensions: experiment
                .observations
                .iter()
                .map(|o| o.extension_fraction)
                .collect(),
            controls: report.input.controls.clone(),
        })
        .unwrap();
        for (actual, published) in replay.states.iter().zip(
            report
                .predictions
                .iter()
                .filter(|p| p.experiment == experiment.id),
        ) {
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&published.state).unwrap()
            );
        }
    }
}

#[test]
fn fit_only_physical_area_support_survives_check_changes_and_cannot_be_erased() {
    let baseline = baseline();
    assert!(baseline.accepted, "{:?}", baseline.reasons);
    let fitted = baseline.fitted_area_ratio_range.unwrap();
    let declared = baseline
        .input
        .material
        .membrane_response
        .as_ref()
        .unwrap()
        .area_response
        .as_ref()
        .unwrap()
        .area_ratio_range;
    assert!(fitted[0] > declared[0] && fitted[1] < declared[1]);
    let mut observed = [1.0_f64; 2];
    for experiment in baseline
        .input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for factor in [1, 2] {
            let mut fixture = experiment.fixture.clone();
            fixture.cells.iter_mut().for_each(|n| *n *= factor);
            let mut extensions = vec![0.0];
            extensions.extend(experiment.observations.iter().map(|o| o.extension_fraction));
            let replay = evaluate(&Request {
                schema: "ftx-strip-experiment/1".into(),
                material: baseline.selection.clone().unwrap(),
                fixture,
                extensions,
                controls: baseline.input.controls.clone(),
            })
            .unwrap();
            for state in replay.states {
                let actual = state.area_ratio_range.unwrap();
                observed[0] = observed[0].min(actual[0]);
                observed[1] = observed[1].max(actual[1]);
            }
        }
    }
    for (actual, fitted) in observed.into_iter().zip(fitted) {
        assert!((actual - fitted).abs() < 1e-12, "{actual} != {fitted}");
    }
    let mut changed = baseline.input.clone();
    changed.experiments.last_mut().unwrap().observations[0].force_n += 10.0;
    let rejected = calibrate(&changed).unwrap();
    assert!(!rejected.accepted);
    assert!(rejected.material.is_none());
    assert_eq!(rejected.parameters, baseline.parameters);
    assert_eq!(
        rejected.fitted_area_ratio_range,
        baseline.fitted_area_ratio_range
    );

    let material = baseline.material.as_ref().unwrap();
    let mut removed = material.clone();
    let response = removed.membrane_response.as_mut().unwrap();
    response.area_response = None;
    for conditions in [
        &mut response.warp.conditions,
        &mut response.weft.conditions,
        &mut response.shear.conditions,
        &mut response.coupling.conditions,
    ] {
        conditions.remove("fitted_area_ratio_range");
    }
    assert!(
        removed.validate().is_err(),
        "jointly identified area law was silently discarded"
    );
    let mut widened = material.clone();
    widened
        .membrane_response
        .as_mut()
        .unwrap()
        .area_response
        .as_mut()
        .unwrap()
        .area_ratio_range = declared;
    assert!(
        widened.validate().is_err(),
        "identified area domain was silently widened"
    );

    let mut narrow = baseline.input.clone();
    narrow
        .material
        .membrane_response
        .as_mut()
        .unwrap()
        .area_response
        .as_mut()
        .unwrap()
        .area_ratio_range = [0.99, 1.01];
    assert!(
        calibrate(&narrow).is_err(),
        "search widened the original source J support"
    );
}

#[test]
fn area_identification_requires_the_declared_family_and_qualified_conditions() {
    let mut candidate = input();
    candidate.response_family = None;
    assert!(
        calibrate(&candidate)
            .unwrap_err()
            .message
            .contains("directional response family")
    );
    candidate = input();
    candidate.conditions.remove("specimen");
    assert!(
        calibrate(&candidate)
            .unwrap_err()
            .message
            .contains("specimen/conditioning")
    );
    candidate = input();
    candidate.area_response = Some(Area::Identify {
        parameter_bounds: [[-1.0, 50.0], [5.0, 35.0]],
    });
    assert!(
        calibrate(&candidate)
            .unwrap_err()
            .message
            .contains("nonnegative")
    );
}

//! Coupon-to-selection adapter for directional panel bending.
use ftx_core::Refusal;
use ftx_pattern::material_bending_calibration::{Input, calibrate};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

use super::{read, simulation_readiness};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let input: Input = read(path)?;
    let report = calibrate(&input)?;
    let material = catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let readiness = simulation_readiness(&material);
    Ok(json!({
        "calibration": report,
        "selection": material.frozen_selection(),
        "simulation_ready": readiness.is_ok(),
        "mechanics_refusal": readiness.err(),
        "fit_claim": false,
        "empirical_validation_claim": false,
        "measurement_note": "Moment resultants and curvature must come from the declared coupon geometry and acquisition method; the command never derives bending from thickness or membrane modulus."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupon_fit_produces_the_frozen_tensor_used_by_the_solver() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/materials/synthetic-bending-coupons.json"
        );
        let denied = run(path, false).unwrap();
        assert_eq!(denied["simulation_ready"], false);
        assert_eq!(denied["selection"]["allow_estimates"], false);

        let admitted = run(path, true).unwrap();
        assert_eq!(admitted["simulation_ready"], true);
        let selection: Selection =
            serde_json::from_value(admitted["selection"].clone()).unwrap();
        let resolved = catalog().unwrap().resolve(&selection).unwrap();
        let bending = resolved.bending_response().unwrap().unwrap();
        let d = bending.matrix();
        assert!((d[0][0] - 4.0e-4).abs() < 1e-12);
        assert!((d[1][1] - 1.5e-4).abs() < 1e-12);
        assert!((d[0][1] - 5.0e-5).abs() < 1e-12);
        assert!((d[2][2] - 8.0e-5).abs() < 1e-12);
        assert_eq!(
            Some(resolved.sha256()),
            selection.expected_sha256.as_deref()
        );
    }
}

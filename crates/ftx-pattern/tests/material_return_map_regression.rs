//! Artificial coefficients isolate floating-point representability in both material histories.
//! These regressions make no measured textile or cyclic calibration claim.

use ftx_pattern::{
    material_bending::BendingTensor,
    material_membrane_plasticity::{Memory as MembraneMemory, PlasticMembrane},
    material_plasticity::{Memory as BendingMemory, PlasticBending},
};

fn pair(
    stiffness: [f64; 3],
    yield_value: [f64; 3],
    hardening: [f64; 3],
) -> (MembraneMemory, BendingMemory) {
    let membrane = PlasticMembrane::new(
        [
            [stiffness[0], 0.0, 0.0],
            [0.0, stiffness[1], 0.0],
            [0.0, 0.0, stiffness[2]],
        ],
        yield_value,
        hardening,
        [0.3; 3],
        [1.0; 3],
    )
    .unwrap()
    .pristine();
    let bending = PlasticBending::new(
        BendingTensor::new(stiffness[0], stiffness[1], 0.0, stiffness[2])
            .unwrap()
            .normal_projected(),
        yield_value,
        hardening,
        [1.0; 3],
        [1.0; 3],
    )
    .unwrap()
    .pristine();
    (membrane, bending)
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-14 * (1.0 + a.abs() + b.abs()));
}

#[test]
fn lost_post_return_yield_stress_refuses_both_material_histories() {
    let (membrane, bending) = pair([1e20, 1.0, 1.0], [1.0; 3], [1.0; 3]);
    let previous_membrane = membrane.clone();
    let previous_bending = bending.clone();
    let coordinate = [0.125, 0.0, 0.0];
    // The coordinate and elastic response are valid. But the return rounds the permanent
    // coordinate to 0.125, leaving stress zero instead of the nonzero hardened yield 1.125.
    assert!(membrane.response(coordinate, false).is_ok());
    assert!(bending.response(coordinate, false).is_ok());
    assert!(membrane.response(coordinate, true).is_err());
    assert!(bending.response(coordinate, true).is_err());
    assert!(membrane.advance(coordinate).is_err());
    assert!(bending.advance(coordinate).is_err());
    assert_eq!(membrane, previous_membrane);
    assert_eq!(bending, previous_bending);
}

#[test]
fn lost_positive_hardening_tangent_refuses_both_material_histories() {
    for (stiffness, coordinate) in [(1e16, 1e-10), (1e12, 1e-6)] {
        let (membrane, bending) = pair([stiffness, 1.0, 1.0], [1.0; 3], [1.0; 3]);
        let previous_membrane = membrane.clone();
        let previous_bending = bending.clone();
        let coordinate = [coordinate, 0.0, 0.0];
        // Residual stress still resolves here. The exact active tangent Q*H/(Q+H) is near 1,
        // but the unguarded subtraction returns 0 or 1.000122... and must not be accepted.
        assert!(membrane.response(coordinate, false).is_ok());
        assert!(bending.response(coordinate, false).is_ok());
        assert!(
            membrane.response(coordinate, true).is_err(),
            "Q={stiffness}"
        );
        assert!(bending.response(coordinate, true).is_err(), "Q={stiffness}");
        assert!(membrane.advance(coordinate).is_err());
        assert!(bending.advance(coordinate).is_err());
        assert_eq!(membrane, previous_membrane);
        assert_eq!(bending, previous_bending);
    }
}

#[test]
fn irrelevant_singular_candidates_do_not_hide_a_valid_shear_return() {
    let (membrane, bending) = pair([1e-20, 1.0, 1.0], [1.0, 1.0, 0.1], [0.0; 3]);
    let coordinate = [0.0, 0.0, 0.2];
    // An earlier normal active block fails its numerical pivot test. The later shear-only
    // return remains well resolved: delta0=delta1=0, delta2=0.1, stress2=0.1, tangent22=0.
    let membrane_response = membrane.response(coordinate, true).unwrap();
    let bending_response = bending.response(coordinate, true).unwrap();
    let membrane_update = membrane.advance(coordinate).unwrap();
    let bending_update = bending.advance(coordinate).unwrap();
    assert_eq!(membrane_update.memory.plastic_strain()[..2], [0.0; 2]);
    assert_eq!(bending_update.memory.plastic_curvature()[..2], [0.0; 2]);
    near(membrane_update.memory.plastic_strain()[2], 0.1);
    near(bending_update.memory.plastic_curvature()[2], 0.1);
    near(membrane_response.stress[2], 0.1);
    near(bending_response.moment_n[2], 0.1);
    assert_eq!(membrane_response.tangent[2][2], 0.0);
    assert_eq!(bending_response.tangent_nm[2][2], 0.0);
    near(membrane_update.dissipated_j_m2, 0.01);
    near(bending_update.dissipated_j_m2, 0.01);
}

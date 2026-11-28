//! Synthetic finite-grip regressions for the ordinary coupled-knit material consumer.
//! These establish constitutive/source wiring and numerical mechanics, not measured-knit adequacy.
use ftx_core::{RefusalCode, Vec2, Vec3};
use ftx_drape::{MembraneElement, strip};
use ftx_pattern::{
    material_catalog::Selection,
    material_knit::KnitMembraneParameters,
    materials::{CATALOG_SCHEMA, Catalog},
};

fn selection(history: bool) -> Selection {
    let mut selected: Selection = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-coupled-knit.json"
    ))
    .unwrap();
    if !history {
        selected
            .record
            .as_mut()
            .unwrap()
            .knit_response
            .as_mut()
            .unwrap()
            .hysteresis = None;
    }
    selected
}

fn membrane(selected: &Selection) -> KnitMembraneParameters {
    selected
        .record
        .as_ref()
        .unwrap()
        .knit_response
        .as_ref()
        .unwrap()
        .membrane
        .parameters
}

fn fixture() -> strip::Fixture {
    strip::Fixture {
        gauge_length_m: 0.1,
        width_m: 0.06,
        warp_axis: [0.0, 1.0],
        cells: [2, 4],
    }
}

fn request() -> strip::Request {
    strip::Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection(false),
        fixture: fixture(),
        extensions: vec![0.0, 0.1, 0.2, 0.1, 0.0],
        controls: strip::Controls::default(),
    }
}

fn history() -> strip::history::Request {
    strip::history::Request {
        schema: "ftx-strip-history/1".into(),
        material: selection(true),
        fixture: fixture(),
        loading: [
            (0.0, 0.0),
            (0.05, 0.12),
            (0.1, 0.12),
            (0.15, 0.0),
            (0.2, 0.0),
        ]
        .into_iter()
        .map(|(time_s, extension_fraction)| strip::history::Loading {
            time_s,
            extension_fraction,
        })
        .collect(),
        controls: strip::history::Controls::default(),
    }
}

fn geometry(fixture: &strip::Fixture, angle: f64) -> strip::Geometry {
    strip::Geometry {
        gauge_m: fixture.gauge_length_m,
        width_m: fixture.width_m,
        warp_angle_deg: angle,
        length_cells: fixture.cells[1],
        width_cells: fixture.cells[0],
    }
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected}"
    );
}

// Independent polynomial + ellipsoidal-barrier answer, with no solver/kernel evaluation.
fn energy_density(m: KnitMembraneParameters, [e1, e2, shear]: [f64; 3]) -> f64 {
    let r2 = (e1 / m.e1_jam).powi(2) + (e2 / m.e2_jam).powi(2) + (shear / m.gamma_jam).powi(2);
    0.5 * (m.q11 * e1 * e1 + 2.0 * m.q12 * e1 * e2 + m.q22 * e2 * e2 + m.q66 * shear * shear)
        + 0.25 * (m.c1111 * e1.powi(4) + m.c2222 * e2.powi(4))
        + 0.5 * m.c1122 * e1 * e1 * e2 * e2
        + m.kappa_jam * (-(1.0 - r2).ln() - r2)
}

fn uniaxial_stress(m: KnitMembraneParameters, strain: f64, axis: usize) -> f64 {
    let (q, c, jam) = if axis == 0 {
        (m.q11, m.c1111, m.e1_jam)
    } else {
        (m.q22, m.c2222, m.e2_jam)
    };
    let r2 = (strain / jam).powi(2);
    q * strain + c * strain.powi(3) + 2.0 * m.kappa_jam * r2 / (1.0 - r2) * strain / (jam * jam)
}

#[test]
fn both_static_paths_use_source_nonlinearity_and_declared_wale_course_axes() {
    let mut input = request();
    let material = input.material.record.as_mut().unwrap();
    // No mass, thickness or contact inputs are needed by this strictly planar static experiment.
    material.properties.clear();
    let knit = material.knit_response.as_mut().unwrap();
    knit.bending = None;
    knit.membrane.parameters.q12 = 0.0;
    let m = membrane(&input.material);
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let resolved = catalog.resolve(&input.material).unwrap();
    for (axis, direction, angle) in [(0, [0.0, 1.0], 0.0), (1, [1.0, 0.0], 90.0)] {
        input.fixture.warp_axis = direction;
        let independent = strip::evaluate(&input).unwrap();
        let continuation = strip::simulate(
            &resolved,
            &geometry(&input.fixture, angle),
            &input.extensions,
            &strip::Options::default(),
        )
        .unwrap();
        for (state, sample) in independent.states.iter().zip(&continuation.samples) {
            let stretch = 1.0 + state.extension_fraction;
            let strain = 0.5 * (stretch * stretch - 1.0);
            let mut e = [0.0; 3];
            e[axis] = strain;
            let force = input.fixture.width_m * stretch * uniaxial_stress(m, strain, axis);
            let energy =
                input.fixture.width_m * input.fixture.gauge_length_m * energy_density(m, e);
            near(state.force_n, force, 2e-7);
            near(sample.axial_force_n, force, 2e-7);
            near(state.energy_j, energy, 2e-10);
            near(sample.energy_j, energy, 2e-10);
            near(state.mid_width_m, input.fixture.width_m, 2e-9);
            near(state.fixed_grip_force_n, -force, 2e-7);
        }
        assert_eq!(
            independent
                .input
                .material
                .record
                .as_ref()
                .unwrap()
                .properties
                .len(),
            0
        );
        let replay = strip::evaluate(&independent.input).unwrap();
        assert_eq!(
            serde_json::to_value(replay).unwrap(),
            serde_json::to_value(independent).unwrap()
        );
    }
}

#[test]
fn coupled_finite_grips_produce_nonuniform_contraction_and_correct_virtual_work() {
    let mut input = request();
    input.fixture.cells = [4, 6];
    let extension = 0.15;
    let delta = 1e-5;
    input.extensions = vec![extension - delta, extension, extension + delta];
    let result = strip::evaluate(&input).unwrap();
    let state = &result.states[1];
    assert!(state.mid_width_m < input.fixture.width_m - 1e-5);
    assert!(state.green_strain_range[0][1] - state.green_strain_range[0][0] > 1e-5);
    assert!(state.green_strain_range[1][0] < -1e-4);
    let work_force = (result.states[2].energy_j - result.states[0].energy_j)
        / (2.0 * delta * input.fixture.gauge_length_m);
    near(work_force, state.force_n, 2e-5);
    let strain = 0.5 * ((1.0 + extension).powi(2) - 1.0);
    let falsely_homogeneous = input.fixture.width_m
        * (1.0 + extension)
        * uniaxial_stress(membrane(&input.material), strain, 0);
    assert!((state.force_n - falsely_homogeneous).abs() > 0.01);
    assert!(state.free_force_residual_n <= input.controls.force_tolerance_n);
}

#[test]
fn timed_coupled_history_reports_accepted_storage_dissipation_and_exact_replay() {
    let mut input = history();
    input.fixture.warp_axis = [0.6, 0.8];
    let result = strip::history::evaluate(&input).unwrap();
    let m = membrane(&input.material);
    let bank = &input
        .material
        .record
        .as_ref()
        .unwrap()
        .knit_response
        .as_ref()
        .unwrap()
        .hysteresis
        .as_ref()
        .unwrap()
        .parameters;
    let [u, v] = input.fixture.warp_axis;
    let rest: Vec<_> = result
        .reference_positions_m
        .iter()
        .map(|p| Vec2::new(u * p[0] + v * p[1], -v * p[0] + u * p[1]))
        .collect();
    let elements: Vec<_> = result
        .triangles
        .iter()
        .map(|&tri| MembraneElement::new(tri, tri.map(|i| rest[i])).unwrap())
        .collect();
    let mut last_work = 0.0;
    for (step, state) in result.states.iter().enumerate() {
        let positions: Vec<_> = state
            .positions_m
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect();
        let accepted = state.knit_hysteresis.as_ref().unwrap();
        assert_eq!(accepted["accepted_steps"], step);
        let histories = accepted["elements"].as_array().unwrap();
        assert_eq!(histories.len(), elements.len());
        let mut stored = 0.0;
        let mut work = 0.0;
        for (triangle, (element, history)) in elements.iter().zip(histories).enumerate() {
            assert_eq!(history["triangle"], triangle);
            let strain = element.strain(&positions);
            let backstrain: Vec<[f64; 3]> =
                serde_json::from_value(history["play_backstrain"].clone()).unwrap();
            let springs: f64 = bank
                .weights
                .iter()
                .zip(backstrain)
                .map(|(&weight, p)| {
                    0.5 * weight * (0..3).map(|j| (strain[j] - p[j]).powi(2)).sum::<f64>()
                })
                .sum();
            stored += element.area * (energy_density(m, strain) + springs);
            work += history["dissipated_work_j"].as_f64().unwrap();
        }
        near(state.stored_membrane_energy_j, stored, 2e-12);
        near(state.knit_hysteresis_dissipation_j.unwrap(), work, 2e-12);
        near(accepted["dissipated_work_j"].as_f64().unwrap(), work, 2e-12);
        assert!(work >= last_work);
        assert!(state.free_dynamic_residual_n <= input.controls.force_tolerance_n);
        assert!(
            state.momentum_balance_residual_n
                <= positions.len() as f64 * input.controls.force_tolerance_n
        );
        assert_eq!(state.maxwell_dissipation_j, 0.0);
        assert_eq!(state.plastic_dissipation_j, 0.0);
        last_work = work;
    }
    assert!(last_work > 1e-5);
    near(result.reference_mass_kg, 0.2 * 0.1 * 0.06, 1e-17);
    assert!(
        result
            .states
            .last()
            .unwrap()
            .knit_hysteresis
            .as_ref()
            .unwrap()["max_backstrain"]
            .as_f64()
            .unwrap()
            > 1e-3
    );
    let replay = serde_json::from_str(&result.replay_request_json).unwrap();
    assert_eq!(
        serde_json::to_value(strip::history::evaluate(&replay).unwrap()).unwrap(),
        serde_json::to_value(&result).unwrap()
    );
    input
        .material
        .record
        .as_mut()
        .unwrap()
        .knit_response
        .as_mut()
        .unwrap()
        .hysteresis = None;
    let memoryless = strip::history::evaluate(&input).unwrap();
    assert!(
        memoryless
            .states
            .iter()
            .all(|s| s.knit_hysteresis.is_none() && s.knit_hysteresis_dissipation_j.is_none())
    );
    assert!(
        result
            .states
            .iter()
            .zip(&memoryless.states)
            .any(|(a, b)| (a.force_n - b.force_n).abs() > 0.01)
    );
}

#[test]
fn timed_reactions_use_the_accepted_return_map_without_a_second_slip_update() {
    let mut input = history();
    let material = input.material.record.as_mut().unwrap();
    material.properties.get_mut("areal_density").unwrap().value = 1e-8;
    let knit = material.knit_response.as_mut().unwrap();
    knit.membrane.parameters.q12 = 0.0;
    let m = knit.membrane.parameters;
    let bank = knit.hysteresis.as_ref().unwrap().parameters.clone();
    let result = strip::history::evaluate(&input).unwrap();
    let area = input.fixture.width_m * input.fixture.gauge_length_m;
    let mut p = vec![0.0; bank.weights.len()];
    let mut dissipated = 0.0;
    for (step, state) in result.states.iter().enumerate() {
        let stretch = 1.0 + state.extension_fraction;
        let e = 0.5 * (stretch * stretch - 1.0);
        if step > 0 {
            for ((backstrain, &threshold), &weight) in
                p.iter_mut().zip(&bank.thresholds).zip(&bank.weights)
            {
                let gap = e - *backstrain;
                let norm = (gap * gap + bank.huber_eps * bank.huber_eps).sqrt();
                if norm > threshold {
                    let update = (1.0 - threshold / norm) * gap;
                    *backstrain += update;
                    dissipated += area * weight * threshold * update.abs();
                }
            }
        }
        let stress = uniaxial_stress(m, e, 0)
            + bank
                .weights
                .iter()
                .zip(&p)
                .map(|(w, p)| w * (e - p))
                .sum::<f64>();
        let springs: f64 = bank
            .weights
            .iter()
            .zip(&p)
            .map(|(w, p)| 0.5 * w * (e - p).powi(2))
            .sum();
        near(
            state.force_n,
            input.fixture.width_m * stretch * stress,
            3e-7,
        );
        near(state.fixed_grip_force_n, -state.force_n, 3e-7);
        near(
            state.stored_membrane_energy_j,
            area * (energy_density(m, [e, 0.0, 0.0]) + springs),
            3e-9,
        );
        near(
            state.knit_hysteresis_dissipation_j.unwrap(),
            dissipated,
            3e-9,
        );
    }
    assert!(result.states.last().unwrap().force_n < -0.01);
}

#[test]
fn coupled_coupons_preserve_source_history_mass_jamming_and_work_refusals() {
    let mut input = request();
    input.material = selection(true);
    assert!(
        strip::evaluate(&input)
            .unwrap_err()
            .message
            .contains("history")
    );
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let resolved = catalog.resolve(&input.material).unwrap();
    assert!(
        strip::simulate(
            &resolved,
            &geometry(&input.fixture, 0.0),
            &[0.1],
            &strip::Options::default()
        )
        .is_err()
    );
    input.material = selection(false);
    input.material.allow_estimates = false;
    assert!(strip::evaluate(&input).is_err());
    input.material = selection(false);
    input.material.expected_sha256 = Some("0".repeat(64));
    assert!(strip::evaluate(&input).is_err());
    input.material = selection(false);
    input.extensions = vec![0.0, 0.1, 0.5];
    assert!(strip::evaluate(&input).is_err());
    let resolved = catalog.resolve(&input.material).unwrap();
    assert!(
        strip::simulate(
            &resolved,
            &geometry(&input.fixture, 0.0),
            &input.extensions,
            &strip::Options::default()
        )
        .is_err()
    );
    let mut timed = history();
    timed
        .material
        .record
        .as_mut()
        .unwrap()
        .properties
        .remove("areal_density");
    assert!(strip::history::evaluate(&timed).is_err());
    timed = history();
    timed.fixture.cells = [16, 16];
    timed.controls.max_step_s = 0.0005;
    assert_eq!(
        strip::history::evaluate(&timed).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

//! Independent area-aware heat networks, not measured fabrics or wearer safety predictions.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::exterior::{Exterior, STEFAN_BOLTZMANN},
    surface::{
        Patch, Surface,
        coverage::{Coverage, CoverageStep, ExteriorConditions, Layout, Portion, Region},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::json;

const REFERENCE_AREAS: [f64; 2] = [0.25, 0.75];
const AREAL_CAPACITIES: [f64; 2] = [100., 200.];
const RESISTANCES: [f64; 2] = [0.2, 0.4];
const INTERFACE: f64 = 0.1;
const INSIDE_FILM: f64 = 0.1;

fn column(id: &str, index: usize, eps: Option<f64>, reference: &str) -> DryThermalModel {
    let conditions = json!({"specimen":id,"conditioning":"synthetic-dry"});
    let q = |value, unit| json!({"value":value,"unit":unit,"source":"test","evidence":"estimate","method":"Synthetic independent known answer","conditions":conditions});
    let mut selection = json!({"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic layered sheet, NOT measured textile","form":"woven",
        "properties":{
            "thermal_resistance":q(RESISTANCES[index],"m2*K/W"),
            "areal_density":q(0.2,"kg/m2"),
            "specific_heat":q(AREAL_CAPACITIES[index]/0.2,"J/(kg*K)")
        },
        "sources":{"test":{"reference":reference,"accessed_on":"2026-10-19","note":"Synthetic test data only"}}
    }});
    if let Some(value) = eps {
        selection["record"]["properties"]["emissivity"] = q(value, "1");
        selection["record"]["properties"]["emissivity"]["conditions"] = json!({
            "specimen":id,"conditioning":"synthetic-dry","face":"public",
            "spectral_basis":"total_hemispherical","temperature_min_c":"-200","temperature_max_c":"200"
        });
    }
    let selection: Selection = serde_json::from_value(selection).unwrap();
    DryThermalModel::new(
        vec![Catalog::builtin().unwrap().resolve(&selection).unwrap()],
        vec![],
        1,
    )
    .unwrap()
}

fn source(lining_eps: Option<f64>, shell_eps: Option<f64>, reference: &str) -> Surface {
    Surface::new(
        vec![
            column("lining", 0, lining_eps, reference),
            column("shell", 1, shell_eps, reference),
        ],
        6,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: REFERENCE_AREAS[0],
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: REFERENCE_AREAS[1],
                column: 1,
            },
        ],
    )
    .unwrap()
}

fn layout() -> Layout {
    Layout {
        schema: "ftx-thermal-coverage-layout/1".into(),
        mapping_basis: "Synthetic declared disjoint coverage for an independent network".into(),
        body_vertex_count: 3,
        body_triangles: vec![[0, 1, 2]],
        cells_per_layer: 1,
        regions: vec![Region {
            id: "front".into(),
            body_triangle: 0,
            body_fraction: 1.,
            layers: vec![
                Portion {
                    layer: "lining".into(),
                    cloth_triangle: 0,
                    fraction: 1.,
                },
                Portion {
                    layer: "shell".into(),
                    cloth_triangle: 1,
                    fraction: 1.,
                },
            ],
            interface_resistance_m2_k_w: vec![INTERFACE],
        }],
    }
}

fn coverage(lining_eps: Option<f64>, shell_eps: Option<f64>) -> Coverage {
    Coverage::new(
        source(lining_eps, shell_eps, "test:coverage-exterior"),
        layout(),
    )
    .unwrap()
}

fn geometry(body_area: f64, lining_area: f64, shell_area: f64) -> (Vec<Vec3>, Vec<Vec3>) {
    (
        vec![
            Vec3::new(0., 0., 0.1),
            Vec3::new(2. * lining_area, 0., 0.1),
            Vec3::new(0., 1., 0.1),
            Vec3::new(0., 0., 0.2),
            Vec3::new(2. * shell_area, 0., 0.2),
            Vec3::new(0., 1., 0.2),
        ],
        vec![
            Vec3::ZERO,
            Vec3::new(2. * body_area, 0., 0.),
            Vec3::new(0., 1., 0.),
        ],
    )
}

fn boundary(inside: f64, air: f64) -> Boundary {
    Boundary {
        inside_c: inside,
        outside_c: air,
        inside_resistance: INSIDE_FILM,
        outside_resistance: 0.,
    }
}

fn environment(h: f64, air: f64, radiant: Option<f64>) -> Exterior {
    serde_json::from_value(json!({
        "air_c":air,"convection_w_m2_k":h,"surface_temperature_range_c":[-200.,200.],
        "convection_source":{"reference":"test:coverage-exterior-boundary","accessed_on":"2026-10-19","note":"Synthetic prescribed convection, NOT measured transfer"},
        "convection_evidence":"estimate","convection_conditions":"Synthetic analytic boundary condition",
        "allow_estimates":true,
        "radiation":radiant.map(|t| json!({"ambient_c":t,"emissivity_property":"emissivity","exposed_face":"public","allow_opaque_gray_approximation":true}))
    }))
    .unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-8 * (1. + expected.abs()),
        "{actual} != {expected}"
    );
}

fn fourth(celsius: f64) -> f64 {
    let kelvin = celsius + 273.15;
    kelvin * kelvin * kelvin * kelvin
}

fn conserved(step: &CoverageStep, seconds: f64) {
    near(step.stored_change_j, seconds * step.net_heat_w);
    let scale = 1.
        + step.stored_change_j.abs()
        + seconds * (step.inside_heat_w.abs() + step.outside_heat_w.abs());
    assert!(step.balance_error_j.abs() < 1e-8 * scale);
    let exterior = step.exterior_exchange.as_ref().unwrap();
    near(
        step.outside_heat_w,
        exterior.convective_heat_w + exterior.radiative_heat_w + step.bare_heat_w,
    );
    near(exterior.balance_residual_w, 0.);
}

#[test]
fn current_sheet_areas_set_conductance_and_reference_mass_sets_storage() {
    let model = coverage(None, None);
    let environments = [environment(5., 2., None)];
    let boundaries = [boundary(32., 2.)];
    let seconds = 7.;
    let initial_c = 12.;
    let capacities = [
        AREAL_CAPACITIES[0] * REFERENCE_AREAS[0],
        AREAL_CAPACITIES[1] * REFERENCE_AREAS[1],
    ];
    // Three poses change body area and the two sheet areas independently. No rest mass changes.
    for (body_area, lining_area, shell_area) in
        [(0.5, 0.5, 1.), (1.5, 0.25, 0.75), (0.125, 2., 0.375)]
    {
        let (cloth, body) = geometry(body_area, lining_area, shell_area);
        // Independent whole-region network in W/K, with one thermal cell per material.
        let gi = 1. / (INSIDE_FILM / body_area + RESISTANCES[0] / (2. * lining_area));
        let gm = 1.
            / (RESISTANCES[0] / (2. * lining_area)
                + INTERFACE / body_area
                + RESISTANCES[1] / (2. * shell_area));
        let go = 1. / (RESISTANCES[1] / (2. * shell_area) + 1. / (5. * shell_area));
        let a = capacities[0] / seconds + gi + gm;
        let d = capacities[1] / seconds + gm + go;
        let b0 = capacities[0] / seconds * initial_c + gi * 32.;
        let b1 = capacities[1] / seconds * initial_c + go * 2.;
        let determinant = a * d - gm * gm;
        let expected = [
            (b0 * d + gm * b1) / determinant,
            (a * b1 + gm * b0) / determinant,
        ];
        let inside_w = gi * (32. - expected[0]);
        let outside_w = go * (expected[1] - 2.);
        let stored_j =
            capacities[0] * (expected[0] - initial_c) + capacities[1] * (expected[1] - initial_c);
        let step = model
            .advance_exterior(
                &model.initial(initial_c).unwrap(),
                &cloth,
                &body,
                seconds,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap();
        near(step.state.temperature_c()[0][0], expected[0]);
        near(step.state.temperature_c()[0][1], expected[1]);
        near(step.inside_heat_w, inside_w);
        near(step.outside_heat_w, outside_w);
        near(step.stored_change_j, stored_j);
        near(step.body_area_m2, body_area);
        assert_eq!(step.bare_heat_w, 0.);
        let exterior = step.exterior_exchange.as_ref().unwrap();
        assert_eq!(exterior.regions.len(), 1);
        let region = &exterior.regions[0];
        assert_eq!(region.id, "front");
        assert_eq!(region.body_triangle, 0);
        assert_eq!(region.outer_cloth_triangle, 1);
        assert_eq!(region.outer_layer, "shell");
        near(region.outer_area_m2, shell_area);
        near(region.flux.surface_c, 2. + outside_w / (5. * shell_area));
        near(region.flux.convective_flux_w_m2, outside_w / shell_area);
        near(region.convective_heat_w, outside_w);
        assert_eq!(region.flux.radiative_flux_w_m2, 0.);
        assert!(!region.flux.radiation_enabled);
        conserved(&step, seconds);

        let total_resistance = INSIDE_FILM / body_area
            + RESISTANCES[0] / lining_area
            + INTERFACE / body_area
            + RESISTANCES[1] / shell_area
            + 1. / (5. * shell_area);
        let steady = model
            .steady_exterior(
                &cloth,
                &body,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap();
        near(steady.inside_heat_w, 30. / total_resistance);
        near(steady.outside_heat_w, 30. / total_resistance);
        conserved(&steady, 1.);
    }
}

#[test]
fn manufactured_signed_radiation_matches_steady_and_transient_networks() {
    let eps = 0.8;
    let model = coverage(Some(0.2), Some(eps));
    let (body_area, lining_area, shell_area) = (0.5, 0.75, 1.25);
    let (cloth, body) = geometry(body_area, lining_area, shell_area);
    let capacities = [
        AREAL_CAPACITIES[0] * REFERENCE_AREAS[0],
        AREAL_CAPACITIES[1] * REFERENCE_AREAS[1],
    ];
    let inner_path = INSIDE_FILM / body_area + RESISTANCES[0] / (2. * lining_area);
    let middle_path = RESISTANCES[0] / (2. * lining_area)
        + INTERFACE / body_area
        + RESISTANCES[1] / (2. * shell_area);
    let outer_half = RESISTANCES[1] / (2. * shell_area);
    // Opposed signed terms with both net directions, then a pure radiation boundary with h = 0.
    for (surface, air, radiant, h) in [
        (20., 30., 0., 4.),
        (15., 0., 40., 4.),
        (26.85, -100., 6.85, 0.),
    ] {
        let convection = h * (surface - air);
        let radiation = eps * STEFAN_BOLTZMANN * (fourth(surface) - fourth(radiant));
        let outside_w = shell_area * (convection + radiation);
        let environments = [environment(h, air, Some(radiant))];
        let steady_inside = surface + outside_w * (inner_path + middle_path + outer_half);
        let steady = model
            .steady_exterior(
                &cloth,
                &body,
                &[boundary(steady_inside, air)],
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap();
        near(steady.inside_heat_w, outside_w);
        near(steady.outside_heat_w, outside_w);
        let flux = &steady.exterior_exchange.as_ref().unwrap().regions[0].flux;
        near(flux.surface_c, surface);
        near(flux.convective_flux_w_m2, convection);
        near(flux.radiative_flux_w_m2, radiation);
        conserved(&steady, 1.);

        // Manufacture the full backward-Euler answer from the exposed face toward the body.
        let seconds = 600.;
        let initial_c = 10.;
        let shell_c = surface + outside_w * outer_half;
        let middle_w = outside_w + capacities[1] * (shell_c - initial_c) / seconds;
        let lining_c = shell_c + middle_w * middle_path;
        let inside_w = middle_w + capacities[0] * (lining_c - initial_c) / seconds;
        let inside_c = lining_c + inside_w * inner_path;
        let step = model
            .advance_exterior(
                &model.initial(initial_c).unwrap(),
                &cloth,
                &body,
                seconds,
                &[boundary(inside_c, air)],
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap();
        near(step.state.temperature_c()[0][0], lining_c);
        near(step.state.temperature_c()[0][1], shell_c);
        near(step.inside_heat_w, inside_w);
        near(step.outside_heat_w, outside_w);
        near(
            step.stored_change_j,
            capacities[0] * (lining_c - initial_c) + capacities[1] * (shell_c - initial_c),
        );
        let exterior = step.exterior_exchange.as_ref().unwrap();
        let region = &exterior.regions[0];
        near(region.flux.surface_c, surface);
        near(region.flux.convective_flux_w_m2, convection);
        near(region.flux.radiative_flux_w_m2, radiation);
        near(region.flux.balance_residual_w_m2, 0.);
        near(region.convective_heat_w, shell_area * convection);
        near(region.radiative_heat_w, shell_area * radiation);
        near(region.balance_residual_w, 0.);
        assert!(region.flux.radiation_enabled);
        conserved(&step, seconds);
    }
}

#[test]
fn only_the_exposed_shell_supplies_optics_and_material_swaps_invalidate_old_state() {
    let (cloth, body) = geometry(0.5, 0.5, 1.);
    let boundaries = [boundary(35., 5.)];
    let environments = [environment(3., 5., Some(-10.))];
    let baseline = coverage(None, Some(0.8));
    let expected = baseline
        .steady_exterior(
            &cloth,
            &body,
            &boundaries,
            ExteriorConditions {
                environments: &environments,
                bare_boundaries: &[],
            },
        )
        .unwrap();
    // A hidden lining need not have emissivity at all. Changing it cannot change exterior heat.
    for lining_eps in [0.05, 0.95] {
        let model = coverage(Some(lining_eps), Some(0.8));
        let step = model
            .steady_exterior(
                &cloth,
                &body,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap();
        near(step.outside_heat_w, expected.outside_heat_w);
        for (&actual, &wanted) in step.state.temperature_c()[0]
            .iter()
            .zip(&expected.state.temperature_c()[0])
        {
            near(actual, wanted);
        }
    }
    let low_emissivity = coverage(None, Some(0.1));
    let changed = low_emissivity
        .steady_exterior(
            &cloth,
            &body,
            &boundaries,
            ExteriorConditions {
                environments: &environments,
                bare_boundaries: &[],
            },
        )
        .unwrap();
    assert!(changed.outside_heat_w < expected.outside_heat_w);
    assert_ne!(low_emissivity.snapshot(), baseline.snapshot());
    assert!(
        low_emissivity
            .advance_exterior(
                &expected.state,
                &cloth,
                &body,
                10.,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );

    let missing_shell_optics = coverage(Some(0.8), None);
    assert!(
        missing_shell_optics
            .steady_exterior(
                &cloth,
                &body,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );
    assert!(
        missing_shell_optics
            .steady_exterior(
                &cloth,
                &body,
                &boundaries,
                ExteriorConditions {
                    environments: &[environment(3., 5., None)],
                    bare_boundaries: &[],
                },
            )
            .is_ok()
    );
}

#[test]
fn partial_coverage_uses_body_indexed_environments_and_explicit_bare_films_once() {
    let mut mapping = layout();
    mapping.body_vertex_count = 6;
    mapping.body_triangles.push([3, 4, 5]);
    let mut front = mapping.regions[0].clone();
    front.body_fraction = 0.5;
    for portion in &mut front.layers {
        portion.fraction = 0.25;
    }
    let mut back = front.clone();
    back.id = "back".into();
    back.body_triangle = 1;
    // Deliberately reverse the region order relative to body/environment indexing.
    mapping.regions = vec![back, front];
    let model = Coverage::new(
        source(None, None, "test:coverage-exterior"),
        mapping,
    )
    .unwrap();
    let (cloth, mut body) = geometry(0.5, 0.5, 1.);
    body.extend([
        Vec3::new(0., 0., 0.5),
        Vec3::new(2., 0., 0.5),
        Vec3::new(0., 1., 0.5),
    ]);
    let boundaries = [boundary(30., 0.), boundary(45., 10.)];
    let environments = [environment(5., 0., None), environment(10., 10., None)];
    let bare = [
        Boundary {
            inside_resistance: 0.05,
            outside_resistance: 0.25,
            ..boundaries[0]
        },
        Boundary {
            inside_resistance: 0.05,
            outside_resistance: 0.45,
            ..boundaries[1]
        },
    ];
    let step = model
        .steady_exterior(
            &cloth,
            &body,
            &boundaries,
            ExteriorConditions {
                environments: &environments,
                bare_boundaries: &bare,
            },
        )
        .unwrap();
    let mut covered_w = [0.; 2];
    for (body_index, body_area, h) in [(0, 0.5, 5.), (1, 1., 10.)] {
        let area = 0.5 * body_area;
        let lining_area = 0.25 * 0.5;
        let shell_area = 0.25;
        let resistance = INSIDE_FILM / area
            + RESISTANCES[0] / lining_area
            + INTERFACE / area
            + RESISTANCES[1] / shell_area
            + 1. / (h * shell_area);
        let boundary = boundaries[body_index];
        let covered = (boundary.inside_c - boundary.outside_c) / resistance;
        let bare_power = area * (boundary.inside_c - boundary.outside_c)
            / (bare[body_index].inside_resistance + bare[body_index].outside_resistance);
        covered_w[body_index] = covered;
        near(step.body[body_index].bare_heat_w, bare_power);
        near(step.body[body_index].inside_heat_w, covered + bare_power);
        near(step.body[body_index].outside_heat_w, covered + bare_power);
        let region = &step.exterior_exchange.as_ref().unwrap().regions[1 - body_index];
        assert_eq!(region.body_triangle, body_index);
        near(region.outer_area_m2, shell_area);
        near(region.convective_heat_w, covered);
        near(
            region.flux.surface_c,
            boundary.outside_c + covered / (h * shell_area),
        );
    }
    near(step.body_area_m2, 1.5);
    near(step.covered_area_m2, 0.75);
    near(step.bare_heat_w, 60.);
    near(step.unassigned_cloth_reference_area_m2, 0.5);
    near(step.outside_heat_w, covered_w[0] + covered_w[1] + 60.);
    near(
        step.exterior_exchange.as_ref().unwrap().convective_heat_w,
        covered_w[0] + covered_w[1],
    );
    conserved(&step, 1.);

    let initial = model.initial(10.).unwrap();
    let initial_temperatures = initial.temperature_c().to_vec();
    for bare_boundaries in [&[][..], &bare[..1]] {
        assert!(
            model
                .advance_exterior(
                    &initial,
                    &cloth,
                    &body,
                    10.,
                    &boundaries,
                    ExteriorConditions {
                        environments: &environments,
                        bare_boundaries,
                    },
                )
                .is_err()
        );
    }
    let mut wrong_body_temperature = bare;
    wrong_body_temperature[1].inside_c += 1.;
    assert!(
        model
            .advance_exterior(
                &initial,
                &cloth,
                &body,
                10.,
                &boundaries,
                ExteriorConditions {
                    environments: &environments,
                    bare_boundaries: &wrong_body_temperature,
                },
            )
            .is_err()
    );
    assert_eq!(initial.temperature_c(), initial_temperatures);
}

#[test]
fn frozen_material_replay_accepts_new_boundary_conditions_but_refuses_conflicts() {
    let model = coverage(None, Some(0.8));
    let frozen = Coverage::from_snapshot(&model.snapshot()).unwrap();
    let (cloth, body) = geometry(0.5, 0.5, 1.);
    let initial = model.initial(20.).unwrap();
    let first_boundaries = [boundary(35., 5.)];
    let first_environments = [environment(3., 5., Some(-10.))];
    let first = model
        .advance_exterior(
            &initial,
            &cloth,
            &body,
            12.,
            &first_boundaries,
            ExteriorConditions {
                environments: &first_environments,
                bare_boundaries: &[],
            },
        )
        .unwrap();
    // Continue the same physical material state under new geometry, air and radiation.
    let (next_cloth, next_body) = geometry(0.8, 0.4, 1.5);
    let next_boundaries = [boundary(35., 25.)];
    let next_environments = [environment(8., 25., Some(40.))];
    let continue_with = |coverage: &Coverage| {
        coverage
            .advance_exterior(
                &first.state,
                &next_cloth,
                &next_body,
                9.,
                &next_boundaries,
                ExteriorConditions {
                    environments: &next_environments,
                    bare_boundaries: &[],
                },
            )
            .unwrap()
    };
    let advanced = continue_with(&model);
    let replay = continue_with(&frozen);
    assert_eq!(advanced.state.temperature_c(), replay.state.temperature_c());
    assert_eq!(advanced.inside_heat_w, replay.inside_heat_w);
    assert_eq!(advanced.outside_heat_w, replay.outside_heat_w);
    assert_eq!(advanced.stored_change_j, replay.stored_change_j);
    assert_eq!(
        serde_json::to_value(&advanced.exterior_exchange.as_ref().unwrap().regions[0].flux).unwrap(),
        serde_json::to_value(&replay.exterior_exchange.as_ref().unwrap().regions[0].flux).unwrap()
    );
    conserved(&advanced, 9.);

    let different_source = Coverage::new(
        source(None, Some(0.8), "test:different-source-same-numbers"),
        layout(),
    )
    .unwrap();
    assert!(
        different_source
            .advance_exterior(
                &first.state,
                &next_cloth,
                &next_body,
                9.,
                &next_boundaries,
                ExteriorConditions {
                    environments: &next_environments,
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );
    let mut conflicting = first_boundaries;
    conflicting[0].outside_resistance = 0.2;
    assert!(
        model
            .steady_exterior(
                &cloth,
                &body,
                &conflicting,
                ExteriorConditions {
                    environments: &first_environments,
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );
    conflicting = first_boundaries;
    conflicting[0].outside_c += 1.;
    assert!(
        model
            .steady_exterior(
                &cloth,
                &body,
                &conflicting,
                ExteriorConditions {
                    environments: &first_environments,
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );
    assert!(
        model
            .advance_exterior(
                &initial,
                &cloth,
                &body,
                12.,
                &first_boundaries,
                ExteriorConditions {
                    environments: &[],
                    bare_boundaries: &[],
                },
            )
            .is_err()
    );
}

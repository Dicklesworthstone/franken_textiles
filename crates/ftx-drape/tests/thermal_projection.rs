//! Independent geometric/network answers; synthetic meshes, not physical garment validation.
use ftx_core::{Vec2, Vec3};
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    surface::{
        Patch, Surface,
        coverage::Coverage,
        projection::{self, Interface, Request, Settings},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::json;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}
fn triangle(z: f64) -> [Vec3; 3] {
    [v(0., 0., z), v(1., 0., z), v(0., 1., z)]
}
fn source(tris: &[([Vec3; 3], f64)]) -> (Surface, Vec<Vec3>) {
    let mut columns = Vec::new();
    let mut x = Vec::new();
    let mut patches = Vec::new();
    for (i, (points, r)) in tris.iter().enumerate() {
        let q = |value, unit| json!({"value":value,"unit":unit,"source":"test","evidence":"estimate","method":"synthetic known answer"});
        let s:Selection=serde_json::from_value(json!({"id":"test","allow_estimates":true,"record":{
            "id":"test","name":"Synthetic sheet, not empirical measurements","form":"woven",
            "properties":{"thermal_resistance":q(*r,"m2*K/W"),"areal_density":q(0.2,"kg/m2"),"specific_heat":q(1000.,"J/(kg*K)")},
            "sources":{"test":{"reference":"test:projection","accessed_on":"2026-10-14","note":"Synthetic inputs only"}}}})).unwrap();
        columns.push(
            DryThermalModel::new(
                vec![Catalog::builtin().unwrap().resolve(&s).unwrap()],
                vec![],
                1,
            )
            .unwrap(),
        );
        patches.push(Patch {
            vertices: [i * 3, i * 3 + 1, i * 3 + 2],
            reference_area_m2: 0.5 * (points[1] - points[0]).cross(points[2] - points[0]).norm(),
            column: i,
        });
        x.extend_from_slice(points);
    }
    (Surface::new(columns, x.len(), patches).unwrap(), x)
}
fn request(names: &[&str]) -> Request {
    let unique: std::collections::BTreeSet<_> = names.iter().copied().collect();
    let mut interfaces = Vec::new();
    for &inner in &unique {
        for &outer in &unique {
            if inner != outer {
                interfaces.push(Interface {
                    inner: inner.into(),
                    outer: outer.into(),
                    resistance_m2_k_w: 0.05,
                    basis: "Explicit synthetic interface assumption".into(),
                    stagnant_gap: None,
                });
            }
        }
    }
    Request {
        schema: "ftx-thermal-projection/1".into(),
        cloth_layers: names.iter().map(|s| s.to_string()).collect(),
        settings: Settings {
            max_distance_m: 0.2,
            min_normal_cosine: 0.2,
            separation_tolerance_m: 1e-8,
            cells_per_layer: 1,
            interfaces,
        },
    }
}
fn boundary() -> Boundary {
    Boundary {
        inside_c: 30.,
        outside_c: 0.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8 * (1. + b.abs()), "{a} != {b}");
}

#[test]
fn full_layer_overlap_uses_polygon_area_and_actual_depth_order() {
    // Source order is deliberately opposite to physical order.
    let (s, x) = source(&[(triangle(0.04), 0.4), (triangle(0.02), 0.1)]);
    let body = triangle(0.);
    let req = request(&["shell", "lining"]);
    let p = projection::project(&s, &x, &body, &[[0, 1, 2]], &req).unwrap();
    assert_eq!(p.layout.regions.len(), 1);
    assert_eq!(p.layout.regions[0].layers[0].layer, "lining");
    near(p.layout.regions[0].body_fraction, 1.);
    near(p.footprints[0].hits[0].mid_surface_distance_m[0], 0.02);
    near(p.footprints[0].hits[1].mid_surface_distance_m[1], 0.04);
    let c = Coverage::new(s, p.layout).unwrap();
    let heat = c.steady(&x, &body, &[boundary()]).unwrap();
    near(heat.body_area_m2, 0.5);
    near(heat.covered_area_m2, 0.5);
    near(heat.inside_heat_w, 0.5 * 30. / 0.85);
    near(heat.unassigned_cloth_reference_area_m2, 0.);
}

#[test]
fn partial_coverage_is_clipped_not_classified_by_a_single_ray() {
    let points = [v(0., 0., 0.02), v(0.5, 0., 0.02), v(0., 1., 0.02)];
    let (s, x) = source(&[(points, 0.1)]);
    let body = triangle(0.);
    let p = projection::project(&s, &x, &body, &[[0, 1, 2]], &request(&["shell"])).unwrap();
    near(p.layout.regions.iter().map(|r| r.body_fraction).sum(), 0.5);
    let c = Coverage::new(s, p.layout).unwrap();
    let h = c.steady(&x, &body, &[boundary()]).unwrap();
    near(h.covered_area_m2, 0.25);
    near(h.bare_heat_w, 25.);
    near(h.inside_heat_w, 43.75);
    near(h.unassigned_cloth_reference_area_m2, 0.);
}

#[test]
fn different_body_and_cloth_tessellations_preserve_heat_and_mass() {
    let body = [v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)];
    let faces = [[0, 1, 2], [0, 2, 3]];
    let a = triangle(0.02);
    let b = [v(1., 0., 0.02), v(1., 1., 0.02), v(0., 1., 0.02)];
    let c = [v(0., 0., 0.04), v(1., 0., 0.04), v(1., 1., 0.04)];
    let d = [v(0., 0., 0.04), v(1., 1., 0.04), v(0., 1., 0.04)];
    let (s, x) = source(&[(a, 0.1), (b, 0.1), (c, 0.4), (d, 0.4)]);
    let req = request(&["lining", "lining", "shell", "shell"]);
    let p = projection::project(&s, &x, &body, &faces, &req).unwrap();
    assert!(p.layout.regions.len() >= 4);
    let model = Coverage::new(s, p.layout).unwrap();
    let h = model.steady(&x, &body, &[boundary(); 2]).unwrap();
    near(h.covered_area_m2, 1.);
    near(h.inside_heat_w, 30. / 0.85);
    near(h.unassigned_cloth_reference_area_m2, 0.);
    let t = model
        .advance(
            &model.initial(10.).unwrap(),
            &x,
            &body,
            5.,
            &[boundary(); 2],
        )
        .unwrap();
    let expected =
        200. * (t.state.temperature_c()[0][0] - 10.) + 200. * (t.state.temperature_c()[0][1] - 10.);
    near(t.stored_change_j, expected);
    near(t.balance_error_j, 0.);
}

#[test]
fn inverse_projection_uses_actual_sloped_cloth_area() {
    let points = [v(0., 0., 0.02), v(1., 0., 0.04), v(0., 1., 0.02)];
    let (s, x) = source(&[(points, 0.1)]);
    let body = triangle(0.);
    let p = projection::project(&s, &x, &body, &[[0, 1, 2]], &request(&["shell"])).unwrap();
    near(p.layout.regions[0].layers[0].fraction, 1.);
    let c = Coverage::new(s, p.layout).unwrap();
    let h = c.steady(&x, &body, &[boundary()]).unwrap();
    near(h.inside_heat_w, 15. / (0.3 + 0.1 / (1.0004_f64).sqrt()));
    let step = c
        .advance(&c.initial(10.).unwrap(), &x, &body, 1., &[boundary()])
        .unwrap();
    near(
        step.stored_change_j,
        100. * (1.0004_f64).sqrt() * (step.state.temperature_c()[0][0] - 10.),
    );
}

#[test]
fn spatially_duplicated_cloth_refuses_even_below_scalar_mass_budget() {
    let (s, x) = source(&[(triangle(0.06), 0.1)]);
    let body = [
        v(0., 0., 0.),
        v(0.4, 0., 0.),
        v(0., 0.4, 0.),
        v(0., 0., 0.02),
        v(0.4, 0., 0.02),
        v(0., 0.4, 0.02),
    ];
    let err = projection::project(&s, &x, &body, &[[0, 1, 2], [3, 4, 5]], &request(&["shell"]))
        .unwrap_err();
    assert!(
        err.to_string().contains("same spatial cloth portion"),
        "{err}"
    );
}

#[test]
fn folded_coincident_and_crossing_layers_refuse_not_nearest_wins() {
    for (tris, names) in [
        (
            vec![(triangle(0.02), 0.1), (triangle(0.04), 0.1)],
            vec!["shell", "shell"],
        ),
        (
            vec![(triangle(0.02), 0.1), (triangle(0.02), 0.1)],
            vec!["lining", "shell"],
        ),
        (
            vec![
                (triangle(0.02), 0.1),
                ([v(0., 0., 0.01), v(1., 0., 0.03), v(0., 1., 0.03)], 0.1),
            ],
            vec!["lining", "shell"],
        ),
    ] {
        let (s, x) = source(&tris);
        assert!(
            projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &request(&names)).is_err()
        );
    }
}

#[test]
fn missing_interfaces_and_ambiguous_horizon_or_body_crossings_refuse() {
    let (s, x) = source(&[(triangle(0.02), 0.1), (triangle(0.04), 0.1)]);
    let mut req = request(&["lining", "shell"]);
    req.settings.interfaces.clear();
    assert!(
        projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &req)
            .unwrap_err()
            .to_string()
            .contains("interface")
    );
    for points in [
        [v(0., 0., -0.01), v(1., 0., 0.01), v(0., 1., 0.01)],
        [v(0., 0., 0.1), v(1., 0., 0.3), v(0., 1., 0.1)],
    ] {
        let (s, x) = source(&[(points, 0.1)]);
        assert!(
            projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &request(&["shell"])).is_err()
        );
    }
    let (s, x) = source(&[([v(0., 0., 0.01), v(1., 0., 0.19), v(0., 1., 0.01)], 0.1)]);
    let mut req = request(&["shell"]);
    req.settings.min_normal_cosine = 0.99;
    assert!(
        projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &req)
            .unwrap_err()
            .to_string()
            .contains("grazing")
    );
}

#[test]
fn remote_sheets_leave_explicit_bare_area_and_unassigned_mass() {
    let (s, x) = source(&[(triangle(0.5), 0.1), (triangle(-0.5), 0.1)]);
    let p = projection::project(
        &s,
        &x,
        &triangle(0.),
        &[[0, 1, 2]],
        &request(&["shell", "lining"]),
    )
    .unwrap();
    assert!(p.layout.regions.is_empty());
    let c = Coverage::new(s, p.layout).unwrap();
    let h = c.steady(&x, &triangle(0.), &[boundary()]).unwrap();
    near(h.bare_heat_w, 50.);
    near(h.unassigned_cloth_reference_area_m2, 1.);
}

#[test]
fn rigid_transform_and_repeat_execution_preserve_geometric_answer() {
    let (s, x) = source(&[(triangle(0.02), 0.1)]);
    let body = triangle(0.);
    let req = request(&["shell"]);
    let p = projection::project(&s, &x, &body, &[[0, 1, 2]], &req).unwrap();
    let again = projection::project(&s, &x, &body, &[[0, 1, 2]], &req).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    let transform = |a: Vec3| v(a.z + 0.137, a.x - 0.215, a.y + 0.034);
    let xt: Vec<_> = x.iter().copied().map(transform).collect();
    let bt = body.map(transform);
    let transformed = projection::project(&s, &xt, &bt, &[[0, 1, 2]], &req).unwrap();
    near(
        transformed
            .layout
            .regions
            .iter()
            .map(|r| r.body_fraction)
            .sum(),
        1.,
    );
    let t = Coverage::new(s, transformed.layout)
        .unwrap()
        .steady(&xt, &bt, &[boundary()])
        .unwrap();
    near(t.inside_heat_w, 15. / 0.4);
    assert_ne!(p.input_sha256, transformed.input_sha256);
}

#[test]
fn malformed_geometry_and_layer_controls_refuse_before_output() {
    let (s, x) = source(&[(triangle(0.02), 0.1)]);
    let req = request(&["shell"]);
    for face in [[0, 1, 1], [0, 1, 3]] {
        assert!(projection::project(&s, &x, &triangle(0.), &[face], &req).is_err());
    }
    for mutate in [
        (|r: &mut Request| r.cloth_layers.clear()) as fn(&mut Request),
        |r| r.settings.max_distance_m = f64::NAN,
        |r| r.settings.min_normal_cosine = 0.,
        |r| r.settings.separation_tolerance_m = 0.,
        |r| r.settings.cells_per_layer = usize::MAX,
    ] {
        let mut r = req.clone();
        mutate(&mut r);
        assert!(projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &r).is_err());
    }
    let mut invalid = x.clone();
    invalid[0].x = f64::INFINITY;
    assert!(projection::project(&s, &invalid, &triangle(0.), &[[0, 1, 2]], &req).is_err());
    assert_eq!(x[0], v(0., 0., 0.02));
}

#[test]
fn retained_polygons_are_inside_both_parent_triangles() {
    let (s, x) = source(&[(
        [v(-0.2, 0.1, 0.02), v(0.7, -0.1, 0.02), v(0.3, 1.1, 0.02)],
        0.1,
    )]);
    let p = projection::project(&s, &x, &triangle(0.), &[[0, 1, 2]], &request(&["shell"])).unwrap();
    let check = |v: &Vec2| {
        assert!(
            v.x >= -1e-10 && v.y >= -1e-10 && v.x + v.y <= 1. + 1e-10,
            "{v:?}"
        );
    };
    for f in &p.footprints {
        for v in &f.body_polygon {
            check(v);
        }
        for hit in &f.hits {
            for v in &hit.cloth_polygon {
                check(v);
            }
        }
    }
    assert!(
        p.layout
            .regions
            .iter()
            .map(|r| r.body_fraction)
            .sum::<f64>()
            < 1.
    );
}

#[test]
fn offset_layer_rectangles_match_independent_series_parallel_area_formula() {
    let body = [v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.)];
    for i in 1..=12 {
        let left = i as f64 / 50.;
        let right = 0.55 + i as f64 / 40.;
        // Lining spans [0,0.6]; shell spans [left,right]. The mesh diagonals disagree.
        let a = [v(0., 0., 0.02), v(0.6, 0., 0.02), v(0., 1., 0.02)];
        let b = [v(0.6, 0., 0.02), v(0.6, 1., 0.02), v(0., 1., 0.02)];
        let c = [v(left, 0., 0.04), v(right, 0., 0.04), v(right, 1., 0.04)];
        let d = [v(left, 0., 0.04), v(right, 1., 0.04), v(left, 1., 0.04)];
        let (s, x) = source(&[(a, 0.1), (b, 0.1), (c, 0.4), (d, 0.4)]);
        let req = request(&["lining", "lining", "shell", "shell"]);
        let p = projection::project(&s, &x, &body, &[[0, 1, 2], [0, 2, 3]], &req).unwrap();
        let h = Coverage::new(s, p.layout)
            .unwrap()
            .steady(&x, &body, &[boundary(); 2])
            .unwrap();
        let both = right.min(0.6) - left;
        let only_lining = 0.6 - both;
        let only_shell = right - left - both;
        let bare = 1. - both - only_lining - only_shell;
        let expected = 30. * (both / 0.85 + only_lining / 0.4 + only_shell / 0.7 + bare / 0.3);
        near(h.inside_heat_w, expected);
        near(h.covered_area_m2, 1. - bare);
        near(h.unassigned_cloth_reference_area_m2, 0.);
    }
}

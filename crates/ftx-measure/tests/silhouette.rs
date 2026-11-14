//! Independent analytic projection/coverage tests, never real-person masks or generated oracles.
use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::{
    camera::{Camera, Distortion, Intrinsics},
    silhouette::*,
    triangulate::Partition,
};

fn camera() -> Camera {
    Camera {
        intrinsics: Intrinsics {
            focal_px: Vec2::new(100.0, 100.0),
            principal_px: Vec2::new(100.0, 100.0),
            distortion: Distortion::default(),
        },
        world_to_camera: Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
        center_m: Vec3::new(0., -2., 0.),
    }
}
fn quad(lo: f64, hi: f64, depth: f64) -> SurfaceMesh {
    SurfaceMesh {
        vertices_m: vec![
            Vec3::new(lo, depth, -0.5),
            Vec3::new(hi, depth, -0.5),
            Vec3::new(hi, depth, 0.5),
            Vec3::new(lo, depth, 0.5),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}
fn append(a: &mut SurfaceMesh, b: SurfaceMesh) {
    let n = a.vertices_m.len();
    a.vertices_m.extend(b.vertices_m);
    a.triangles
        .extend(b.triangles.into_iter().map(|t| t.map(|k| k + n)));
}
fn evidence(intervals: Vec<[f64; 2]>) -> Silhouette {
    Silhouette {
        evidence: "analytic synthetic silhouette".into(),
        lines: vec![Scanline {
            axis: Axis::Horizontal,
            at_px: 100.,
            intervals_px: intervals,
            sigma_px: 1.,
            partition: Partition::Fit,
        }],
    }
}

fn normalized_mesh(points: &[[f64; 2]], triangles: Vec<[usize; 3]>) -> SurfaceMesh {
    // The camera above maps these world positions to (2*x, 2*y, 2), so the pinhole
    // normalized coordinates are exactly the supplied points, independently of the lens law.
    SurfaceMesh {
        vertices_m: points
            .iter()
            .map(|&[x, y]| Vec3::new(2.0 * x, 0.0, -2.0 * y))
            .collect(),
        triangles,
    }
}

fn normalized_quad(left: f64, right: f64, bottom: f64, top: f64) -> SurfaceMesh {
    normalized_mesh(
        &[[left, bottom], [right, bottom], [right, top], [left, top]],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

fn assert_brown_intervals(result: &LineComparison, expected: &[[f64; 2]]) {
    let bound = result.prediction_error_bound_px.unwrap();
    assert!(bound.is_finite() && (0.0..=1e-9).contains(&bound));
    assert_eq!(result.predicted_intervals_px.len(), expected.len());
    for (actual, expected) in result
        .predicted_intervals_px
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        // Allow only the final expected-value arithmetic's rounding beyond the published
        // endpoint enclosure, not a pixel-scale chord or raster approximation.
        let rounding = 16.0 * f64::EPSILON * (1.0 + expected.abs());
        assert!(
            (actual - expected).abs() <= bound + rounding,
            "{actual} != {expected} within endpoint bound {bound}"
        );
    }
}

#[test]
fn brown_rectangle_uses_curved_edges_and_keeps_the_boundary_gate_conservative() {
    let mesh = normalized_quad(-0.4, 0.4, -0.6, 0.6);
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = 0.4;
    // At y'=0, y=0 and x'=x*(1+0.4*x^2), giving +/-0.4256. The endpoint
    // chords instead use +/-0.4832 because both corners have |y|=0.6.
    let mut observed = evidence(vec![[57.44, 142.56]]);
    observed.lines.push(Scanline {
        axis: Axis::Vertical,
        at_px: 100.0,
        intervals_px: vec![[31.36, 168.64]], // y'=+/-0.6*(1+0.4*0.6^2)
        sigma_px: 2.0,
        partition: Partition::HeldOut,
    });
    let result = compare(&mesh, &c, [200, 200], &observed, None).unwrap();
    assert_brown_intervals(&result[0], &observed.lines[0].intervals_px);
    assert_brown_intervals(&result[1], &observed.lines[1].intervals_px);
    assert!(result.iter().all(|line| line.passed));
    let wrong_chord = evidence(vec![[51.68, 148.32]]);
    let wrong = compare(&mesh, &c, [200, 200], &wrong_chord, None).unwrap();
    assert!(!wrong[0].passed);
    assert!((wrong[0].boundary_error_px.unwrap() - 5.76).abs() < 1e-8);

    // The independent answer above checks geometry. Displace the certified numerical centers
    // here solely to isolate the decision at exactly 3px from root-coordinate rounding.
    let bound = result[0].prediction_error_bound_px.unwrap();
    assert!(bound > 0.0);
    let [left, right] = result[0].predicted_intervals_px[0];
    let at_limit = evidence(vec![[left + 3.0, right + 3.0]]);
    let limited = compare(&mesh, &c, [200, 200], &at_limit, None).unwrap();
    assert_eq!(limited[0].boundary_error_px, Some(3.0));
    assert!(
        !limited[0].passed,
        "a positive endpoint uncertainty must tighten the gate"
    );
    let inside = evidence(vec![[left + 3.0 - 1e-6, right + 3.0 - 1e-6]]);
    assert!(compare(&mesh, &c, [200, 200], &inside, None).unwrap()[0].passed);

    c.intrinsics.distortion.radial[0] = 0.0;
    let pinhole = compare(&mesh, &c, [200, 200], &evidence(vec![[60.0, 140.0]]), None).unwrap();
    assert!(pinhole[0].passed);
    assert!(pinhole[0].prediction_error_bound_px.is_none());
    assert!(
        serde_json::to_value(&pinhole[0])
            .unwrap()
            .get("prediction_error_bound_px")
            .is_none(),
        "legacy pinhole reports keep their original serialized fields"
    );
}

#[test]
fn brown_bowed_edge_resolves_double_crossings_tangencies_and_shared_vertices() {
    let mesh = normalized_mesh(&[[-0.5, 0.5], [0.5, 0.5], [0.0, 0.875]], vec![[0, 1, 2]]);
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = 1.0;
    // Along the lower edge, y'=0.5*(1+x^2+0.25). At y'=0.65625 the two
    // roots are x=+/-0.25 and x'=+/-0.328125. Every projected vertex has
    // y'>=0.75, so a projected-vertex bounding box would discard real coverage.
    let observed = Silhouette {
        evidence: "independent dyadic radial boundary intersections".into(),
        lines: vec![
            Scanline {
                axis: Axis::Horizontal,
                at_px: 162.5, // y'=0.625: one tangent point, no positive-length foreground
                intervals_px: vec![],
                sigma_px: 1.0,
                partition: Partition::HeldOut,
            },
            Scanline {
                axis: Axis::Horizontal,
                at_px: 165.625,
                intervals_px: vec![[67.1875, 132.8125]],
                sigma_px: 1.0,
                partition: Partition::Fit,
            },
            Scanline {
                axis: Axis::Horizontal,
                at_px: 175.0, // both shared lower vertices: x'=+/-0.75
                intervals_px: vec![[25.0, 175.0]],
                sigma_px: 1.0,
                partition: Partition::HeldOut,
            },
        ],
    };
    let result = compare(&mesh, &c, [200, 200], &observed, None).unwrap();
    for (actual, expected) in result.iter().zip(&observed.lines) {
        assert_brown_intervals(actual, &expected.intervals_px);
        assert!(actual.passed);
    }
    let fit_only = compare(&mesh, &c, [200, 200], &observed, Some(Partition::Fit)).unwrap();
    assert_eq!(fit_only.len(), 1);
    assert_brown_intervals(&fit_only[0], &[[67.1875, 132.8125]]);
}

#[test]
fn brown_coincident_edges_and_tangential_distortion_keep_exact_axis_meaning() {
    let mesh = normalized_mesh(&[[-0.5, 0.0], [0.5, 0.0], [0.0, 0.75]], vec![[0, 1, 2]]);
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = 1.0;
    // The entire base remains on y'=0, not an unresolved infinite collection of roots.
    let observed = evidence(vec![[37.5, 162.5]]);
    let result = compare(&mesh, &c, [300, 300], &observed, None).unwrap();
    assert_brown_intervals(&result[0], &observed.lines[0].intervals_px);
    assert!(result[0].passed);

    let mesh = normalized_quad(-0.4, 0.4, -0.6, 0.6);
    for (tangential, axis, expected) in [
        ([0.0, 0.02], Axis::Horizontal, [68.768, 132.768]),
        ([0.02, 0.0], Axis::Vertical, [30.592, 174.592]),
    ] {
        let mut c = camera();
        c.intrinsics.focal_px = Vec2::new(80.0, 120.0);
        c.intrinsics.distortion.tangential = tangential;
        // On y'=0 with p2=.02, x'=x+3*p2*x^2. On x'=0 with p1=.02,
        // y'=y+3*p1*y^2. Unequal focal lengths must scale the corresponding axis.
        let mut observed = evidence(vec![expected]);
        observed.lines[0].axis = axis;
        let result = compare(&mesh, &c, [200, 200], &observed, None).unwrap();
        assert_brown_intervals(&result[0], &[expected]);
        assert!(result[0].passed);
    }
}

#[test]
fn brown_union_preserves_gaps_overlaps_and_triangle_order_in_both_axes() {
    let mut mesh = normalized_quad(-0.6, -0.2, -0.3, 0.3);
    append(&mut mesh, normalized_quad(0.2, 0.6, -0.3, 0.3));
    append(&mut mesh, normalized_quad(-0.5, -0.25, -0.2, 0.2));
    // A genuinely deeper surface has exactly the first rectangle's projection. Its winding
    // adds to foreground coverage; it must not expose duplicate or hidden contour components.
    let mut rear = normalized_quad(-0.6, -0.2, -0.3, 0.3);
    for vertex in &mut rear.vertices_m {
        vertex.x *= 2.0;
        vertex.y = 2.0;
        vertex.z *= 2.0;
    }
    append(&mut mesh, rear);
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = 0.4;
    let mut observed = evidence(vec![[31.36, 79.68], [120.32, 168.64]]);
    let result = compare(&mesh, &c, [200, 200], &observed, None).unwrap();
    assert_brown_intervals(&result[0], &observed.lines[0].intervals_px);
    assert!(result[0].passed);
    mesh.triangles.reverse();
    for triangle in &mut mesh.triangles {
        triangle.swap(0, 2);
    }
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(compare(&mesh, &c, [200, 200], &observed, None).unwrap()).unwrap()
    );
    // Swap normalized x/y while preserving world depth; vertical winding has the opposite
    // crossing sign convention, but the same two analytic foreground intervals.
    for vertex in &mut mesh.vertices_m {
        let x = vertex.x;
        vertex.x = -vertex.z;
        vertex.z = -x;
    }
    observed.lines[0].axis = Axis::Vertical;
    let result = compare(&mesh, &c, [200, 200], &observed, None).unwrap();
    assert_brown_intervals(&result[0], &observed.lines[0].intervals_px);
    assert!(result[0].passed);
}

#[test]
fn mesh_union_removes_internal_edges_and_keeps_limb_gaps() {
    let mut mesh = quad(-0.8, -0.2, 0.);
    append(&mut mesh, quad(0.2, 0.8, 0.));
    // Rear surfaces covered by the same projection must not add hidden outline edges.
    append(&mut mesh, quad(-1.6, -0.4, 2.));
    let s = evidence(vec![[60., 90.], [110., 140.]]);
    let result = compare(&mesh, &camera(), [200, 200], &s, None).unwrap();
    assert_eq!(result[0].predicted_intervals_px, s.lines[0].intervals_px);
    assert!(result[0].passed);
    assert_eq!(result[0].symmetric_difference_px, 0.);
    assert!(
        result[0]
            .residuals_px
            .as_ref()
            .unwrap()
            .iter()
            .all(|&r| r == 0.)
    );
    mesh.triangles.reverse();
    for t in &mut mesh.triangles {
        t.swap(0, 2);
    }
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(compare(&mesh, &camera(), [200, 200], &s, None).unwrap()).unwrap()
    );
}

#[test]
fn perspective_triangle_slices_and_vertical_lines_have_exact_answers() {
    let mesh = SurfaceMesh {
        vertices_m: vec![
            Vec3::new(-1., 0., 0.),
            Vec3::new(1., 0., 0.),
            Vec3::new(0., 2., 2.),
        ],
        triangles: vec![[0, 1, 2]],
    };
    // Perspective-projected triangle = (50,100), (150,100), (100,50).
    let s = Silhouette {
        evidence: "analytic perspective triangle".into(),
        lines: vec![
            Scanline {
                axis: Axis::Horizontal,
                at_px: 75.,
                intervals_px: vec![[75., 125.]],
                sigma_px: 1.,
                partition: Partition::Fit,
            },
            Scanline {
                axis: Axis::Vertical,
                at_px: 100.,
                intervals_px: vec![[50., 100.]],
                sigma_px: 1.,
                partition: Partition::HeldOut,
            },
        ],
    };
    for c in compare(&mesh, &camera(), [200, 200], &s, None).unwrap() {
        assert_eq!(c.boundary_error_px, Some(0.));
        assert!(c.passed);
    }
    assert_eq!(
        compare(&mesh, &camera(), [200, 200], &s, Some(Partition::Fit))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn extra_components_and_missing_coverage_cannot_pass_one_way_boundary_checks() {
    let s = evidence(vec![[70., 90.]]);
    let mut mesh = quad(-0.6, -0.2, 0.);
    append(&mut mesh, quad(0.4, 0.8, 0.));
    let r = compare(&mesh, &camera(), [200, 200], &s, None)
        .unwrap()
        .remove(0);
    assert!(!r.passed);
    assert_eq!(r.symmetric_difference_px, 20.);
    assert_eq!(r.boundary_error_px, Some(50.));
    let residuals = r.residuals_px.unwrap();
    assert_eq!(&residuals[..2], &[0., 0.]);
    assert_eq!(&residuals[2..6], &[0., 0., 30., 50.]);
    assert!(residuals[6..18].iter().all(|&v| v == 0.));
    assert_eq!(residuals[18], 20.);
    let mut empty = s.clone();
    empty.lines.push(Scanline {
        at_px: 10.,
        intervals_px: vec![[10., 20.]],
        ..s.lines[0].clone()
    });
    let r = compare(&mesh, &camera(), [200, 200], &empty, None).unwrap();
    assert_eq!(r[0].at_px, 10.);
    assert!(!r[0].passed);
    assert!(r[0].residuals_px.is_none());
    assert!(r[0].boundary_error_px.is_none());
    let mut background = s;
    background.lines.push(Scanline {
        at_px: 90.,
        intervals_px: vec![],
        ..background.lines[0].clone()
    });
    assert!(!compare(&mesh, &camera(), [200, 200], &background, None).unwrap()[0].passed);
}

#[test]
fn binary_masks_preserve_half_pixel_boundaries_and_reject_cropped_or_soft_evidence() {
    let mut mask = vec![0; 100];
    for y in 2..8 {
        for x in 3..7 {
            mask[y * 10 + x] = 255;
        }
    }
    let samples = vec![
        MaskSample {
            axis: Axis::Horizontal,
            index: 5,
            sigma_px: 0.5,
            partition: Partition::Fit,
        },
        MaskSample {
            axis: Axis::Vertical,
            index: 4,
            sigma_px: 0.5,
            partition: Partition::HeldOut,
        },
        MaskSample {
            axis: Axis::Horizontal,
            index: 1,
            sigma_px: 1.,
            partition: Partition::HeldOut,
        },
    ];
    let s = sample_mask([10, 10], &mask, &samples, "synthetic mask").unwrap();
    assert!(s.lines[0].intervals_px.is_empty());
    assert_eq!(s.lines[1].intervals_px, vec![[2.5, 6.5]]);
    assert_eq!(s.lines[2].intervals_px, vec![[1.5, 7.5]]);
    mask[0] = 255;
    assert!(sample_mask([10, 10], &mask, &samples, "mask").is_err());
    mask[0] = 1;
    assert!(sample_mask([10, 10], &mask, &samples, "mask").is_err());
    mask[0] = 0;
    assert!(sample_mask([10, 11], &mask, &samples, "mask").is_err());
    assert!(sample_mask([10, 10], &mask, &vec![samples[0].clone(); 2], "mask").is_err());
    let mut overflow = vec![0; 22 * 4];
    for x in (1..21).step_by(2) {
        overflow[22 + x] = 255;
    }
    assert!(
        sample_mask(
            [22, 4],
            &overflow,
            &[MaskSample {
                index: 1,
                ..samples[0].clone()
            }],
            "mask"
        )
        .is_err()
    );
}

#[test]
fn invalid_surfaces_lenses_intervals_and_budgets_refuse_without_truncation() {
    let mesh = quad(-0.6, 0.6, 0.);
    let s = evidence(vec![[70., 130.]]);
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = -1.0;
    assert!(compare(&mesh, &c, [200, 200], &s, None).is_err());
    c.intrinsics.distortion.radial[0] = 0.01;
    c.intrinsics.distortion.max_radius = 0.1;
    assert!(compare(&mesh, &c, [200, 200], &s, None).is_err());
    let epsilon = f64::EPSILON;
    let edge_on = SurfaceMesh {
        // Camera depths 1,2,4 make a nondegenerate spatial triangle with normalized vertices
        // (0,0), (1,1+epsilon), (1-epsilon,1). Its exact projected determinant is +epsilon^2,
        // but the ordinary rounded cross product is zero. It cannot be silently discarded.
        vertices_m: vec![
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(2.0, 0.0, -2.0 * (1.0 + epsilon)),
            Vec3::new(4.0 * (1.0 - epsilon), 2.0, -4.0),
        ],
        triangles: vec![[0, 1, 2]],
    };
    let mut c = camera();
    c.intrinsics.distortion.radial[0] = 0.01;
    assert_eq!(
        compare(&edge_on, &c, [200, 200], &s, None)
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::NotConverged
    );
    for triangles in [
        vec![],
        vec![[0, 1, 10]],
        vec![[0, 1, 1]],
        vec![[0, 1, 2]; 200_001],
    ] {
        let m = SurfaceMesh {
            triangles,
            ..mesh.clone()
        };
        assert!(compare(&m, &camera(), [200, 200], &s, None).is_err());
    }
    for intervals in [
        vec![[90., 70.]],
        vec![[70., 100.], [90., 130.]],
        vec![[f64::NAN, 130.]],
        vec![[70., 201.]],
    ] {
        assert!(compare(&mesh, &camera(), [200, 200], &evidence(intervals), None).is_err());
    }
    let mut duplicate = s.clone();
    duplicate.lines.push(Scanline {
        partition: Partition::HeldOut,
        ..s.lines[0].clone()
    });
    assert!(compare(&mesh, &camera(), [200, 200], &duplicate, None).is_err());
    let mut behind = mesh.clone();
    behind.vertices_m[0].y = -3.;
    assert!(compare(&behind, &camera(), [200, 200], &s, None).is_err());
    let mut collapsed = mesh;
    collapsed.vertices_m[2] = collapsed.vertices_m[1];
    assert!(compare(&collapsed, &camera(), [200, 200], &s, None).is_err());
}

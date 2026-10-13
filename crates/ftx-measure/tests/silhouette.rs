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
    assert_eq!(r.residuals_px, Some(vec![0., 0., 0., 20.]));
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
    c.intrinsics.distortion.radial[0] = 0.01;
    assert!(compare(&mesh, &c, [200, 200], &s, None).is_err());
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

//! Sparse body meshes must not allocate a dense grid over the empty space between components.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::{Body, Contact, ContactParams};

fn patch(at: Vec3) -> Vec<Vec3> {
    [
        Vec3::ZERO,
        Vec3::new(0.0625, 0.0, 0.0),
        Vec3::new(0.0, 0.0625, 0.0),
    ]
    .into_iter()
    .map(|p| p + at)
    .collect()
}

fn params() -> ContactParams {
    ContactParams {
        dhat: 0.001,
        thickness: 0.0005,
        kappa: 10.0,
        mu: 0.0,
        eps_v: 0.001,
    }
}

#[test]
fn widely_separated_body_components_keep_all_contact_queries() {
    // The old grid's cell-count product overflows usize (rather than attempting a huge allocation).
    let far = Vec3::new(1e8, 1e8, 1e8);
    let mut vertices = patch(Vec3::ZERO);
    vertices.extend(patch(far));
    let body = Body::new(vertices, vec![[0, 1, 2], [3, 4, 5]], 0.001).unwrap();
    for at in [Vec3::ZERO, far] {
        let c = Contact::new(body.clone(), params(), vec![[0, 1, 2]], |_, _| 0.0625);
        let touching = patch(at);
        assert!(!c.crossings(&touching, 10).is_empty());
        let clear = patch(at + Vec3::new(0.0, 0.0, 0.002));
        assert!(c.first_intersection(&clear).is_none());
        let gap = c.min_gap(&clear, 0.01).unwrap();
        assert!((gap - 0.0015).abs() < 1e-7, "{gap}");
    }
    let c = Contact::new(body, params(), vec![[0, 1, 2]], |_, _| 0.0625);
    let empty = patch(far * 0.5);
    assert!(c.crossings(&empty, 10).is_empty());
    assert_eq!(c.min_gap(&empty, 0.01), None);
}

#[test]
fn unused_remote_body_vertex_cannot_explode_the_vertex_grid() {
    let mut vertices = patch(Vec3::ZERO);
    vertices.push(Vec3::new(1e8, 1e8, 1e8));
    let body = Body::new(vertices, vec![[0, 1, 2]], 0.001).unwrap();
    let c = Contact::new(body, params(), vec![[0, 1, 2]], |_, _| 0.0625);
    assert!(c.first_intersection(&patch(Vec3::ZERO)).is_some());
    assert!(
        c.first_intersection(&patch(Vec3::new(0.0, 0.0, 1.0)))
            .is_none()
    );
}

#[test]
fn body_rejects_invalid_minimum_cell_size() {
    for cell in [0.0, -0.001, f64::NAN, f64::INFINITY] {
        let code = if cell.is_finite() {
            RefusalCode::OutOfRange
        } else {
            RefusalCode::NonFinite
        };
        assert_eq!(
            Body::new(patch(Vec3::ZERO), vec![[0, 1, 2]], cell)
                .unwrap_err()
                .code,
            code
        );
    }
}

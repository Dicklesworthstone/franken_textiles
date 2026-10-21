//! Conduction-only surface integration: synthetic known answers and source-qualified HFM inputs.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::{SteadyConductionModel, surface::SteadySurface},
    surface::{Patch, Surface},
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::json;

fn selection(r: f64) -> Selection {
    serde_json::from_value(json!({"id":"synthetic","allow_estimates":true,"record":{
        "id":"synthetic","name":"Synthetic resistance without storage","form":"woven",
        "sources":{"test":{"reference":"test:steady-surface","accessed_on":"2026-10-15",
            "note":"Synthetic known answer, not a measured textile"}},
        "properties":{"thermal_resistance":{"value":r,"unit":"m2*K/W","evidence":"estimate",
            "source":"test","method":"Synthetic constant resistance"}}
    }}))
    .unwrap()
}
fn column(r: f64) -> SteadyConductionModel {
    SteadyConductionModel::new(
        vec![Catalog::builtin().unwrap().resolve(&selection(r)).unwrap()],
        vec![],
    )
    .unwrap()
}
fn patch() -> Patch {
    Patch {
        vertices: [0, 1, 2],
        reference_area_m2: 0.5,
        column: 0,
    }
}
fn mesh(x: f64) -> Vec<Vec3> {
    vec![Vec3::ZERO, Vec3::new(x, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)]
}
fn boundary() -> Boundary {
    Boundary {
        inside_c: 30.0,
        outside_c: 0.0,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7, "{a} != {b}");
}
#[test]
fn no_storage_needed_area_scaling_rigid_motion_and_signed_flux() {
    let resolved = Catalog::builtin()
        .unwrap()
        .resolve(&selection(0.4))
        .unwrap();
    assert!(DryThermalModel::new(vec![resolved], vec![], 1).is_err());
    let s = SteadySurface::new(vec![column(0.4)], 3, vec![patch()]).unwrap();
    for scale in [0.25, 1.0, 2.0, 4.0] {
        let x = mesh(scale);
        let q = s.solve(&x, &[boundary()]).unwrap();
        near(q.columns[0].flux_w_m2, 30.0 / 0.7);
        near(q.power_w, scale * 0.5 * 30.0 / 0.7);
        near(q.area_m2, scale * 0.5);
        let moved: Vec<_> = x
            .iter()
            .map(|p| Vec3::new(4.0 + p.z, 2.0 + p.x, -3.0 + p.y))
            .collect();
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            serde_json::to_value(s.solve(&moved, &[boundary()]).unwrap()).unwrap()
        );
        let reverse = Boundary {
            inside_c: 0.0,
            outside_c: 30.0,
            ..boundary()
        };
        near(s.solve(&x, &[reverse]).unwrap().power_w, -q.power_w);
    }
}
#[test]
fn explicit_layers_use_one_area_and_independent_patch_columns() {
    let c = Catalog::builtin().unwrap();
    let layers = [0.2, 0.4]
        .iter()
        .map(|&r| c.resolve(&selection(r)).unwrap())
        .collect();
    let stack = SteadyConductionModel::new(layers, vec![0.1]).unwrap();
    let s = SteadySurface::new(
        vec![stack, column(0.4)],
        4,
        vec![
            patch(),
            Patch {
                vertices: [1, 3, 2],
                column: 1,
                ..patch()
            },
        ],
    )
    .unwrap();
    let x = vec![
        Vec3::ZERO,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
    ];
    let q = s.solve(&x, &[boundary(); 2]).unwrap();
    assert_eq!(q.columns[0].layer_faces_c.len(), 2);
    near(q.area_m2, 1.0);
    near(q.power_w, 0.5 * 30.0 / 1.0 + 0.5 * 30.0 / 0.7);
    near(q.patches[0].power_w, 15.0);
    near(q.patches[1].power_w, 15.0 / 0.7);
}
#[test]
fn source_locked_replay_material_substitution_and_tamper_refusal() {
    let a = SteadySurface::new(vec![column(0.4)], 3, vec![patch()]).unwrap();
    let b = SteadySurface::new(vec![column(0.8)], 3, vec![patch()]).unwrap();
    let snapshot = a.snapshot().unwrap();
    assert_ne!(snapshot["sha256"], b.snapshot().unwrap()["sha256"]);
    assert!(
        b.solve(&mesh(1.0), &[boundary()]).unwrap().power_w
            < a.solve(&mesh(1.0), &[boundary()]).unwrap().power_w
    );
    let restored = SteadySurface::from_snapshot(&snapshot).unwrap();
    assert_eq!(restored.snapshot().unwrap(), snapshot);
    assert!(Surface::from_snapshot(&snapshot).is_err());
    for field in ["reference_area_m2", "column"] {
        let mut changed = snapshot.clone();
        changed["patches"][0][field] = json!(12);
        assert!(SteadySurface::from_snapshot(&changed).is_err());
    }
    let mut changed = snapshot.clone();
    changed["columns"][0]["layers"][0]["record"]["properties"]["thermal_resistance"]["value"] =
        json!(0.8);
    assert!(SteadySurface::from_snapshot(&changed).is_err());
    let mut changed = snapshot;
    changed["columns"][0]["layers"][0]["allow_estimates"] = json!(false);
    assert!(SteadySurface::from_snapshot(&changed).is_err());
}
#[test]
fn real_hfm_runs_support_geometry_without_becoming_transient_or_mechanical_records() {
    let c = Catalog::builtin().unwrap();
    let b = Boundary {
        inside_c: 40.0,
        outside_c: 20.0,
        inside_resistance: 0.0,
        outside_resistance: 0.0,
    };
    let mut powers = Vec::new();
    for id in [
        "fsri/hfm/cotton-sheet/dry/220330-r1",
        "fsri/hfm/cotton-sheet/wet/220318-r1",
    ] {
        let r = c
            .resolve(&Selection {
                id: id.into(),
                allow_estimates: true,
                ..Selection::default()
            })
            .unwrap();
        assert!(r.mechanics().is_err());
        assert!(DryThermalModel::new(vec![r.clone()], vec![], 1).is_err());
        let m = SteadyConductionModel::new(vec![r], vec![]).unwrap();
        let expected = m.solve(b).unwrap().flux_w_m2 * 0.5;
        let s = SteadySurface::new(vec![m], 3, vec![patch()]).unwrap();
        let q = s.solve(&mesh(1.0), &[b]).unwrap();
        near(q.power_w, expected);
        powers.push(q.power_w);
        assert!(
            s.solve(
                &mesh(1.0),
                &[Boundary {
                    outside_c: -30.0,
                    ..b
                }]
            )
            .is_err()
        );
    }
    assert_ne!(powers[0], powers[1]);
}
#[test]
fn invalid_geometry_allocations_and_boundaries_refuse_without_mutation() {
    let s = SteadySurface::new(vec![column(0.4)], 3, vec![patch()]).unwrap();
    let before = s.snapshot().unwrap();
    for x in [mesh(0.0), mesh(f64::NAN), mesh(f64::MAX), vec![]] {
        assert!(s.solve(&x, &[boundary()]).is_err());
    }
    assert!(s.solve(&mesh(1.0), &[]).is_err());
    assert!(
        s.solve(
            &mesh(1.0),
            &[Boundary {
                inside_c: f64::NAN,
                ..boundary()
            }]
        )
        .is_err()
    );
    for p in [
        Patch {
            vertices: [0, 0, 1],
            ..patch()
        },
        Patch {
            vertices: [0, 1, 3],
            ..patch()
        },
        Patch {
            column: 1,
            ..patch()
        },
        Patch {
            reference_area_m2: 0.0,
            ..patch()
        },
    ] {
        assert!(SteadySurface::new(vec![column(0.4)], 3, vec![p]).is_err());
    }
    assert!(SteadySurface::new(vec![column(0.4)], 3, vec![patch(), patch()]).is_err());
    assert!(SteadySurface::new(vec![column(0.4), column(0.2)], 3, vec![patch()]).is_err());
    assert_eq!(s.snapshot().unwrap(), before);
}

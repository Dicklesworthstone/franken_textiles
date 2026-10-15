//! Synthetic geometric/network answers, plus real conductivity records on declared test geometry.
use ftx_core::{Vec3, receipt::sha256_hex};
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::{SteadyConductionModel, coverage::SteadyCoverage, surface::SteadySurface},
    surface::{
        Patch, Surface,
        projection::{self, Interface, Request, Settings, gap::Conduction},
    },
};
use ftx_pattern::{
    material_catalog::{ResolvedMaterial, Selection},
    materials::Catalog,
};
use serde_json::{Value, json};

fn tri(z: f64) -> Vec<Vec3> {
    vec![
        Vec3::new(0., 0., z),
        Vec3::new(1., 0., z),
        Vec3::new(0., 1., z),
    ]
}
fn record(r: f64, thickness: f64, storage: bool) -> ResolvedMaterial {
    let q = |v, u| json!({"value":v,"unit":u,"evidence":"estimate","source":"test","method":"Synthetic known answer"});
    let mut input = json!({"id":"test","allow_estimates":true,"record":{
        "id":"test","name":"Synthetic sheet, not measured textile data","form":"woven",
        "properties":{"thermal_resistance":q(r,"m2*K/W"),"thickness":q(thickness,"m")},
        "sources":{"test":{"reference":"test:steady-projection","accessed_on":"2026-10-15","note":"Synthetic inputs"}}
    }});
    if storage {
        input["record"]["properties"]["areal_density"] = q(0.2, "kg/m2");
        input["record"]["properties"]["specific_heat"] = q(1000., "J/(kg*K)");
    }
    Catalog::builtin()
        .unwrap()
        .resolve(&serde_json::from_value(input).unwrap())
        .unwrap()
}
fn patches(n: usize) -> Vec<Patch> {
    (0..n)
        .map(|i| Patch {
            vertices: [3 * i, 3 * i + 1, 3 * i + 2],
            reference_area_m2: 0.5,
            column: i,
        })
        .collect()
}
fn source(records: &[ResolvedMaterial]) -> SteadySurface {
    SteadySurface::new(
        records
            .iter()
            .map(|m| SteadyConductionModel::new(vec![m.clone()], vec![]).unwrap())
            .collect(),
        3 * records.len(),
        patches(records.len()),
    )
    .unwrap()
}
fn request(names: &[&str]) -> Request {
    Request {
        schema: "ftx-thermal-projection/1".into(),
        cloth_layers: names.iter().map(|s| s.to_string()).collect(),
        settings: Settings {
            max_distance_m: 0.1,
            min_normal_cosine: 0.2,
            separation_tolerance_m: 1e-8,
            cells_per_layer: 2,
            interfaces: if names.len() == 2 {
                vec![Interface {
                    inner: names[0].into(),
                    outer: names[1].into(),
                    resistance_m2_k_w: 0.05,
                    basis: "Synthetic interface assumption".into(),
                    stagnant_gap: None,
                }]
            } else {
                vec![]
            },
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
fn projected(
    s: &SteadySurface,
    x: &[Vec3],
    r: &Request,
) -> Result<projection::Projection, ftx_core::Refusal> {
    s.project_coverage(x, &tri(0.), &[[0, 1, 2]], r)
}

#[test]
fn conduction_only_layers_reach_geometric_coverage_and_series_parallel_heat() {
    let records = [record(0.1, 0.001, false), record(0.4, 0.003, false)];
    for m in &records {
        assert!(DryThermalModel::new(vec![m.clone()], vec![], 1).is_err());
    }
    let s = source(&records);
    let mut x = tri(0.01);
    x.extend(tri(0.02));
    // Lining covers the whole body triangle, shell exactly half its area.
    x[4].x = 0.5;
    let p = projected(&s, &x, &request(&["lining", "shell"])).unwrap();
    assert_eq!(p.layout.regions.len(), 2);
    let q = SteadyCoverage::new(s, p.layout)
        .unwrap()
        .solve(&x, &tri(0.), &[boundary()])
        .unwrap();
    near(q.covered_area_m2, 0.5);
    near(q.bare_power_w, 0.);
    near(q.power_w, 0.25 * 30. / 0.4 + 0.25 * 30. / 0.85);
}

#[test]
fn transient_geometry_and_receipt_identity_remain_unchanged() {
    let records = [record(0.1, 0.001, true), record(0.4, 0.003, true)];
    let s = source(&records);
    let dry = Surface::new(
        records
            .iter()
            .map(|m| DryThermalModel::new(vec![m.clone()], vec![], 2).unwrap())
            .collect(),
        6,
        patches(2),
    )
    .unwrap();
    let mut x = tri(0.01);
    x.extend(tri(0.02));
    x[4].x = 0.5;
    let body = tri(0.);
    let faces = [[0, 1, 2]];
    let req = request(&["lining", "shell"]);
    let a = projected(&s, &x, &req).unwrap();
    let b = projection::project(&dry, &x, &body, &faces, &req).unwrap();
    // Keep the exact pre-extension input identity expression for the storage-bearing path.
    let expected =
        sha256_hex(&serde_json::to_vec(&(dry.snapshot(), &x, &body, &faces, &req)).unwrap());
    assert_eq!(b.input_sha256, expected);
    assert_ne!(a.input_sha256, b.input_sha256);
    let normalize = |p: projection::Projection| {
        let mut v = serde_json::to_value(p).unwrap();
        v["input_sha256"] = json!("capability-specific");
        v["layout"]["mapping_basis"] = json!("capability-specific");
        v
    };
    assert_eq!(normalize(a), normalize(b));
}

#[test]
fn face_gap_and_heat_follow_material_thickness_without_storage_or_geometry_edits() {
    let mut r = request(&["lining", "shell"]);
    r.settings.interfaces[0].stagnant_gap=Some(Conduction {
        conductivity_w_m_k:0.02,
        source:serde_json::from_value(json!({"reference":"test:gas","accessed_on":"2026-10-15","note":"Synthetic constant, not measured gas"})).unwrap(),
        conditions:"Synthetic stagnant gas; no convection or radiation".into(),gap_range_m:[1e-6,0.05],
        relative_tolerance:1e-4,allow_conduction_only_approximation:true,
    });
    let mut x = tri(0.01);
    x.extend(tri(0.016));
    let mut hashes = Vec::new();
    for (thickness, gap) in [(0.001, 0.004), (0.003, 0.003)] {
        let s = source(&[record(0.1, thickness, false), record(0.4, 0.003, false)]);
        let p = projected(&s, &x, &r).unwrap();
        near(p.gap_interfaces[0].surface_gap_m[0], gap);
        near(p.gap_interfaces[0].resistance_m2_k_w, gap / 0.02);
        hashes.push(p.input_sha256.clone());
        let q = SteadyCoverage::new(s, p.layout)
            .unwrap()
            .solve(&x, &tri(0.), &[boundary()])
            .unwrap();
        near(q.power_w, 0.5 * 30. / (0.85 + gap / 0.02));
    }
    assert_ne!(hashes[0], hashes[1]);
    let overlap = source(&[record(0.1, 0.02, false), record(0.4, 0.003, false)]);
    assert!(projected(&overlap, &x, &r).is_err());
    r.settings.interfaces[0]
        .stagnant_gap
        .as_mut()
        .unwrap()
        .allow_conduction_only_approximation = false;
    assert!(
        projected(
            &source(&[record(0.1, 0.001, false), record(0.4, 0.003, false)]),
            &x,
            &r
        )
        .is_err()
    );
}

#[test]
fn real_hfm_material_works_on_declared_geometry_without_becoming_storage_ready() {
    let m = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "fsri/hfm/cotton-sheet/dry/220330-r1".into(),
            allow_estimates: true,
            ..Selection::default()
        })
        .unwrap();
    assert!(m.mechanics().is_err());
    assert!(DryThermalModel::new(vec![m.clone()], vec![], 1).is_err());
    let b = Boundary {
        inside_c: 40.,
        outside_c: 20.,
        inside_resistance: 0.,
        outside_resistance: 0.,
    };
    let flux = SteadyConductionModel::new(vec![m.clone()], vec![])
        .unwrap()
        .solve(b)
        .unwrap()
        .flux_w_m2;
    let s = source(&[m]);
    let x = tri(0.01);
    let r = request(&["sheet"]);
    let p = projected(&s, &x, &r).unwrap();
    let c = SteadyCoverage::new(s.clone(), p.layout).unwrap();
    near(c.solve(&x, &tri(0.), &[b]).unwrap().power_w, 0.5 * flux);
    assert!(
        c.solve(
            &x,
            &tri(0.),
            &[Boundary {
                outside_c: -30.,
                ..b
            }]
        )
        .is_err()
    );
    let restored = SteadySurface::from_snapshot(&s.snapshot().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(projected(&restored, &x, &r).unwrap()).unwrap(),
        serde_json::to_value(projected(&s, &x, &r).unwrap()).unwrap()
    );
}

#[test]
fn pose_changes_recompute_coverage_and_material_receipts_do_not_relabel_geometry() {
    let s = source(&[record(0.1, 0.001, false)]);
    let r = request(&["shell"]);
    let mut x = tri(0.01);
    let full = projected(&s, &x, &r).unwrap();
    // Shift a parallel sheet to leave exactly one quarter of the body area covered.
    for v in &mut x {
        v.x += 0.5;
    }
    let partial = projected(&s, &x, &r).unwrap();
    near(partial.layout.regions[0].body_fraction, 0.25);
    assert_ne!(full.input_sha256, partial.input_sha256);
    let q = SteadyCoverage::new(s, partial.layout)
        .unwrap()
        .solve(&x, &tri(0.), &[boundary()])
        .unwrap();
    near(q.power_w, 0.125 * 30. / 0.4 + 0.375 * 30. / 0.3);
    near(q.unassigned_cloth_reference_area_m2, 0.375);
}

#[test]
fn ambiguous_geometry_invalid_controls_and_stacked_columns_still_refuse() {
    let records = [record(0.1, 0.001, false), record(0.4, 0.003, false)];
    let s = source(&records);
    let r = request(&["lining", "shell"]);
    let mut x = tri(0.01);
    x.extend(tri(0.01));
    assert!(projected(&s, &x, &r).is_err());
    x[3].z = 0.005;
    x[4].z = 0.015;
    x[5].z = 0.015;
    assert!(projected(&s, &x, &r).is_err());
    x[3..].copy_from_slice(&tri(0.02));
    assert!(s.project_coverage(&x, &tri(0.), &[[0, 1, 3]], &r).is_err());
    for count in [0, 4097] {
        let mut bad = r.clone();
        bad.settings.cells_per_layer = count;
        assert!(projected(&s, &x, &bad).is_err());
    }
    let mut bad = r.clone();
    bad.settings.interfaces.clear();
    assert!(projected(&s, &x, &bad).is_err());
    let mut bad = r.clone();
    bad.cloth_layers = vec!["shell".into(); 2];
    bad.settings.interfaces.clear();
    assert!(projected(&s, &x, &bad).is_err());
    let stack = SteadySurface::new(
        vec![SteadyConductionModel::new(records.to_vec(), vec![0.]).unwrap()],
        3,
        patches(1),
    )
    .unwrap();
    assert!(projected(&stack, &tri(0.01), &request(&["shell"])).is_err());
    let mut raw: Value = s.snapshot().unwrap();
    raw["columns"][0]["layers"][0]["allow_estimates"] = json!(false);
    assert!(SteadySurface::from_snapshot(&raw).is_err());
}

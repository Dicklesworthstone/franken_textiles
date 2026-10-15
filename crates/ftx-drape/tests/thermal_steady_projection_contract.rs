//! One geometric mapper for conduction-only and storage-bearing source sheets.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    steady::{SteadyConductionModel, coverage::SteadyCoverage, surface::SteadySurface},
    surface::{
        Patch, Surface,
        coverage::Coverage,
        projection::{self, Request},
    },
};
use ftx_pattern::{material_catalog::Selection, materials::Catalog};
use serde_json::{Value, json};

fn selection(r: f64, t: Option<f64>, storage: bool) -> Selection {
    let mut s = json!({"id":"test","allow_estimates":true,"record":{
        "id":"test","name":"Synthetic projection sheet","form":"woven",
        "sources":{"test":{"reference":"test:steady-projection","accessed_on":"2026-10-15","note":"Synthetic known answer only"}},
        "properties":{"thermal_resistance":{"value":r,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"Synthetic"}}
    }});
    let p = s["record"]["properties"].as_object_mut().unwrap();
    for (key, value, unit) in [
        ("thickness", t, "m"),
        ("areal_density", storage.then_some(0.2), "kg/m2"),
        ("specific_heat", storage.then_some(1000.0), "J/(kg*K)"),
    ] {
        if let Some(value) = value {
            p.insert(key.into(),json!({"value":value,"unit":unit,"evidence":"estimate","source":"test","method":"Synthetic"}));
        }
    }
    serde_json::from_value(s).unwrap()
}
fn patches() -> Vec<Patch> {
    vec![
        Patch {
            vertices: [0, 1, 2],
            column: 0,
            reference_area_m2: 0.5,
        },
        Patch {
            vertices: [3, 4, 5],
            column: 1,
            reference_area_m2: 1.0,
        },
    ]
}
fn sources(t: Option<f64>, storage: bool) -> (SteadySurface, Option<Surface>) {
    let c = Catalog::builtin().unwrap();
    let layers = [0.2, 0.4]
        .iter()
        .map(|&r| c.resolve(&selection(r, t, storage)).unwrap())
        .collect::<Vec<_>>();
    let dry = storage.then(|| {
        Surface::new(
            layers
                .iter()
                .map(|m| DryThermalModel::new(vec![m.clone()], vec![], 1).unwrap())
                .collect(),
            6,
            patches(),
        )
        .unwrap()
    });
    if !storage {
        assert!(DryThermalModel::new(vec![layers[0].clone()], vec![], 1).is_err());
    }
    let steady = SteadySurface::new(
        layers
            .into_iter()
            .map(|m| SteadyConductionModel::new(vec![m], vec![]).unwrap())
            .collect(),
        6,
        patches(),
    )
    .unwrap();
    (steady, dry)
}
fn geometry() -> (Vec<Vec3>, Vec<Vec3>) {
    (
        vec![
            Vec3::new(0., 0., 0.1),
            Vec3::new(1., 0., 0.1),
            Vec3::new(0., 1., 0.1),
            Vec3::new(0., 0., 0.2),
            Vec3::new(2., 0., 0.2),
            Vec3::new(0., 1., 0.2),
        ],
        vec![Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)],
    )
}
fn request() -> Request {
    serde_json::from_value(json!({"schema":"ftx-thermal-projection/1","cloth_layers":["lining","shell"],
    "settings":{"max_distance_m":0.5,"min_normal_cosine":0.1,"separation_tolerance_m":0.000001,"cells_per_layer":1,
        "interfaces":[{"inner":"lining","outer":"shell","resistance_m2_k_w":0.1,"basis":"Synthetic explicit contact"}]}})).unwrap()
}
fn b() -> Boundary {
    Boundary {
        inside_c: 30.,
        outside_c: 0.,
        inside_resistance: 0.1,
        outside_resistance: 0.2,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7, "{a} != {b}");
}

#[test]
fn conduction_only_geometric_overlap_allocates_actual_sheet_portions() {
    let (s, _) = sources(None, false);
    let (cloth, body) = geometry();
    let p = s
        .project_coverage(&cloth, &body, &[[0, 1, 2]], &request())
        .unwrap();
    near(p.layout.regions[0].layers[0].fraction, 1.0);
    near(p.layout.regions[0].layers[1].fraction, 0.5);
    let q = SteadyCoverage::new(s, p.layout)
        .unwrap()
        .solve(&cloth, &body, &[b()])
        .unwrap();
    near(q.covered_area_m2, 0.5);
    near(q.power_w, 15.0);
    near(q.unassigned_cloth_reference_area_m2, 0.5);
}

#[test]
fn storage_capability_changes_no_geometric_polygons_or_constant_steady_heat() {
    let (s, d) = sources(None, true);
    let d = d.unwrap();
    let (cloth, body) = geometry();
    let sp = s
        .project_coverage(&cloth, &body, &[[0, 1, 2]], &request())
        .unwrap();
    let dp = projection::project(&d, &cloth, &body, &[[0, 1, 2]], &request()).unwrap();
    assert_ne!(sp.input_sha256, dp.input_sha256);
    let scrub = |mut value: Value| {
        value["input_sha256"] = Value::Null;
        value["layout"]["mapping_basis"] = Value::Null;
        value
    };
    assert_eq!(scrub(json!(&sp)), scrub(json!(&dp)));
    let sq = SteadyCoverage::new(s, sp.layout)
        .unwrap()
        .solve(&cloth, &body, &[b()])
        .unwrap();
    let dq = Coverage::new(d, dp.layout)
        .unwrap()
        .steady(&cloth, &body, &[b()])
        .unwrap();
    near(sq.power_w, dq.inside_heat_w);
    near(sq.power_w, dq.outside_heat_w);
}

#[test]
fn qualified_face_thickness_changes_declared_gap_without_inventing_cp() {
    let mut value = json!(request());
    value["settings"]["interfaces"][0]["stagnant_gap"] = json!({"conductivity_w_m_k":0.025,
        "source":{"reference":"test:synthetic-gas","accessed_on":"2026-10-15","note":"Synthetic constant gas, no real garment claim"},
        "conditions":"Synthetic dry stagnant gas approximation only","gap_range_m":[0.001,0.2],
        "relative_tolerance":0.000001,"allow_conduction_only_approximation":true});
    let r: Request = serde_json::from_value(value.clone()).unwrap();
    let (cloth, body) = geometry();
    let mut power = Vec::new();
    for thickness in [0.002, 0.004] {
        let (s, _) = sources(Some(thickness), false);
        let p = s.project_coverage(&cloth, &body, &[[0, 1, 2]], &r).unwrap();
        let resistance = (0.1 - thickness) / 0.025;
        near(p.gap_interfaces[0].resistance_m2_k_w, resistance);
        let q = SteadyCoverage::new(s, p.layout)
            .unwrap()
            .solve(&cloth, &body, &[b()])
            .unwrap();
        near(q.power_w, 15.0 / (1.0 + resistance));
        power.push(q.power_w);
    }
    assert!(power[1] > power[0]);
    let (s, _) = sources(None, false);
    assert!(s.project_coverage(&cloth, &body, &[[0, 1, 2]], &r).is_err());
    value["settings"]["interfaces"][0]["stagnant_gap"]["allow_conduction_only_approximation"] =
        json!(false);
    let r = serde_json::from_value(value).unwrap();
    assert!(s.project_coverage(&cloth, &body, &[[0, 1, 2]], &r).is_err());
}

#[test]
fn ambiguous_folds_and_missing_interfaces_still_refuse_without_storage() {
    let (s, _) = sources(None, false);
    let (mut cloth, body) = geometry();
    let mut r = request();
    r.settings.interfaces.clear();
    assert!(s.project_coverage(&cloth, &body, &[[0, 1, 2]], &r).is_err());
    let mut r = request();
    r.cloth_layers[1] = "lining".into();
    r.settings.interfaces.clear();
    assert!(s.project_coverage(&cloth, &body, &[[0, 1, 2]], &r).is_err());
    cloth[3].z = 0.1;
    cloth[4].z = 0.1;
    cloth[5].z = 0.1;
    assert!(
        s.project_coverage(&cloth, &body, &[[0, 1, 2]], &request())
            .is_err()
    );
}

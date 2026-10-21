//! Geometry/transport known answers. These synthetic gas and textile coefficients are not data.
use ftx_core::Vec3;
use ftx_drape::thermal::{
    Boundary, DryThermalModel,
    surface::{
        Patch, Surface,
        coverage::Coverage,
        coverage::motion::Frame,
        projection::{self, Interface, Request, Settings, gap::Conduction},
    },
};
use ftx_pattern::{
    material_catalog::Selection,
    materials::{Catalog, Source},
};
use serde_json::{Value, json};

fn tri(z: f64) -> Vec<Vec3> {
    vec![
        Vec3::new(0., 0., z),
        Vec3::new(1., 0., z),
        Vec3::new(0., 1., z),
    ]
}
fn sheet(id: &str, thickness: f64) -> Selection {
    let q = |v, u| json!({"value":v,"unit":u,"evidence":"estimate","source":"test","method":"synthetic mathematical test"});
    serde_json::from_value(json!({"id":id,"allow_estimates":true,"record":{
        "id":id,"name":"Synthetic, not a real textile","form":"woven",
        "properties":{"thickness":q(thickness,"m"),"areal_density":q(0.2,"kg/m2"),
            "specific_heat":q(1000.,"J/(kg*K)"),"thermal_resistance":q(0.1,"m2*K/W")},
        "sources":{"test":{"reference":"test:gap","accessed_on":"2026-10-14","note":"Synthetic only"}}
    }})).unwrap()
}
fn source(a: &Selection, b: &Selection) -> Surface {
    let db = Catalog::builtin().unwrap();
    Surface::new(
        [a, b]
            .iter()
            .map(|s| DryThermalModel::new(vec![db.resolve(s).unwrap()], vec![], 1).unwrap())
            .collect(),
        6,
        vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap()
}
fn setup() -> (Surface, Vec<Vec3>, Request) {
    let src = source(&sheet("lining", 0.001), &sheet("shell", 0.003));
    let mut cloth = tri(0.01);
    cloth.extend(tri(0.016));
    let request = Request {
        schema: "ftx-thermal-projection/1".into(),
        cloth_layers: vec!["lining".into(), "shell".into()],
        settings: Settings {
            max_distance_m: 0.2,
            min_normal_cosine: 0.2,
            separation_tolerance_m: 1e-8,
            cells_per_layer: 2,
            interfaces: vec![Interface {
                inner: "lining".into(),
                outer: "shell".into(),
                resistance_m2_k_w: 0.05,
                basis: "synthetic interface".into(),
                stagnant_gap: Some(Conduction {
                    conductivity_w_m_k: 0.02,
                    source: Source {
                        reference: "test:gas".into(),
                        accessed_on: "2026-10-14".into(),
                        note: "Not measured gas data".into(),
                    },
                    conditions: "Synthetic constant gas coefficient, no radiation/convection"
                        .into(),
                    gap_range_m: [1e-6, 0.05],
                    relative_tolerance: 1e-4,
                    allow_conduction_only_approximation: true,
                }),
            }],
        },
    };
    (src, cloth, request)
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
    assert!((a - b).abs() < 1e-10 * (1. + b.abs()), "{a} != {b}");
}
fn projected(
    s: &Surface,
    x: &[Vec3],
    r: &Request,
) -> Result<projection::Projection, ftx_core::Refusal> {
    projection::project(s, x, &tri(0.), &[[0, 1, 2]], r)
}

#[test]
fn parallel_gap_uses_actual_faces_and_reaches_steady_and_transient_transport() {
    let (s, x, r) = setup();
    let p = projected(&s, &x, &r).unwrap();
    let gap = &p.gap_interfaces[0];
    near(gap.surface_gap_m[0], 0.004);
    near(gap.surface_gap_m[1], 0.004);
    near(gap.resistance_m2_k_w, 0.2);
    assert_eq!(gap.quadrature_triangles, 1);
    near(p.layout.regions[0].interface_resistance_m2_k_w[0], 0.25);
    let c = Coverage::new(s, p.layout).unwrap();
    let steady = c.steady(&x, &tri(0.), &[boundary()]).unwrap();
    near(steady.inside_heat_w, 0.5 * 30. / 0.75);
    let step = c
        .advance(&c.initial(10.).unwrap(), &x, &tri(0.), 5., &[boundary()])
        .unwrap();
    near(step.balance_error_j, 0.);
    near(step.stored_change_j, 5. * step.net_heat_w);
}

#[test]
fn tilted_wedge_integrates_parallel_conductance_not_midpoint_or_mean_resistance() {
    let (s, mut x, r) = setup();
    let slope = 0.004_f64;
    let cosine = 1. / (1. + slope * slope).sqrt();
    // Offset the outer mid-plane so the true FACE gap is exactly .004*(1+x).
    for v in &mut x[3..] {
        v.z = 0.01 + 0.0005 + 0.0015 / cosine + 0.004 + slope * v.x;
    }
    let p = projected(&s, &x, &r).unwrap();
    let g = &p.gap_interfaces[0];
    near(g.surface_gap_m[0], 0.004);
    near(g.surface_gap_m[1], 0.008);
    // Integral over 0<x<1, 0<y<1-x: mean(1/(1+x)) = 2*(2*ln(2)-1).
    let exact = 0.004 / (0.02 * 2. * (2. * 2_f64.ln() - 1.));
    let [lo, hi] = g.numerical_resistance_bounds_m2_k_w;
    assert!(lo <= exact && exact <= hi, "{lo} <= {exact} <= {hi}");
    assert!(hi - lo <= 1.001e-4 * lo);
    assert!((g.resistance_m2_k_w - exact).abs() < 1e-4 * exact);
    assert!((g.resistance_m2_k_w - (0.004 + 0.004 / 3.) / 0.02).abs() > 0.005);
    assert!(g.quadrature_triangles > 1);
    let again = projected(&s, &x, &r).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(again).unwrap()
    );
}

#[test]
fn selecting_thicker_fabric_changes_gap_heat_and_identity_without_geometry_edits() {
    let (s, x, r) = setup();
    let changed = source(&sheet("lining", 0.003), &sheet("shell", 0.003));
    let a = projected(&s, &x, &r).unwrap();
    let b = projected(&changed, &x, &r).unwrap();
    near(b.gap_interfaces[0].surface_gap_m[0], 0.003);
    near(b.gap_interfaces[0].resistance_m2_k_w, 0.15);
    assert_ne!(a.input_sha256, b.input_sha256);
    let old = Frame::new(s, x.clone(), tri(0.), &[[0, 1, 2]], r.clone()).unwrap();
    let new = Frame::new(changed, x, tri(0.), &[[0, 1, 2]], r).unwrap();
    let state = old.initial(10.).unwrap();
    assert!(new.advance(&state, 1., &[boundary()]).is_err());
    assert!(new.remap(&old, &state).is_err());
    assert!(
        new.steady(&[boundary()]).unwrap().inside_heat_w()
            > old.steady(&[boundary()]).unwrap().inside_heat_w()
    );
}

#[test]
fn changing_pose_recomputes_gap_but_remaps_material_heat_without_a_reset() {
    let (s, mut x, r) = setup();
    let old = Frame::new(s.clone(), x.clone(), tri(0.), &[[0, 1, 2]], r.clone()).unwrap();
    let state = old.steady(&[boundary()]).unwrap().into_state();
    for v in &mut x[3..] {
        v.z += 0.004;
    }
    let new = Frame::new(s, x, tri(0.), &[[0, 1, 2]], r).unwrap();
    near(new.projection().gap_interfaces[0].resistance_m2_k_w, 0.4);
    let transfer = new.remap(&old, &state).unwrap();
    assert_eq!(state.temperature_c(), transfer.state.temperature_c());
    near(transfer.balance.residual_j, 0.);
    let step = new.advance(&transfer.state, 1., &[boundary()]).unwrap();
    near(step.balance_error_j(), 0.);
    assert!(
        new.steady(&[boundary()]).unwrap().inside_heat_w()
            < old.steady(&[boundary()]).unwrap().inside_heat_w()
    );
    let saved = new.checkpoint(&step.into_state()).unwrap();
    let (replayed, replayed_state) = Frame::from_checkpoint(&saved).unwrap();
    assert_eq!(replayed.checkpoint(&replayed_state).unwrap(), saved);
    let mut tampered = saved;
    tampered["frame"]["request"]["settings"]["interfaces"][0]["stagnant_gap"]["conductivity_w_m_k"] =
        json!(0.03);
    assert!(Frame::from_checkpoint(&tampered).is_err());
}

#[test]
fn reverse_sheet_winding_does_not_change_physical_gap() {
    let (s, x, r) = setup();
    let mut snapshot = s.snapshot();
    snapshot["patches"][0]["vertices"] = json!([0, 2, 1]);
    // Build an independently oriented source; do not bypass source snapshot tamper checks.
    let db = Catalog::builtin().unwrap();
    let src = Surface::new(
        vec![
            DryThermalModel::new(
                vec![db.resolve(&sheet("lining", 0.001)).unwrap()],
                vec![],
                1,
            )
            .unwrap(),
            DryThermalModel::new(vec![db.resolve(&sheet("shell", 0.003)).unwrap()], vec![], 1)
                .unwrap(),
        ],
        6,
        vec![
            Patch {
                vertices: [0, 2, 1],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 5, 4],
                reference_area_m2: 0.5,
                column: 1,
            },
        ],
    )
    .unwrap();
    near(
        projected(&src, &x, &r).unwrap().gap_interfaces[0].resistance_m2_k_w,
        projected(&s, &x, &r).unwrap().gap_interfaces[0].resistance_m2_k_w,
    );
}

#[test]
fn overlap_thickness_uncertainty_and_out_of_domain_gaps_refuse() {
    let (s, mut x, mut r) = setup();
    for v in &mut x[3..] {
        v.z = 0.011;
    }
    assert!(
        projected(&s, &x, &r)
            .unwrap_err()
            .message
            .contains("surface gap")
    );
    let (_, x, _) = setup();
    let mut a = sheet("lining", 0.001);
    a.record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("thickness")
        .unwrap()
        .bounds = Some([0.0005, 0.02]);
    assert!(projected(&source(&a, &sheet("shell", 0.003)), &x, &r).is_err());
    r.settings.interfaces[0]
        .stagnant_gap
        .as_mut()
        .unwrap()
        .gap_range_m = [0.005, 0.05];
    assert!(projected(&s, &x, &r).is_err());
}

#[test]
fn incomplete_or_unapproved_gap_inputs_refuse_and_legacy_path_is_unchanged() {
    let (s, x, r) = setup();
    for change in [
        "consent",
        "source",
        "coefficient",
        "conditions",
        "tolerance",
    ] {
        let mut r = r.clone();
        let law = r.settings.interfaces[0].stagnant_gap.as_mut().unwrap();
        match change {
            "consent" => law.allow_conduction_only_approximation = false,
            "source" => law.source.reference.clear(),
            "coefficient" => law.conductivity_w_m_k = f64::NAN,
            "conditions" => law.conditions.clear(),
            _ => law.relative_tolerance = 0.,
        }
        assert!(projected(&s, &x, &r).is_err());
    }
    let mut a = sheet("lining", 0.001);
    a.record.as_mut().unwrap().properties.remove("thickness");
    let missing = source(&a, &sheet("shell", 0.003));
    assert!(projected(&missing, &x, &r).is_err());
    let mut legacy = r;
    legacy.settings.interfaces[0].stagnant_gap = None;
    let json = serde_json::to_value(&legacy).unwrap();
    assert!(
        json["settings"]["interfaces"][0]
            .get("stagnant_gap")
            .is_none()
    );
    let p = projected(&missing, &x, &legacy).unwrap();
    near(p.layout.regions[0].interface_resistance_m2_k_w[0], 0.05);
    let value: Value = serde_json::to_value(p).unwrap();
    assert!(value.get("gap_interfaces").is_none());
}

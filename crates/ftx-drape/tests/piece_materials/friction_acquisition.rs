//! Source-bearing acquisition through real meshing/contact/Newton, not measured textile validation.
use super::*;
use ftx_pattern::material_friction::{Binding, Input, Target, calibrate};
use ftx_pattern::textile_import::catalog;

fn calibrated(mut raw: Value, mu: f64, allowed: bool) -> Pattern {
    let mut input: Input = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    input.target = Target::Pair {
        materials: Box::new([
            serde_json::from_value(raw["fabric"]["material"].clone()).unwrap(),
            serde_json::from_value(raw["pieces"][1]["fabric"]["material"].clone()).unwrap(),
        ]),
    };
    for e in &mut input.experiments {
        for s in &mut e.samples {
            s.pull_n = s.pull_n.map(|f| f.map(|v| v * mu / 0.4));
        }
    }
    let db = catalog().unwrap();
    let report = calibrate(&input, &db).unwrap();
    let Binding::Interface { interface, .. } = report.binding(&db, allowed).unwrap() else {
        panic!()
    };
    assert!((interface.friction.value - mu).abs() < 1e-12);
    raw["fabric"]["interfaces"] = json!([interface]);
    Pattern::new(raw)
}

#[test]
fn fitted_pair_consent_and_counterfaces_are_enforced_by_the_real_resolver() {
    let p = assigned(2.0);
    let opts = DrapeOptions {
        self_contact: Some(options().contact),
        ..options()
    };
    assert!(resolve(&calibrated(p.raw().clone(), 0.4, false), &opts).is_err());
    let bound = calibrated(p.raw().clone(), 0.4, true);
    let r = resolve(&bound, &opts).unwrap();
    assert!((r.report["self_friction_matrix"][0][1].as_f64().unwrap() - 0.4).abs() < 1e-12);
    assert_eq!(r.report["self_friction_matrix"][0][0], 0.3);
    assert_eq!(r.report["self_friction_matrix"][1][1], 0.3);
    assert_eq!(p.raw()["pieces"], bound.raw()["pieces"]);
    let mut stale = bound.raw().clone();
    stale["pieces"][1]["fabric"]["material"]["record"]["properties"]["thickness"]["value"] =
        json!(1.2);
    assert!(resolve(&Pattern::new(stale), &opts).is_err());
}

#[test]
fn changing_only_acquired_pull_forces_changes_actual_two_sheet_slip() {
    let sliding = |mu: f64| {
        let mut raw = assigned(2.0).raw().clone();
        raw["seams"] = json!([]);
        for (i, piece) in raw["pieces"].as_array_mut().unwrap().iter_mut().enumerate() {
            piece["vertices"] = json!({"a":[0,0],"b":[20,0],"c":[20,20],"d":[0,20]});
            piece["placement"]["theta_deg"] = json!(0);
            piece["placement"]["x0"] = json!(10);
            piece["placement"]["radius_mm"] = json!(if i == 0 { 40.0 } else { 41.3 });
        }
        let pattern = calibrated(raw, mu, true);
        let r = run(
            &pattern,
            DrapeOptions {
                self_contact: Some(options().contact),
                spacing: 10.0,
                ..options()
            },
        );
        let fixed: Vec<_> = r
            .scene
            .mesh
            .owner
            .iter()
            .map(|&o| [o == 0, true, true])
            .collect();
        let mut x = r.scene.solver.x;
        for (v, &w) in r.scene.weld.iter().enumerate() {
            x[w] = r.scene.start.relaxed[v];
        }
        let mut solver = ftx_drape::Solver::new(r.scene.solver.model, x, &fixed, Vec3::ZERO);
        solver.contact = r.scene.solver.contact;
        solver = solver.with_self_contact(options().contact).unwrap();
        solver.tol = 1e-8;
        for (i, v) in solver.v.iter_mut().enumerate() {
            if r.scene.mesh.owner[i] == 1 {
                v.x = 0.01;
            }
        }
        (pattern, solver)
    };
    let (pa, mut a) = sliding(0.0);
    let (pb, mut b) = sliding(0.8);
    assert_eq!(pa.raw()["pieces"], pb.raw()["pieces"]);
    assert_eq!(pa.raw()["seams"], pb.raw()["seams"]);
    assert_eq!(a.x, b.x);
    assert_eq!(a.v, b.v);
    assert_eq!(a.model.mass, b.model.mass);
    assert_eq!(a.model.membranes, b.model.membranes);
    assert_eq!(a.model.membrane, b.model.membrane);
    let start: f64 = a.x.iter().map(|p| p.x).sum();
    for _ in 0..5 {
        assert!(a.step(0.001).unwrap().converged);
        assert!(b.step(0.001).unwrap().converged);
        b.contact
            .as_ref()
            .unwrap()
            .audit_self_contact(&b.x)
            .unwrap();
    }
    let free = a.x.iter().map(|p| p.x).sum::<f64>() - start;
    let frictional = b.x.iter().map(|p| p.x).sum::<f64>() - start;
    assert!(
        free > 0.0 && frictional.abs() < free * 0.8,
        "{free} vs {frictional}"
    );
}

//! Accepted Maxwell work -> independent-sheet and moving-coverage heat on sewn synthetic garments.
//! No measured specimen, wearer comfort, or thermal feedback claim.
use ftx_core::{Vec3, det};
use ftx_drape::{
    run::DrapeBody,
    scene::PartFrames,
    start::PartFrame,
    study::{Scenario, Session, motion},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

#[path = "relaxation_heating/surface_water.rs"]
mod surface_water;

fn maxwell(fraction: f64) -> Value {
    let mut s: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-relaxation-heat.json"
    ))
    .unwrap();
    s["record"]["properties"]["membrane_relaxation_heat_fraction"]["value"] = json!(fraction);
    s
}
fn plastic() -> Value {
    let mut s: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
    ))
    .unwrap();
    s["record"]["properties"]["membrane_plastic_heat_fraction"] = json!({
        "value":0.8,"unit":"1","source":"test","evidence":"estimate",
        "method":"Synthetic conversion, not empirical data",
        "conditions":{"specimen_id":"artificial-stretch-A","conditioning":"synthetic constant dry state",
            "work_basis":"irreversible_membrane_plastic_dissipation","temperature_min_c":"-20","temperature_max_c":"80"}});
    s
}
fn garment(back: Value) -> Pattern {
    let w = 40. * std::f64::consts::PI;
    let mut pieces: Vec<_> = ["front", "back"].into_iter().enumerate().map(|(i,id)| json!({
        "id":id,"cut":{"qty":1},"vertices":{"a":[0,0],"b":[w,0],"c":[w,30],"d":[0,30]},
        "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
            {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
        "grain":[[0,0],[0,30]],"placement":{"part":"torso","radius_mm":40,"x0":w/2.,"theta_deg":180*i}
    })).collect();
    pieces[1]["fabric"] = json!({"material":back});
    Pattern::new(
        json!({"schema":"fabric-pattern/1","units":"mm","fabric":{"material":maxwell(0.75)},
        "pieces":pieces,"seams":[{"id":"left","a":"front/right","b":"back/left"},
        {"id":"right","a":"back/right","b":"front/left"}]}),
    )
}
struct Frames;
impl PartFrames for Frames {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0., 0., 1.),
            front: Vec3::new(0., -1., 0.),
            lat: Vec3::new(1., 0., 0.),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.
    }
}
fn scenario() -> Scenario {
    serde_json::from_value(json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":16.,"sew_steps":3,"drape_steps":3,"step":0.0005},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":4000000,
        "motion":{"schema":"ftx-drape-motion/1","keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
            "max_step_s":0.0005,"max_trials":16,"max_pairs":100000,"max_sample_vertices":200000,
            "transient_surface":{"schema":"ftx-motion-surface-heat/1","initial_temperature_c":20.,
                "cells_per_layer":2,"max_step_s":0.00025,"max_cell_steps":5000000,
                "relaxation_dissipation_heat":"uniform_reference_thickness",
                "boundaries":[{"inside_c":20.,"outside_c":20.,
                    "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}]}}})).unwrap()
}
fn coverage_scenario() -> Scenario {
    let mut s = scenario();
    s.motion.as_mut().unwrap().transient_coverage = Some(
        serde_json::from_value(json!({
            "schema":"ftx-motion-coverage-heat/1","body_triangle_ids":[0],
            "instance_layers":{"front":"shell","back":"shell"},
            "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                "separation_tolerance_m":0.00001,"cells_per_layer":2,"interfaces":[]},
            "initial_temperature_c":20.,"max_step_s":0.00025,"max_cell_steps":5000000,
            "max_geometry_operations":1000000,
            "relaxation_dissipation_heat":"uniform_reference_thickness",
            "boundaries":[{"inside_c":20.,"outside_c":20.,
                "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.}],
            "exposed_boundaries":[null]
        }))
        .unwrap(),
    );
    s
}
fn evaluate_unchecked(s: &Scenario, p: &Pattern) -> Value {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (sin, cos) = det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * cos, 0.03 * sin, z));
        }
    }
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    let body = DrapeBody {
        frames: &Frames,
        collision: (&vertices, &triangles),
        display: (&vertices, &triangles),
        arm_deg: 35.,
    };
    let target = vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00005))
        .collect();
    let mut session = Session::new(s, &body).unwrap();
    session
        .set_motion_path(motion::Path::new(
            s.motion.as_ref().unwrap(),
            body.collision,
            vec![target],
        ))
        .unwrap();
    session.evaluate(p, |_| {}).unwrap()
}
fn evaluate(s: &Scenario, p: &Pattern) -> Value {
    let result = evaluate_unchecked(s, p);
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    result
}

fn heat(result: &Value) -> &Value {
    let h = &result["motion"]["result"]["transient_surface"];
    assert_eq!(h["status"], "available", "{h}");
    &h["result"]
}
fn coverage_heat(result: &Value) -> &Value {
    let h = &result["motion"]["result"]["transient_coverage"];
    assert_eq!(h["status"], "available", "{h}");
    &h["result"]
}

// Independently integrate constant-capacity reference mass across covered portions and remaining
// exposed cloth, without using solver-reported heat/storage or treating current area as mass.
fn checkpoint_enthalpy_change(checkpoint: &Value) -> f64 {
    let source = &checkpoint["frame"]["source"];
    let patches = source["patches"].as_array().unwrap();
    let capacities: Vec<_> = patches
        .iter()
        .map(|p| {
            let properties = &source["columns"][p["column"].as_u64().unwrap() as usize]["layers"]
                [0]["normalized"]["properties"];
            p["reference_area_m2"].as_f64().unwrap()
                * properties["areal_density"]["value"].as_f64().unwrap()
                * properties["specific_heat"]["value"].as_f64().unwrap()
        })
        .collect();
    let layout = &checkpoint["frame"]["projection"]["layout"];
    let cells = layout["cells_per_layer"].as_u64().unwrap() as usize;
    let mut allocated = vec![0.; patches.len()];
    let mut stored = 0.;
    for (region, temperatures) in layout["regions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(checkpoint["temperature_c"].as_array().unwrap())
    {
        for (layer, portion) in region["layers"].as_array().unwrap().iter().enumerate() {
            let i = portion["cloth_triangle"].as_u64().unwrap() as usize;
            let fraction = portion["fraction"].as_f64().unwrap();
            allocated[i] += fraction;
            for cell in 0..cells {
                stored += capacities[i] * fraction / cells as f64
                    * (temperatures[layer * cells + cell].as_f64().unwrap() - 20.);
            }
        }
    }
    for (i, temperatures) in checkpoint["exposed_temperature_c"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for t in temperatures.as_array().unwrap() {
            stored +=
                capacities[i] * (1. - allocated[i]) / cells as f64 * (t.as_f64().unwrap() - 20.);
        }
    }
    stored
}
fn unchanged_mechanics(a: &Value, b: &Value) {
    assert_eq!(a["drape"], b["drape"]);
    for key in [
        "final_positions_sha256",
        "final_velocities_m_s",
        "membrane_history",
        "membrane_plasticity_history",
        "bending_history",
    ] {
        assert_eq!(
            a["motion"]["result"][key], b["motion"]["result"][key],
            "{key}"
        );
    }
    for (a, b) in a["motion"]["result"]["samples"]
        .as_array()
        .unwrap()
        .iter()
        .zip(b["motion"]["result"]["samples"].as_array().unwrap())
    {
        for key in ["positions_m", "body_positions_m", "solve", "time_s"] {
            assert_eq!(a[key], b[key], "{key}");
        }
    }
}

fn bending(fraction: f64) -> Value {
    let mut selected: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-plastic-bending.json"
    ))
    .unwrap();
    selected["record"]["properties"]["bending_plastic_heat_fraction"] = json!({
        "value":fraction,"unit":"1","source":"test","evidence":"estimate",
        "method":"Synthetic irreversible-curvature conversion, not measured textile data",
        "conditions":{"specimen_id":"artificial-coupon-A","conditioning":"synthetic constant state",
            "work_basis":"irreversible_bending_plastic_dissipation",
            "temperature_min_c":"-20","temperature_max_c":"80"}});
    selected
}

fn bending_scenario() -> Scenario {
    use ftx_drape::study::heat::DissipationHeatDistribution::UniformReferenceThickness;
    let mut s = coverage_scenario();
    s.options.spacing = 10.;
    let motion = s.motion.as_mut().unwrap();
    let surface = motion.transient_surface.as_mut().unwrap();
    surface.relaxation_dissipation_heat = None;
    surface.bending_plastic_dissipation_heat = Some(UniformReferenceThickness);
    surface.boundaries[0].inside_resistance_m2_k_w = 1e12;
    surface.boundaries[0].outside_resistance_m2_k_w = 1e12;
    let coverage = motion.transient_coverage.as_mut().unwrap();
    coverage.relaxation_dissipation_heat = None;
    coverage.bending_plastic_dissipation_heat = Some(UniformReferenceThickness);
    coverage.boundaries = surface.boundaries.clone();
    s
}

#[test]
fn bending_heat_follows_source_reference_area_and_shares_one_accepted_motion_capture() {
    let mut elastic = bending(0.75);
    elastic["record"]["structure"]
        .as_object_mut()
        .unwrap()
        .remove("bending_plasticity_model");
    let mut raw = garment(elastic).raw().clone();
    raw["fabric"]["material"] = bending(0.75);
    let p = Pattern::new(raw);
    let s = bending_scenario();
    let result = evaluate(&s, &p);
    let h = heat(&result);
    let covered = coverage_heat(&result);
    let source = &h["bending_plastic_dissipation_heat"];
    assert_eq!(source, &covered["bending_plastic_dissipation_heat"]);
    let m = &result["motion"]["result"];
    let initial = m["initial_sample"]["bending_plastic_dissipation_j"]
        .as_array()
        .unwrap();
    let final_work =
        m["samples"].as_array().unwrap().last().unwrap()["bending_plastic_dissipation_j"]
            .as_array()
            .unwrap();
    let patches = h["model"]["patches"].as_array().unwrap();
    let mut expected_work = 0.;
    for (i, (old, current)) in initial.iter().zip(final_work).enumerate() {
        let delta = current.as_f64().unwrap() - old.as_f64().unwrap();
        assert!(delta >= 0.);
        expected_work += delta;
        let owner = result["drape"]["material"]["triangle_instance"][i]
            .as_u64()
            .unwrap() as usize;
        let piece = result["drape"]["material"]["instances"][owner]["piece_index"]
            .as_u64()
            .unwrap();
        if piece == 1 {
            assert_eq!(old, &json!(0.));
            assert_eq!(current, &json!(0.));
        }
        // Almost adiabatic films make this an independent Q = reference mass * cp * dT oracle.
        // No six-node stencil area or current stretched exchange area enters storage.
        let capacity = patches[i]["reference_area_m2"].as_f64().unwrap() * 0.2 * 1200.;
        for t in h["final"]["temperature_c"][i].as_array().unwrap() {
            assert!((t.as_f64().unwrap() - 20. - 0.75 * delta / capacity).abs() < 1e-8);
        }
    }
    assert!(expected_work > 0.);
    assert!(source["initial_plastic_dissipation_j"].as_f64().unwrap() > 0.);
    assert!((source["plastic_dissipation_j"].as_f64().unwrap() - expected_work).abs() < 1e-12);
    let expected_heat = 0.75 * expected_work;
    assert!((source["supplied_heat_j"].as_f64().unwrap() - expected_heat).abs() < 1e-12);
    assert!(
        (h["stored_change_j"].as_f64().unwrap()
            - h["net_boundary_energy_j"].as_f64().unwrap()
            - expected_heat)
            .abs()
            < 1e-10
    );
    assert!(
        (covered["physical_stored_change_j"].as_f64().unwrap()
            - covered["net_boundary_energy_j"].as_f64().unwrap()
            - expected_heat)
            .abs()
            < 1e-10
    );
    assert!(
        (checkpoint_enthalpy_change(&covered["checkpoint"])
            - covered["physical_stored_change_j"].as_f64().unwrap()
            - covered["signed_remap_residual_j"].as_f64().unwrap())
        .abs()
            < 1e-10
    );
    assert!(
        !covered["checkpoint"]["frame"]["projection"]["layout"]["regions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        covered["checkpoint"]["exposed_temperature_c"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| !v.as_array().unwrap().is_empty())
    );
    assert_eq!(
        m["retained_dissipation_scalars"],
        json!((m["samples"].as_array().unwrap().len() + 1) * initial.len())
    );
    assert_eq!(evaluate(&s, &p), result);

    let mut off = s;
    off.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .bending_plastic_dissipation_heat = None;
    off.motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .bending_plastic_dissipation_heat = None;
    let cold = evaluate(&off, &p);
    unchanged_mechanics(&result, &cold);
    assert!(
        cold["motion"]["result"]
            .get("retained_dissipation_scalars")
            .is_none()
    );
    assert!(
        cold["motion"]["result"]["initial_sample"]
            .get("bending_plastic_dissipation_j")
            .is_none()
    );
}

#[test]
fn bending_and_maxwell_heat_sum_once_and_thermal_failure_keeps_the_other_consumer() {
    use ftx_drape::study::heat::DissipationHeatDistribution::UniformReferenceThickness;
    let p = garment(bending(0.8)); // Maxwell front, irreversible curvature on back.
    let mut s = bending_scenario();
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .relaxation_dissipation_heat = Some(UniformReferenceThickness);
    s.motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .relaxation_dissipation_heat = Some(UniformReferenceThickness);
    let result = evaluate(&s, &p);
    let h = heat(&result);
    let covered = coverage_heat(&result);
    let mut supplied = 0.;
    for channel in [
        "bending_plastic_dissipation_heat",
        "relaxation_dissipation_heat",
    ] {
        assert_eq!(h[channel], covered[channel]);
        let q = h[channel]["supplied_heat_j"].as_f64().unwrap();
        assert!(q > 0., "{channel}");
        supplied += q;
    }
    for (heat, stored) in [
        (h, "stored_change_j"),
        (covered, "physical_stored_change_j"),
    ] {
        assert!(
            (heat[stored].as_f64().unwrap()
                - heat["net_boundary_energy_j"].as_f64().unwrap()
                - supplied)
                .abs()
                < 1e-10
        );
    }
    let first = covered["trace"][0]["thermal_substeps"].as_u64().unwrap();
    let all = covered["thermal_steps"].as_u64().unwrap();
    assert!(first < all);
    let mut limited = s.clone();
    limited
        .motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .max_cell_steps = (first * covered["cell_steps"].as_u64().unwrap() / all) as usize;
    let failed = evaluate(&limited, &p);
    unchanged_mechanics(&result, &failed);
    let unaffected = heat(&failed);
    // The path identity deliberately retains the entire motion request, including the
    // other consumer's changed budget. Every physical field and this consumer's own
    // request must remain identical; print only the first differing field on failure.
    assert_ne!(unaffected["path_sha256"], h["path_sha256"]);
    assert_eq!(
        unaffected["path_sha256"],
        failed["motion"]["result"]["path_sha256"]
    );
    assert!(
        unaffected
            .as_object()
            .unwrap()
            .keys()
            .eq(h.as_object().unwrap().keys()),
        "unaffected surface report fields changed"
    );
    for (field, expected) in h.as_object().unwrap() {
        if field != "path_sha256" {
            assert!(
                &unaffected[field] == expected,
                "unaffected surface {field} changed"
            );
        }
    }
    assert_eq!(
        failed["motion"]["result"]["transient_coverage"]["status"],
        "refused"
    );
    assert!(
        failed["motion"]["result"]["transient_coverage"]
            .get("result")
            .is_none()
    );

    // The covered domain independently qualifies conversion even when the sheet consumer does not.
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .bending_plastic_dissipation_heat = None;
    for missing in [false, true] {
        let mut raw = p.raw().clone();
        let properties = &mut raw["pieces"][1]["fabric"]["material"]["record"]["properties"];
        if missing {
            properties
                .as_object_mut()
                .unwrap()
                .remove("bending_plastic_heat_fraction");
        } else {
            properties["bending_plastic_heat_fraction"]["conditions"]["temperature_max_c"] =
                json!("20");
        }
        let failed = evaluate(&s, &Pattern::new(raw));
        assert_eq!(
            failed["motion"]["result"]["final_positions_sha256"],
            result["motion"]["result"]["final_positions_sha256"]
        );
        assert_eq!(
            failed["motion"]["result"]["transient_surface"]["status"],
            "available"
        );
        assert_eq!(
            failed["motion"]["result"]["transient_coverage"]["status"],
            "refused"
        );
        assert!(
            failed["motion"]["result"]["transient_coverage"]
                .get("result")
                .is_none()
        );
    }
}

#[test]
fn accepted_maxwell_work_heats_its_own_triangles_and_excludes_initial_drape() {
    let s = scenario();
    let p = garment(maxwell(0.));
    let r = evaluate(&s, &p);
    let h = heat(&r);
    let m = &r["motion"]["result"];
    let heating = &h["relaxation_dissipation_heat"];
    assert!(heating["initial_viscous_dissipation_j"].as_f64().unwrap() > 0.);
    let initial = &m["initial_sample"]["membrane_relaxation_dissipation_j"];
    let last =
        &m["samples"].as_array().unwrap().last().unwrap()["membrane_relaxation_dissipation_j"];
    let mut expected_work = 0.;
    let mut expected_heat = 0.;
    for (i, owner) in r["drape"]["material"]["triangle_instance"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let delta = last[i].as_f64().unwrap() - initial[i].as_f64().unwrap();
        expected_work += delta;
        let piece =
            r["drape"]["material"]["instances"][owner.as_u64().unwrap() as usize]["piece_index"]
                .as_u64()
                .unwrap();
        if piece == 0 {
            expected_heat += 0.75 * delta;
        } else {
            assert!(
                h["final"]["temperature_c"][i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|t| (t.as_f64().unwrap() - 20.).abs() < 1e-10)
            );
        }
    }
    assert!(expected_heat > 0.);
    assert!((heating["viscous_dissipation_j"].as_f64().unwrap() - expected_work).abs() < 1e-12);
    assert!((heating["supplied_heat_j"].as_f64().unwrap() - expected_heat).abs() < 1e-12);
    assert!(
        (m["membrane_history"]["viscous_step_work_j"]
            .as_f64()
            .unwrap()
            - m["initial_membrane_history"]["viscous_step_work_j"]
                .as_f64()
                .unwrap()
            - expected_work)
            .abs()
            < 1e-12
    );
    assert!(
        (h["stored_change_j"].as_f64().unwrap()
            - h["net_boundary_energy_j"].as_f64().unwrap()
            - expected_heat)
            .abs()
            < 1e-10
    );
    assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-10);
    assert!(h.get("plastic_dissipation_heat").is_none());
    assert!(heating.get("plastic_dissipation_j").is_none());
    assert_eq!(evaluate(&s, &p), r);
    let mut off = s.clone();
    off.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .relaxation_dissipation_heat = None;
    let cold = evaluate(&off, &p);
    unchanged_mechanics(&r, &cold);
    assert!(heat(&cold).get("relaxation_dissipation_heat").is_none());
    assert!(
        cold["motion"]["result"]
            .get("retained_dissipation_scalars")
            .is_none()
    );
    assert!(
        cold["motion"]["result"]["initial_sample"]
            .get("membrane_relaxation_dissipation_j")
            .is_none()
    );
}

#[test]
fn heat_budget_and_source_refusals_discard_thermal_history_without_changing_mechanics() {
    let p = garment(maxwell(0.));
    let s = scenario();
    let r = evaluate(&s, &p);
    let h = heat(&r);
    let first = h["trace"][0]["thermal_substeps"].as_u64().unwrap();
    let all = h["thermal_steps"].as_u64().unwrap();
    assert!(first < all);
    let mut limited = s.clone();
    limited
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .max_cell_steps = (first * h["cell_steps"].as_u64().unwrap() / all) as usize;
    let failed = evaluate(&limited, &p);
    unchanged_mechanics(&r, &failed);
    assert_eq!(
        failed["motion"]["result"]["samples"],
        r["motion"]["result"]["samples"]
    );
    let outcome = &failed["motion"]["result"]["transient_surface"];
    assert_eq!(outcome["status"], "refused");
    assert!(outcome.get("result").is_none());
    assert_eq!(outcome["refusal"]["code"], "budget_exceeded");
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["membrane_relaxation_heat_fraction"]["conditions"]
        ["specimen_id"] = json!("unrelated");
    let refused = evaluate(&s, &Pattern::new(raw));
    assert_eq!(
        refused["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    assert_eq!(
        refused["motion"]["result"]["final_positions_sha256"],
        r["motion"]["result"]["final_positions_sha256"]
    );
    // A valid but narrow temperature domain admits the initial state and refuses generated heating.
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["membrane_relaxation_heat_fraction"]["conditions"]
        ["temperature_max_c"] = json!("20");
    let refused = evaluate(&s, &Pattern::new(raw));
    let refusal = &refused["motion"]["result"]["transient_surface"];
    assert_eq!(refusal["status"], "refused", "{refusal}");
    assert!(
        refusal["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("temperature")
    );
    assert!(refusal.get("result").is_none());
    assert_eq!(
        refused["motion"]["result"]["final_positions_sha256"],
        r["motion"]["result"]["final_positions_sha256"]
    );
}

#[test]
fn mixed_maxwell_and_plastic_sheets_add_only_their_own_work_and_charge_both_channels() {
    let p = garment(plastic());
    let mut s = scenario();
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .plastic_dissipation_heat =
        Some(ftx_drape::study::heat::PlasticHeatDistribution::UniformReferenceThickness);
    let r = evaluate(&s, &p);
    let h = heat(&r);
    let m = &r["motion"]["result"];
    let last = m["samples"].as_array().unwrap().last().unwrap();
    let mut total = 0.;
    for (array, report, work, fraction) in [
        (
            "membrane_relaxation_dissipation_j",
            "relaxation_dissipation_heat",
            "viscous_dissipation_j",
            0.75,
        ),
        (
            "membrane_plastic_dissipation_j",
            "plastic_dissipation_heat",
            "plastic_dissipation_j",
            0.8,
        ),
    ] {
        let initial = m["initial_sample"][array].as_array().unwrap();
        let expected: f64 = last[array]
            .as_array()
            .unwrap()
            .iter()
            .zip(initial)
            .map(|(a, b)| a.as_f64().unwrap() - b.as_f64().unwrap())
            .sum();
        assert!(expected > 0., "{array}");
        assert!((h[report][work].as_f64().unwrap() - expected).abs() < 1e-12);
        assert!(
            (h[report]["supplied_heat_j"].as_f64().unwrap() - expected * fraction).abs() < 1e-12
        );
        total += expected * fraction;
    }
    for (a, b) in last["membrane_relaxation_dissipation_j"]
        .as_array()
        .unwrap()
        .iter()
        .zip(last["membrane_plastic_dissipation_j"].as_array().unwrap())
    {
        assert!(
            a.as_f64().unwrap() == 0. || b.as_f64().unwrap() == 0.,
            "no unimplemented same-triangle viscoplastic law"
        );
    }
    assert!(
        (h["stored_change_j"].as_f64().unwrap()
            - h["net_boundary_energy_j"].as_f64().unwrap()
            - total)
            .abs()
            < 1e-10
    );
    let sample_count = m["samples"].as_array().unwrap().len() + 1;
    let triangles = m["triangles"].as_array().unwrap().len();
    assert_eq!(
        m["retained_dissipation_scalars"],
        json!(sample_count * triangles * 2)
    );
    let mut cold = s.clone();
    let req = cold
        .motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap();
    req.relaxation_dissipation_heat = None;
    req.plastic_dissipation_heat = None;
    unchanged_mechanics(&r, &evaluate(&cold, &p));
    // Exact retained-work budget succeeds, one scalar fewer fails the whole mechanical publication.
    let limit = m["retained_vertex_samples"].as_u64().unwrap()
        + m["retained_dissipation_scalars"].as_u64().unwrap();
    s.motion.as_mut().unwrap().max_sample_vertices = limit as usize;
    assert_eq!(
        evaluate(&s, &p)["motion"]["result"]["samples"],
        m["samples"]
    );
    s.motion.as_mut().unwrap().max_sample_vertices -= 1;
    let refused = evaluate_unchecked(&s, &p);
    assert_eq!(refused["motion"]["status"], "refused");
    assert_eq!(refused["motion"]["refusal"]["code"], "budget_exceeded");
    assert!(refused["motion"].get("result").is_none());
    assert_eq!(refused["drape"], r["drape"]);
}

#[test]
fn coverage_maxwell_heat_follows_reference_mass_and_captures_work_without_surface_history() {
    let p = garment(maxwell(0.));
    let s = coverage_scenario();
    let result = evaluate(&s, &p);
    let coverage = coverage_heat(&result);
    let source = &coverage["relaxation_dissipation_heat"];
    assert_eq!(source, &heat(&result)["relaxation_dissipation_heat"]);
    assert!(source["supplied_heat_j"].as_f64().unwrap() > 0.);
    assert!(source["initial_viscous_dissipation_j"].as_f64().unwrap() > 0.);
    let checkpoint = &coverage["checkpoint"];
    assert!(
        !checkpoint["frame"]["projection"]["layout"]["regions"]
            .as_array()
            .unwrap()
            .is_empty(),
        "exercise covered material as well as exposed cloth"
    );
    assert!(
        checkpoint["exposed_temperature_c"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| !row.as_array().unwrap().is_empty())
    );
    let stored = checkpoint_enthalpy_change(checkpoint);
    assert!(
        (stored
            - coverage["physical_stored_change_j"].as_f64().unwrap()
            - coverage["signed_remap_residual_j"].as_f64().unwrap())
        .abs()
            < 1e-10
    );
    for row in coverage["trace"]
        .as_array()
        .unwrap()
        .iter()
        .chain(std::iter::once(coverage))
    {
        let balance = row["physical_stored_change_j"].as_f64().unwrap()
            - row["inside_energy_j"].as_f64().unwrap()
            + row["outside_energy_j"].as_f64().unwrap()
            - row["relaxation_dissipation_heat"]["supplied_heat_j"]
                .as_f64()
                .unwrap();
        assert!(balance.abs() < 1e-10);
        assert!((balance - row["balance_error_j"].as_f64().unwrap()).abs() < 1e-12);
    }
    let (frame, state) =
        ftx_drape::thermal::surface::coverage::motion::Frame::from_checkpoint(checkpoint).unwrap();
    assert_eq!(frame.checkpoint(&state).unwrap(), *checkpoint);
    let mut only = s.clone();
    only.motion.as_mut().unwrap().transient_surface = None;
    let single = evaluate(&only, &p);
    unchanged_mechanics(&result, &single);
    for field in [
        "relaxation_dissipation_heat",
        "physical_stored_change_j",
        "net_boundary_energy_j",
        "balance_error_j",
        "checkpoint",
        "trace",
    ] {
        assert!(
            coverage_heat(&single)[field] == coverage[field],
            "coverage-only {field}"
        );
    }
    let m = &single["motion"]["result"];
    assert_eq!(
        m["retained_dissipation_scalars"],
        json!(
            (m["samples"].as_array().unwrap().len() + 1) * m["triangles"].as_array().unwrap().len()
        )
    );
    assert_eq!(
        m["retained_dissipation_scalars"],
        result["motion"]["result"]["retained_dissipation_scalars"],
        "two thermal models share one captured work history"
    );
    let mut disabled = only;
    disabled
        .motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .relaxation_dissipation_heat = None;
    let cold = evaluate(&disabled, &p);
    unchanged_mechanics(&single, &cold);
    assert!(checkpoint_enthalpy_change(&coverage_heat(&cold)["checkpoint"]).abs() < 1e-10);
    assert!(
        coverage_heat(&cold)
            .get("relaxation_dissipation_heat")
            .is_none()
    );
    assert!(
        cold["motion"]["result"]
            .get("retained_dissipation_scalars")
            .is_none()
    );
}

#[test]
fn mixed_sources_in_coverage_sum_once_and_each_domain_refuses_independently() {
    use ftx_drape::study::heat::DissipationHeatDistribution::UniformReferenceThickness;
    let p = garment(plastic());
    let mut s = coverage_scenario();
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .plastic_dissipation_heat = Some(UniformReferenceThickness);
    s.motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .plastic_dissipation_heat = Some(UniformReferenceThickness);
    let result = evaluate(&s, &p);
    let coverage = coverage_heat(&result);
    let mut supplied = 0.;
    for channel in ["plastic_dissipation_heat", "relaxation_dissipation_heat"] {
        assert_eq!(coverage[channel], heat(&result)[channel]);
        let value = coverage[channel]["supplied_heat_j"].as_f64().unwrap();
        assert!(value > 0.);
        supplied += value;
    }
    assert!(
        (coverage["physical_stored_change_j"].as_f64().unwrap()
            - coverage["net_boundary_energy_j"].as_f64().unwrap()
            - supplied)
            .abs()
            < 1e-10
    );
    assert!(
        (checkpoint_enthalpy_change(&coverage["checkpoint"])
            - coverage["physical_stored_change_j"].as_f64().unwrap()
            - coverage["signed_remap_residual_j"].as_f64().unwrap())
        .abs()
            < 1e-10
    );
    // Exhaust after the first successful sample, proving no partial coverage heat is published.
    let first = coverage["trace"][0]["thermal_substeps"].as_u64().unwrap();
    let all = coverage["thermal_steps"].as_u64().unwrap();
    assert!(first < all);
    let mut limited = s.clone();
    limited
        .motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .max_cell_steps = (first * coverage["cell_steps"].as_u64().unwrap() / all) as usize;
    let failed = evaluate(&limited, &p);
    unchanged_mechanics(&result, &failed);
    for field in [
        "relaxation_dissipation_heat",
        "plastic_dissipation_heat",
        "stored_change_j",
        "net_boundary_energy_j",
        "balance_error_j",
        "final",
        "trace",
    ] {
        assert!(
            heat(&failed)[field] == heat(&result)[field],
            "unaffected surface {field}"
        );
    }
    let refused = &failed["motion"]["result"]["transient_coverage"];
    assert_eq!(refused["status"], "refused");
    assert_eq!(refused["refusal"]["code"], "budget_exceeded");
    assert!(refused.get("result").is_none());

    // Coverage must inspect its own conversion evidence even if the independent-sheet channel is off.
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .relaxation_dissipation_heat = None;
    for missing in [false, true] {
        let mut raw = p.raw().clone();
        let properties = &mut raw["fabric"]["material"]["record"]["properties"];
        if missing {
            properties
                .as_object_mut()
                .unwrap()
                .remove("membrane_relaxation_heat_fraction");
        } else {
            properties["membrane_relaxation_heat_fraction"]["conditions"]["temperature_max_c"] =
                json!("20");
        }
        let failed = evaluate(&s, &Pattern::new(raw));
        assert_eq!(
            failed["motion"]["result"]["final_positions_sha256"],
            result["motion"]["result"]["final_positions_sha256"]
        );
        assert_eq!(
            failed["motion"]["result"]["transient_surface"]["status"],
            "available"
        );
        let refused = &failed["motion"]["result"]["transient_coverage"];
        assert_eq!(refused["status"], "refused", "{refused}");
        assert!(refused.get("result").is_none());
    }
}

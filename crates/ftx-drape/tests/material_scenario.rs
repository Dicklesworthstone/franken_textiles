//! Real production drapes and heat solves on synthetic geometry; no fitted wearer or measured textile.
use ftx_core::{RefusalCode, Vec3};
use ftx_drape::{
    Body, PartFrame,
    run::{DrapeBody, DrapeOptions, drape},
    scene::PartFrames,
    study::{Scenario, Session},
    thermal::steady::coverage::request as coverage,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

struct Form;
impl PartFrames for Form {
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
fn form() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.);
            vertices.push(Vec3::new(0.03 * c, 0.03 * s, z));
        }
    }
    let mut triangles = Vec::new();
    for i in 0..16 {
        let j = (i + 1) % 16;
        triangles.extend([[i, j, j + 16], [i, j + 16, i + 16]]);
    }
    Body::new(vertices, triangles, 0.01).unwrap()
}
fn body(form: &Body) -> DrapeBody<'_> {
    DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.,
    }
}
fn pattern() -> Pattern {
    let w = 40. * std::f64::consts::TAU;
    let mut props = serde_json::Map::new();
    for (k, v, u) in [
        ("areal_density", 0.2, "kg/m2"),
        ("thickness", 0.0004, "m"),
        ("warp_stiffness", 640., "N/m"),
        ("weft_stiffness", 420., "N/m"),
        ("shear_stiffness", 95., "N/m"),
        ("poisson_warp_weft", 0.2, "1"),
        ("bending_rigidity", 2.8e-5, "N*m"),
        ("friction_skin", 0.24, "1"),
        ("friction_self", 0.36, "1"),
        ("thermal_conductivity", 0.05, "W/(m*K)"),
        ("width", 1.4, "m"),
    ] {
        props.insert(
            k.into(),
            json!({"value":v,"unit":u,"source":"test","evidence":"estimate",
            "method":"Synthetic software gate; not measured textile data"}),
        );
    }
    Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":{"id":"synthetic-scenario-sheet","allow_estimates":true,"record":{
            "id":"synthetic-scenario-sheet","name":"Synthetic scenario sheet","form":"woven",
            "properties":props,"sources":{"test":{"reference":"test:scenario",
                "accessed_on":"2026-10-16","note":"Synthetic software parameters only"}}}}},
        "pieces":[{"id":"band","cut":{"qty":1},"grain":[[0.,0.],[0.,20.]],
            "vertices":{"a":[0.,0.],"b":[w,0.],"c":[w,20.],"d":[0.,20.]},
            "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
            "placement":{"part":"torso","radius_mm":40.,"x0":w/2.}}],
        "seams":[{"id":"join","a":"band/right","b":"band/left"}]}))
}
fn scenario() -> Scenario {
    let film = json!({"inside_c":30.,"outside_c":10.,"inside_resistance_m2_k_w":0.1,
        "outside_resistance_m2_k_w":0.2});
    serde_json::from_value(json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":24.,"sew_steps":3,"drape_steps":3,"step":0.001},"fit_spec":{},
        "max_mesh_points":4096,"max_vertex_frames":1000000,
        "surface":{"schema":"ftx-steady-surface-conditions/1","boundaries":[film]},
        "coverage":{"body_triangle_ids":[0],"instance_layers":{"band":"shell"},
            "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                "separation_tolerance_m":0.00001,"cells_per_layer":1,"interfaces":[]},
            "conditions":{"boundaries":[film]}}}))
    .unwrap()
}
fn changed(pattern: &Pattern, key: &str, value: f64) -> Pattern {
    let mut raw = pattern.raw().clone();
    raw["fabric"]["material"]["record"]["properties"][key]["value"] = json!(value);
    Pattern::new(raw)
}

fn vapor_pattern() -> Pattern {
    let mut raw = pattern().raw().clone();
    raw["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thermal_conductivity");
    raw["fabric"]["material"]["record"]["properties"]["vapor_resistance"] = json!({
        "value":7.,"unit":"m2*Pa/W","source":"test","evidence":"estimate",
        "method":"Synthetic software gate; not measured textile data"});
    Pattern::new(raw)
}

#[test]
fn vapor_substitution_uses_current_sewn_geometry_without_dry_heat_or_storage() {
    let form = form();
    let body = body(&form);
    let p = vapor_pattern();
    let mut s = scenario();
    s.vapor_surface = Some(
        serde_json::from_value(json!({
        "schema":"ftx-vapor-surface-conditions/1","boundaries":[{
            "inside_vapor_pressure_pa":2000.,"outside_vapor_pressure_pa":1000.,
            "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":2.,
            "latent_heat_j_kg":2_500_000.}]}))
        .unwrap(),
    );
    let ordinary = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let direct = s
        .vapor_surface
        .as_ref()
        .unwrap()
        .evaluate(&ordinary.report)
        .unwrap();
    let mut session = Session::new(&s, &body).unwrap();
    let a = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(a["vapor_surface"]["status"], "available");
    assert_eq!(a["vapor_surface"]["result"], direct);
    assert_eq!(a["surface"]["status"], "refused");
    assert_eq!(a["coverage"]["status"], "refused");
    assert_eq!(a["drape"]["thermal_surface"]["available"], false);
    let current_area: f64 = ordinary
        .scene
        .solver
        .model
        .membranes
        .iter()
        .map(|m| {
            let [a, b, c] = m.v.map(|i| ordinary.scene.solver.x[i]);
            0.5 * (b - a).cross(c - a).norm()
        })
        .sum();
    let heat = a["metrics"]["independent_vapor_potential_heat_flow_w"]
        .as_f64()
        .unwrap();
    assert!((heat - current_area * 100.).abs() < 1e-12);
    let b = session
        .evaluate(&changed(&p, "vapor_resistance", 17.), |_| {})
        .unwrap();
    assert_eq!(a["positions_m"], b["positions_m"]);
    assert_eq!(a["metrics"]["cloth_mass_kg"], b["metrics"]["cloth_mass_kg"]);
    assert_ne!(
        a["vapor_surface"]["result"]["model_sha256"],
        b["vapor_surface"]["result"]["model_sha256"]
    );
    let changed_heat = b["metrics"]["independent_vapor_potential_heat_flow_w"]
        .as_f64()
        .unwrap();
    assert!((changed_heat - heat / 2.).abs() < 1e-12);
    assert_eq!(a, session.evaluate(&p, |_| {}).unwrap());

    let no_vapor = session.evaluate(&pattern(), |_| {}).unwrap();
    assert_eq!(no_vapor["vapor_surface"]["status"], "refused");
    assert!(no_vapor["metrics"]["independent_vapor_potential_heat_flow_w"].is_null());
    assert!(no_vapor["metrics"]["cloth_mass_kg"].is_number());
    assert_eq!(no_vapor["surface"]["status"], "available");

    s.motion = motion_scenario().motion;
    s.vapor_coverage = Some(
        serde_json::from_value(json!({
            "body_triangle_ids":[0],"instance_layers":{"band":"shell"},
            "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                "separation_tolerance_m":0.00001,"interfaces":[]},
            "conditions":{"boundaries":[{
                "inside_vapor_pressure_pa":2000.,"outside_vapor_pressure_pa":1000.,
                "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":2.,
                "latent_heat_j_kg":2_500_000.}]}
        }))
        .unwrap(),
    );
    s.motion.as_mut().unwrap().vapor_coverage_history = Some(
        serde_json::from_value(json!({
            "schema":"ftx-motion-vapor-coverage/1","integration":"right_endpoint",
            "max_geometry_operations":1_000_000,
        }))
        .unwrap(),
    );
    s.max_vertex_frames = 2_000_000;
    let mut moving = Session::new(&s, &body).unwrap();
    moving.set_motion_path(Ok(path(&s, &body))).unwrap();
    let moved = moving.evaluate(&p, |_| {}).unwrap();
    assert_eq!(
        moved["motion"]["status"], "available",
        "{}",
        moved["motion"]
    );
    let motion = &moved["motion"]["result"];
    for endpoint in motion["keyframes"].as_array().unwrap() {
        let sample = endpoint["sample_index"].as_u64().unwrap() as usize;
        let vapor = &endpoint["vapor_surface"];
        assert_eq!(vapor["status"], "available");
        assert_eq!(
            vapor["result"]["positions_m"],
            motion["samples"][sample]["positions_m"]
        );
        assert_ne!(vapor["result"]["request_sha256"], direct["request_sha256"]);
        assert_eq!(vapor["result"]["model_sha256"], direct["model_sha256"]);
        let area = vapor["result"]["area_m2"].as_f64().unwrap();
        let heat = vapor["result"]["potential_heat_flow_w"].as_f64().unwrap();
        assert!((heat - area * 100.).abs() < 1e-12);
        let coverage = &endpoint["vapor_coverage"];
        assert_eq!(coverage["status"], "available", "{coverage}");
        assert_eq!(
            coverage["result"]["request"]["source"]["positions_m"],
            motion["samples"][sample]["positions_m"]
        );
        assert_eq!(
            coverage["result"]["request"]["map"]["body_positions_m"],
            motion["samples"][sample]["body_positions_m"]
        );
    }
    let history = &motion["vapor_coverage_history"];
    assert_eq!(history["status"], "available", "{history}");
    let history = &history["result"];
    assert_eq!(history["path_sha256"], motion["path_sha256"]);
    assert_eq!(
        history["initial_positions_sha256"],
        moved["positions_sha256"]
    );
    assert_eq!(
        history["samples"].as_array().unwrap().len(),
        motion["samples"].as_array().unwrap().len()
    );
    let mut integral = 0.0;
    let mut previous = 0.0;
    for (sample, row) in motion["samples"]
        .as_array()
        .unwrap()
        .iter()
        .zip(history["samples"].as_array().unwrap())
    {
        assert_eq!(row["time_s"], sample["time_s"]);
        assert_eq!(
            row["result"]["request"]["source"]["positions_m"],
            sample["positions_m"]
        );
        assert_eq!(
            row["result"]["request"]["map"]["body_positions_m"],
            sample["body_positions_m"]
        );
        let time = sample["time_s"].as_f64().unwrap();
        integral += (time - previous)
            * row["result"]["coverage"]["covered_potential_heat_flow_w"]
                .as_f64()
                .unwrap();
        previous = time;
    }
    assert_eq!(history["totals"]["potential_energy_j"], integral);
    let replay: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_str(history["replay_request_json"].as_str().unwrap()).unwrap();
    assert_eq!(replay.evaluate().unwrap(), *history);

    // A transport-budget refusal must preserve every accepted mechanical sample and the
    // independent instantaneous endpoint consumers; no partial integral may survive.
    s.motion
        .as_mut()
        .unwrap()
        .vapor_coverage_history
        .as_mut()
        .unwrap()
        .max_geometry_operations = 1;
    let mut limited = Session::new(&s, &body).unwrap();
    limited.set_motion_path(Ok(path(&s, &body))).unwrap();
    let refused = limited.evaluate(&p, |_| {}).unwrap();
    assert_eq!(refused["motion"]["status"], "available");
    assert_eq!(refused["motion"]["result"]["samples"], motion["samples"]);
    assert_eq!(
        refused["motion"]["result"]["vapor_coverage_history"]["status"],
        "refused"
    );
    assert!(refused["motion"]["result"]["vapor_coverage_history"]["result"].is_null());
    assert_eq!(
        refused["motion"]["result"]["keyframes"][0]["vapor_coverage"]["status"],
        "available"
    );
    s.motion
        .as_mut()
        .unwrap()
        .vapor_coverage_history
        .as_mut()
        .unwrap()
        .max_geometry_operations = 20_000_000;
    let mut aggregate = Session::new(&s, &body).unwrap();
    aggregate.set_motion_path(Ok(path(&s, &body))).unwrap();
    for _ in 0..2 {
        let missing = aggregate.evaluate(&pattern(), |_| {}).unwrap();
        assert_eq!(missing["motion"]["status"], "available");
        assert_eq!(
            missing["motion"]["result"]["vapor_coverage_history"]["status"],
            "refused"
        );
    }
    // Both failed source qualifications still consume their admitted whole-history reservation.
    let exhausted = aggregate.evaluate(&p, |_| {}).unwrap();
    assert_eq!(exhausted["motion"]["status"], "available");
    let refusal = &exhausted["motion"]["result"]["vapor_coverage_history"]["refusal"];
    assert!(
        refusal["message"]
            .as_str()
            .unwrap()
            .contains("reserved session"),
        "{refusal}"
    );
}

fn vapor_coverage_scenario() -> Scenario {
    let mut s = motion_scenario();
    s.vapor_coverage = Some(
        serde_json::from_value(json!({
            "body_triangle_ids":[0],"instance_layers":{"band":"shell"},
            "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                "separation_tolerance_m":0.00001,"interfaces":[]},
            "conditions":{"boundaries":[{
                "inside_vapor_pressure_pa":2000.,"outside_vapor_pressure_pa":1000.,
                "inside_film_resistance_m2_pa_w":1.,"outside_film_resistance_m2_pa_w":2.,
                "latent_heat_j_kg":2_500_000.}]}}))
        .unwrap(),
    );
    s
}

#[test]
fn moving_layered_vapor_uses_actual_endpoint_geometry_and_canonical_source_replay() {
    use ftx_drape::vapor::coverage::Request;

    let form = form();
    let body = body(&form);
    let s = vapor_coverage_scenario();
    let p = vapor_pattern();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    let initial = &result["vapor_coverage"]["result"];
    assert_eq!(result["vapor_coverage"]["status"], "available");
    assert_eq!(result["motion"]["status"], "available");
    let motion = &result["motion"]["result"];
    assert!(
        motion["vapor_basis"]
            .as_str()
            .unwrap()
            .contains("snapshots")
    );
    for endpoint in motion["keyframes"].as_array().unwrap() {
        let sample = &motion["samples"][endpoint["sample_index"].as_u64().unwrap() as usize];
        let vapor = &endpoint["vapor_coverage"];
        assert_eq!(vapor["status"], "available", "{vapor}");
        assert_eq!(endpoint["surface"]["status"], "refused");
        assert_eq!(endpoint["coverage"]["status"], "refused");
        let report = &vapor["result"];
        let frozen = &report["request"];
        assert_eq!(frozen["source"]["positions_m"], sample["positions_m"]);
        assert_eq!(
            frozen["map"]["body_positions_m"],
            sample["body_positions_m"]
        );
        assert_eq!(frozen["map"]["body_triangles"], json!([form.triangles[0]]));
        assert_ne!(frozen["source"]["positions_m"], result["positions_m"]);
        assert_ne!(frozen["map"]["body_positions_m"], json!(form.vertices));
        assert_ne!(report["request_sha256"], initial["request_sha256"]);
        assert_ne!(report["body_mesh_sha256"], initial["body_mesh_sha256"]);
        assert_eq!(
            frozen["source"]["materials"],
            initial["request"]["source"]["materials"]
        );
        assert_eq!(
            frozen["source"]["patches"],
            initial["request"]["source"]["patches"]
        );
        let replay: Request =
            serde_json::from_str(report["replay_request_json"].as_str().unwrap()).unwrap();
        replay
            .source
            .matches_material(&result["drape"]["material"])
            .unwrap();
        assert_eq!(replay.evaluate().unwrap(), *report);
        let mut wrong_protocol = replay.clone();
        wrong_protocol.conditions.required_method = Some("different source acquisition".into());
        assert!(wrong_protocol.evaluate().is_err());

        // Independent resistance/area answer, summed over disjoint BODY regions. The actual
        // curved cloth area, rather than reference or projected area, scales each source Ret.
        let expected: f64 = report["coverage"]["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|region| {
                let body_area = region["body_area_m2"].as_f64().unwrap();
                let cloth_area = region["layer_area_m2"][0].as_f64().unwrap();
                1000. * body_area / (1. + 7. * body_area / cloth_area + 2.)
            })
            .sum();
        assert!(expected > 0.0);
        let actual = report["coverage"]["covered_potential_heat_flow_w"]
            .as_f64()
            .unwrap();
        assert!((actual - expected).abs() < 1e-12);
        let mass = report["coverage"]["covered_potential_mass_flow_kg_s"]
            .as_f64()
            .unwrap();
        assert!((mass - expected / 2_500_000.).abs() < 1e-18);

        let mut stale = replay;
        stale.source.materials[0]
            .record
            .as_mut()
            .unwrap()
            .properties
            .get_mut("vapor_resistance")
            .unwrap()
            .value = 8.;
        assert!(stale.evaluate().is_err());
    }
    let substituted = session
        .evaluate(&changed(&p, "vapor_resistance", 17.), |_| {})
        .unwrap();
    assert_eq!(
        motion["samples"],
        substituted["motion"]["result"]["samples"]
    );
    for (a, b) in motion["keyframes"].as_array().unwrap().iter().zip(
        substituted["motion"]["result"]["keyframes"]
            .as_array()
            .unwrap(),
    ) {
        let a = &a["vapor_coverage"]["result"];
        let b = &b["vapor_coverage"]["result"];
        assert_ne!(a["source_sha256"], b["source_sha256"]);
        assert_eq!(a["projection"]["footprints"], b["projection"]["footprints"]);
        assert!(
            b["coverage"]["covered_potential_heat_flow_w"]
                .as_f64()
                .unwrap()
                < a["coverage"]["covered_potential_heat_flow_w"]
                    .as_f64()
                    .unwrap()
        );
    }
}

#[test]
fn moving_wet_film_tracks_material_dependent_depletion_without_changing_mechanics() {
    let form = form();
    let body = body(&form);
    let mut raw = vapor_pattern().raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["thermal_conductivity"] =
        pattern().raw()["fabric"]["material"]["record"]["properties"]["thermal_conductivity"]
            .clone();
    let p = Pattern::new(raw);
    let mut s = vapor_coverage_scenario();
    s.motion.as_mut().unwrap().vapor_coverage_history = Some(
        serde_json::from_value(json!({"schema":"ftx-motion-vapor-coverage/1",
            "integration":"right_endpoint","max_geometry_operations":1_000_000}))
        .unwrap(),
    );
    let mut baseline = Session::new(&s, &body).unwrap();
    baseline.set_motion_path(Ok(path(&s, &body))).unwrap();
    let reference = baseline.evaluate(&p, |_| {}).unwrap();
    let motion = &reference["motion"]["result"];
    let potential = &motion["vapor_coverage_history"]["result"];
    assert_eq!(motion["vapor_coverage_history"]["status"], "available");

    // Calculate the higher-Ret alternative independently on the retained sewn geometry.
    // Choose one finite film between the two demands: the lower resistance empties it,
    // while the higher resistance leaves some water. There is no reset at keyframes.
    let mut previous = 0.0;
    let mut slower_demand = 0.0;
    for row in potential["samples"].as_array().unwrap() {
        let time = row["time_s"].as_f64().unwrap();
        let rate: f64 = row["result"]["coverage"]["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let body_area = r["body_area_m2"].as_f64().unwrap();
                let cloth_area = r["layer_area_m2"][0].as_f64().unwrap();
                1000.0 * body_area / (1.0 + 17.0 * body_area / cloth_area + 2.0) / 2_500_000.0
            })
            .sum();
        slower_demand += rate * (time - previous);
        previous = time;
    }
    let faster_demand = potential["totals"]["potential_mass_kg"].as_f64().unwrap();
    assert!(slower_demand > 0.0 && slower_demand < faster_demand);
    let initial_water = 0.5 * (slower_demand + faster_demand);
    let mut policy = serde_json::to_value(
        s.motion
            .as_ref()
            .unwrap()
            .vapor_coverage_history
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    policy["wet_film"] = json!({"schema":"ftx-body-wet-film/1",
        "body_mesh_sha256":reference["vapor_coverage"]["result"]["body_mesh_sha256"],
        "source":"test:sewn-body-wet-film","basis":"Synthetic finite film on the selected body triangle; no wearer data",
        "initial_liquid_mass_kg":[initial_water],"supply_rate_kg_s":[0.0]});
    s.motion.as_mut().unwrap().vapor_coverage_history =
        Some(serde_json::from_value(policy.clone()).unwrap());
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let faster = session.evaluate(&p, |_| {}).unwrap();
    let slower = session
        .evaluate(&changed(&p, "vapor_resistance", 17.0), |_| {})
        .unwrap();
    for result in [&faster, &slower] {
        let current_motion = &result["motion"]["result"];
        assert_eq!(result["motion"]["status"], "available");
        assert_eq!(current_motion["samples"], motion["samples"]);
        assert_eq!(
            current_motion["vapor_coverage_history"]["status"],
            "available"
        );
        assert_eq!(
            result["metrics"]["cloth_mass_kg"],
            reference["metrics"]["cloth_mass_kg"]
        );
        assert_eq!(
            result["metrics"]["body_coverage_power_w"],
            reference["metrics"]["body_coverage_power_w"]
        );
        assert_eq!(
            result["metrics"]["independent_surface_power_w"],
            reference["metrics"]["independent_surface_power_w"]
        );
        let history = &current_motion["vapor_coverage_history"]["result"];
        assert_eq!(
            history["wet_film"]["body_mesh_sha256"],
            policy["wet_film"]["body_mesh_sha256"]
        );
        assert_eq!(history["wet_film"]["body"].as_array().unwrap().len(), 1);
        assert_eq!(history["wet_film"]["textile_temperature_coupled"], false);
        let mut remaining = initial_water;
        let mut evaporated = 0.0;
        for row in history["samples"].as_array().unwrap() {
            let water = &row["wet_film"]["body"][0];
            assert_eq!(water["body_triangle"], 0);
            assert_eq!(water["starting_liquid_mass_kg"], remaining);
            assert_eq!(water["supplied_mass_kg"], 0.0);
            evaporated += water["evaporated_mass_kg"].as_f64().unwrap();
            remaining = water["remaining_liquid_mass_kg"].as_f64().unwrap();
            assert!(water["mass_balance_error_kg"].as_f64().unwrap().abs() < 1e-18);
        }
        let water = &history["wet_film"]["totals"];
        assert!((evaporated + remaining - initial_water).abs() < 1e-18);
        assert!((water["evaporated_mass_kg"].as_f64().unwrap() - evaporated).abs() < 1e-18);
        assert!(
            (water["body_reservoir_latent_energy_j"].as_f64().unwrap() - evaporated * 2_500_000.0)
                .abs()
                < 1e-12
        );
        assert_eq!(water["remaining_liquid_mass_kg"], remaining);
        let replay: ftx_drape::vapor::coverage::history::Request =
            serde_json::from_str(history["replay_request_json"].as_str().unwrap()).unwrap();
        assert_eq!(replay.evaluate().unwrap(), *history);
    }
    let faster_water =
        &faster["motion"]["result"]["vapor_coverage_history"]["result"]["wet_film"]["totals"];
    let slower_water =
        &slower["motion"]["result"]["vapor_coverage_history"]["result"]["wet_film"]["totals"];
    assert!((faster_water["evaporated_mass_kg"].as_f64().unwrap() - initial_water).abs() < 1e-18);
    assert_eq!(faster_water["remaining_liquid_mass_kg"], 0.0);
    assert!((slower_water["evaporated_mass_kg"].as_f64().unwrap() - slower_demand).abs() < 1e-18);
    assert!(slower_water["remaining_liquid_mass_kg"].as_f64().unwrap() > 0.0);

    // Water is tied to the initial selected body topology. Bad water inputs refuse that
    // history alone; instantaneous vapor, dry heat, and all accepted mechanics survive.
    for (field, value) in [
        ("body_mesh_sha256", json!("0".repeat(64))),
        (
            "initial_liquid_mass_kg",
            json!([initial_water, initial_water]),
        ),
    ] {
        let mut bad_policy = policy.clone();
        bad_policy["wet_film"][field] = value;
        let mut invalid = s.clone();
        invalid.motion.as_mut().unwrap().vapor_coverage_history =
            Some(serde_json::from_value(bad_policy).unwrap());
        let mut rejected = Session::new(&invalid, &body).unwrap();
        rejected.set_motion_path(Ok(path(&invalid, &body))).unwrap();
        let result = rejected.evaluate(&p, |_| {}).unwrap();
        assert_eq!(result["motion"]["status"], "available");
        assert_eq!(result["motion"]["result"]["samples"], motion["samples"]);
        assert_eq!(
            result["motion"]["result"]["vapor_coverage_history"]["status"],
            "refused"
        );
        assert!(result["motion"]["result"]["vapor_coverage_history"]["result"].is_null());
        for endpoint in result["motion"]["result"]["keyframes"].as_array().unwrap() {
            assert_eq!(endpoint["vapor_coverage"]["status"], "available");
            assert_eq!(endpoint["surface"]["status"], "available");
            assert_eq!(endpoint["coverage"]["status"], "available");
        }
    }
}

#[test]
fn moving_layered_vapor_refusals_preserve_motion_and_reserve_retained_geometry() {
    let form = form();
    let body = body(&form);
    let p = pattern(); // Mechanics and dry heat are available; Ret is deliberately missing.
    let ordinary = motion_scenario();
    let mut baseline = Session::new(&ordinary, &body).unwrap();
    baseline
        .set_motion_path(Ok(path(&ordinary, &body)))
        .unwrap();
    let reference = baseline.evaluate(&p, |_| {}).unwrap();
    let s = vapor_coverage_scenario();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let missing = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(missing["vapor_coverage"]["status"], "refused");
    assert_eq!(missing["motion"]["status"], "available");
    let motion = &missing["motion"]["result"];
    assert_eq!(motion["samples"], reference["motion"]["result"]["samples"]);
    assert!(
        motion["reserved_retained_vertices"].as_u64().unwrap()
            > reference["motion"]["result"]["reserved_retained_vertices"]
                .as_u64()
                .unwrap()
    );
    for endpoint in motion["keyframes"].as_array().unwrap() {
        assert_eq!(endpoint["vapor_coverage"]["status"], "refused");
        assert!(endpoint["vapor_coverage"]["result"].is_null());
        assert_eq!(endpoint["surface"]["status"], "available");
        assert_eq!(endpoint["coverage"]["status"], "available");
    }

    let available = session.evaluate(&vapor_pattern(), |_| {}).unwrap();
    let mut locked = s.clone();
    locked
        .vapor_coverage
        .as_mut()
        .unwrap()
        .conditions
        .body_mesh_sha256 = Some(
        available["vapor_coverage"]["result"]["body_mesh_sha256"]
            .as_str()
            .unwrap()
            .into(),
    );
    let mut bound = Session::new(&locked, &body).unwrap();
    bound.set_motion_path(Ok(path(&locked, &body))).unwrap();
    let r = bound.evaluate(&vapor_pattern(), |_| {}).unwrap();
    assert_eq!(r["vapor_coverage"]["status"], "available");
    assert_eq!(r["motion"]["status"], "available");
    for endpoint in r["motion"]["result"]["keyframes"].as_array().unwrap() {
        let coverage = &endpoint["vapor_coverage"];
        assert_eq!(coverage["status"], "available");
        assert_ne!(
            coverage["result"]["request"]["conditions"]["body_mesh_sha256"],
            r["vapor_coverage"]["result"]["body_mesh_sha256"]
        );
        assert_eq!(
            coverage["result"]["request"]["conditions"]["body_mesh_sha256"],
            coverage["result"]["body_mesh_sha256"]
        );
    }
    // Moving fields can follow verified body-triangle identities, but an initially stale
    // boundary cannot acquire a valid identity merely because a motion was requested.
    locked
        .vapor_coverage
        .as_mut()
        .unwrap()
        .conditions
        .body_mesh_sha256 = Some("0".repeat(64));
    let mut stale = Session::new(&locked, &body).unwrap();
    stale.set_motion_path(Ok(path(&locked, &body))).unwrap();
    let refused = stale.evaluate(&vapor_pattern(), |_| {}).unwrap();
    assert_eq!(refused["vapor_coverage"]["status"], "refused");
    assert_eq!(refused["motion"]["status"], "available");
    assert_eq!(
        refused["motion"]["result"]["samples"],
        r["motion"]["result"]["samples"]
    );
    for endpoint in refused["motion"]["result"]["keyframes"].as_array().unwrap() {
        assert_eq!(endpoint["vapor_coverage"]["status"], "refused");
        assert!(
            endpoint["vapor_coverage"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("body_mesh_sha256")
        );
    }
}

#[test]
fn composed_evaluation_equals_real_drape_surface_and_projected_coverage() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let s = scenario();
    let ordinary = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["drape"], ordinary.report);
    assert_eq!(r["positions_m"], json!(ordinary.scene.solver.x));
    assert_eq!(r["surface"]["status"], "available", "{}", r["surface"]);
    assert_eq!(
        r["surface"]["result"],
        s.surface
            .as_ref()
            .unwrap()
            .evaluate(&ordinary.report)
            .unwrap()
    );
    let c = s.coverage.as_ref().unwrap();
    let direct = coverage::Request {
        schema: "ftx-steady-coverage-request/1".into(),
        report: ordinary.report,
        map: coverage::Map {
            body_positions_m: form.vertices,
            body_triangles: vec![form.triangles[0]],
            instance_layers: c.instance_layers.clone(),
            settings: c.settings.clone(),
        },
        conditions: c.conditions.clone(),
    }
    .evaluate()
    .unwrap();
    assert_eq!(r["coverage"]["status"], "available", "{}", r["coverage"]);
    assert_eq!(r["coverage"]["result"], direct);
    assert!(r["metrics"]["cloth_mass_kg"].as_f64().unwrap() > 0.);
    assert!(!r["physical_validation_claim"].as_bool().unwrap());
}

#[test]
fn substitutions_recompute_heat_and_mechanics_without_warm_starting() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let original = p.raw().clone();
    let s = scenario();
    let mut session = Session::new(&s, &body).unwrap();
    let a = session.evaluate(&p, |_| {}).unwrap();
    let b = session
        .evaluate(&changed(&p, "thermal_conductivity", 0.1), |_| {})
        .unwrap();
    assert_eq!(a["positions_m"], b["positions_m"]);
    assert_eq!(a["body_geometry_sha256"], b["body_geometry_sha256"]);
    assert_eq!(a["placements_sha256"], b["placements_sha256"]);
    assert_ne!(
        a["surface"]["result"]["model_sha256"],
        b["surface"]["result"]["model_sha256"]
    );
    assert_ne!(
        a["metrics"]["independent_surface_power_w"],
        b["metrics"]["independent_surface_power_w"]
    );
    assert_ne!(
        a["metrics"]["body_coverage_power_w"],
        b["metrics"]["body_coverage_power_w"]
    );
    let mechanical = session
        .evaluate(&changed(&p, "areal_density", 0.4), |_| {})
        .unwrap();
    assert_ne!(
        a["metrics"]["cloth_mass_kg"],
        mechanical["metrics"]["cloth_mass_kg"]
    );
    assert_ne!(a["positions_sha256"], mechanical["positions_sha256"]);
    assert_eq!(a, session.evaluate(&p, |_| {}).unwrap());
    assert_eq!(p.raw(), &original);
}

#[test]
fn missing_thermal_and_stale_fields_do_not_erase_valid_draping() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    let mut raw = p.raw().clone();
    raw["fabric"]["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("thermal_conductivity");
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&Pattern::new(raw), |_| {})
        .unwrap();
    assert_eq!(r["surface"]["status"], "refused");
    assert_eq!(r["coverage"]["status"], "refused");
    assert!(r["metrics"]["independent_surface_power_w"].is_null());
    assert!(r["metrics"]["cloth_mass_kg"].is_number());
    s.surface.as_mut().unwrap().model_sha256 = Some("0".repeat(64));
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["surface"]["status"], "refused");
    assert_eq!(r["coverage"]["status"], "available");
    s.coverage.as_mut().unwrap().body_triangle_ids = vec![0, 0];
    s.surface = None;
    let r = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(r["coverage"]["status"], "refused");
    assert_eq!(r["surface"]["status"], "not_requested");
}

#[test]
fn budgets_and_identity_are_checked_before_a_solver_starts() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = scenario();
    let original = Session::new(&s, &body).unwrap().descriptor();
    s.options.step *= 2.;
    assert_ne!(original, Session::new(&s, &body).unwrap().descriptor());
    let shifted: Vec<_> = form
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0.001, 0., 0.))
        .collect();
    let moved = DrapeBody {
        collision: (&shifted, &form.triangles),
        ..body
    };
    assert_ne!(
        original["body_geometry_sha256"],
        Session::new(&s, &moved).unwrap().descriptor()["body_geometry_sha256"]
    );
    s.max_mesh_points = 3;
    let mut called = false;
    assert_eq!(
        Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| called = true)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(!called);
    s.max_mesh_points = 4096;
    s.max_vertex_frames = 1;
    assert_eq!(
        Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| called = true)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(!called);
    s.options.lag_rounds = [usize::MAX, 0];
    assert!(s.validate().is_err());
    let mut raw = serde_json::to_value(scenario()).unwrap();
    raw["options"]["typo"] = json!(1);
    assert!(serde_json::from_value::<Scenario>(raw).is_err());
    assert!(
        serde_json::from_value::<DrapeOptions>(json!({"contact":{"dhat":0.1,"typo":1}})).is_err()
    );
}

fn motion_scenario() -> Scenario {
    let mut s = scenario();
    s.max_vertex_frames = 2_000_000;
    s.motion = Some(
        serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
        "keyframes":[{"time_s":0.002,"arm_abduction_deg":35.0},
            {"time_s":0.004,"arm_abduction_deg":36.0}],
        "max_step_s":0.001,"max_trials":16,"max_pairs":100000,"max_sample_vertices":10000}))
        .unwrap(),
    );
    s
}
fn path(s: &Scenario, body: &DrapeBody<'_>) -> ftx_drape::study::motion::Path {
    // Explicit caller-owned cylinder translations, not an anatomical arm-angle fixture.
    let endpoints = [0.0001, 0.0002]
        .into_iter()
        .map(|z| {
            body.collision
                .0
                .iter()
                .map(|p| *p + Vec3::new(0.0, 0.0, z))
                .collect()
        })
        .collect();
    ftx_drape::study::motion::Path::new(s.motion.as_ref().unwrap(), body.collision, endpoints)
        .unwrap()
}

#[test]
fn motion_continues_the_same_sewn_solver_and_recomputes_endpoint_heat() {
    use ftx_drape::motion::{MotionOptions, advance_body};
    use ftx_drape::start::{edge_strain, face_edges};
    let form = form();
    let body = body(&form);
    let p = pattern();
    let s = motion_scenario();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let result = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(
        result["motion"]["status"], "available",
        "{}",
        result["motion"]
    );
    let motion = &result["motion"]["result"];
    let mut direct = drape(&p, &s.fit_spec, &body, &s.options, |_| {}).unwrap();
    assert_eq!(result["drape"], direct.report); // static report is still the INITIAL drape
    assert_eq!(result["positions_m"], json!(direct.scene.solver.x));
    assert_eq!(motion["initial_sample"]["time_s"], 0.0);
    assert_eq!(
        motion["initial_sample"]["positions_m"],
        result["positions_m"]
    );
    assert_eq!(
        motion["initial_sample"]["body_positions_m"],
        json!(form.vertices)
    );
    assert_eq!(
        motion["initial_membrane_history"],
        json!(direct.scene.solver.relaxation_report().unwrap())
    );
    assert_eq!(
        motion["initial_bending_history"],
        json!(direct.scene.solver.bending_plasticity_report().unwrap())
    );
    let original = direct.scene.solver.x.clone();
    let mut samples = Vec::new();
    let edges = face_edges(&direct.scene.mesh.faces);
    let mut peak = edge_strain(
        &direct.scene.positions(),
        &direct.scene.mesh.verts2d,
        &edges,
    )
    .into_iter()
    .fold(0.0_f64, |a, b| a.max(100.0 * b.abs()));
    let mut trials = 16;
    let mut vertices = 10000 - form.vertices.len() - direct.scene.solver.x.len();
    for (i, z) in [0.0001, 0.0002].into_iter().enumerate() {
        let target: Vec<_> = form
            .vertices
            .iter()
            .map(|p| *p + Vec3::new(0.0, 0.0, z))
            .collect();
        let r = advance_body(
            &mut direct.scene.solver,
            &target,
            0.002,
            MotionOptions {
                max_step_s: 0.001,
                max_trials: trials,
                max_pairs: 100000,
                max_sample_vertices: vertices,
            },
        )
        .unwrap();
        trials -= r.trials;
        vertices -= r.samples.len() * (form.vertices.len() + direct.scene.solver.x.len());
        for sample in r.samples {
            let expanded: Vec<_> = direct.scene.weld.iter().map(|&w| sample.cloth[w]).collect();
            peak = edge_strain(&expanded, &direct.scene.mesh.verts2d, &edges)
                .into_iter()
                .fold(peak, |a, b| a.max(100.0 * b.abs()));
            samples.push(json!({"time_s":i as f64*0.002+sample.time_s,
                "positions_m":sample.cloth,"body_positions_m":sample.body}));
        }
        let endpoint = &motion["keyframes"][i];
        assert_eq!(endpoint["surface"]["status"], "available");
        let actual = &endpoint["surface"]["result"];
        let boundary = ftx_drape::thermal::Boundary {
            inside_c: 30.0,
            outside_c: 10.0,
            inside_resistance: 0.1,
            outside_resistance: 0.2,
        };
        let solved = direct
            .steady_thermal_surface()
            .unwrap()
            .solve(&direct.scene.solver.x, &[boundary])
            .unwrap();
        assert_eq!(actual["steady"], serde_json::to_value(solved).unwrap());
        assert_eq!(actual["positions_m"], json!(direct.scene.solver.x));
        let c = &endpoint["coverage"];
        assert_eq!(c["status"], "available", "{c}");
        assert_eq!(
            c["result"]["request"]["map"]["body_positions_m"],
            json!(target)
        );
    }
    assert_eq!(samples.len(), motion["samples"].as_array().unwrap().len());
    for (expected, actual) in samples.iter().zip(motion["samples"].as_array().unwrap()) {
        for field in ["time_s", "positions_m", "body_positions_m"] {
            assert_eq!(expected[field], actual[field]);
        }
        assert_eq!(actual["solve"]["converged"], true);
    }
    assert_eq!(motion["final_velocities_m_s"], json!(direct.scene.solver.v));
    assert_eq!(
        motion["final_positions_sha256"],
        motion["keyframes"][1]["positions_sha256"]
    );
    assert_eq!(
        motion["final_body_collision_sha256"],
        motion["keyframes"][1]["body_collision_sha256"]
    );
    assert_eq!(
        motion["retained_vertex_samples"],
        json!((samples.len() + 1) * (form.vertices.len() + direct.scene.solver.x.len()))
    );
    assert_eq!(
        motion["metrics"]["max_absolute_edge_strain_pct"],
        json!(peak)
    );
    assert_ne!(
        json!(original),
        motion["samples"].as_array().unwrap().last().unwrap()["positions_m"]
    );
    assert_eq!(
        motion["path_sha256"],
        session.descriptor()["motion_path"]["sha256"]
    );
    assert_eq!(result, session.evaluate(&p, |_| {}).unwrap());
}

#[test]
fn motion_limits_do_not_preempt_later_independent_static_drapes() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    let mut reference = Session::new(&s, &body).unwrap();
    reference.set_motion_path(Ok(path(&s, &body))).unwrap();
    let expected = reference.evaluate(&p, |_| {}).unwrap();
    assert_eq!(expected["motion"]["status"], "available");
    let work = expected["motion"]["result"]["reserved_vertex_frames"]
        .as_u64()
        .unwrap() as usize;
    let static_work = expected["charged_vertex_frames"].as_u64().unwrap() as usize;
    s.max_vertex_frames = work.max(3 * static_work);
    s.max_motion_trials = s.motion.as_ref().unwrap().max_trials;
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    for i in 0..3 {
        let actual = session.evaluate(&p, |_| {}).unwrap();
        for field in [
            "drape",
            "surface",
            "coverage",
            "positions_m",
            "triangles",
            "metrics",
            "charged_vertex_frames",
        ] {
            assert_eq!(actual[field], expected[field], "alternative {i}, {field}");
        }
        assert_eq!(
            actual["motion"]["status"],
            if i == 0 { "available" } else { "refused" }
        );
        if i > 0 {
            assert!(actual["motion"]["result"].is_null());
        }
    }
    // Omitted/default trial limits preserve canonical identities for pre-existing requests.
    let original = motion_scenario();
    let mut explicit = serde_json::to_value(&original).unwrap();
    assert!(explicit.get("max_motion_trials").is_none());
    explicit["max_motion_trials"] = json!(4096);
    let explicit: Scenario = serde_json::from_value(explicit).unwrap();
    assert_eq!(
        Session::new(&original, &body).unwrap().descriptor(),
        Session::new(&explicit, &body).unwrap().descriptor()
    );
    let mut invalid = original.clone();
    invalid.max_motion_trials = 0;
    assert!(invalid.validate().is_err());
    let mut default_sample = serde_json::to_value(&original).unwrap();
    default_sample["motion"]
        .as_object_mut()
        .unwrap()
        .remove("max_sample_vertices");
    let default_sample: Scenario = serde_json::from_value(default_sample).unwrap();
    assert_eq!(default_sample.motion.unwrap().max_sample_vertices, 500_000);
}

#[test]
fn motion_substitutions_keep_history_within_a_variant_not_between_variants() {
    let form = form();
    let body = body(&form);
    let s = motion_scenario();
    let p = pattern();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let a = session.evaluate(&p, |_| {}).unwrap();
    let thermal = session
        .evaluate(&changed(&p, "thermal_conductivity", 0.1), |_| {})
        .unwrap();
    assert_eq!(a["motion"]["status"], "available");
    assert_eq!(
        a["motion"]["result"]["samples"],
        thermal["motion"]["result"]["samples"]
    );
    assert_ne!(
        a["motion"]["result"]["keyframes"][0]["surface"]["result"]["steady"]["power_w"],
        thermal["motion"]["result"]["keyframes"][0]["surface"]["result"]["steady"]["power_w"]
    );
    let heavy = session
        .evaluate(&changed(&p, "areal_density", 0.4), |_| {})
        .unwrap();
    assert_eq!(
        heavy["motion"]["status"], "available",
        "{}",
        heavy["motion"]
    );
    assert_ne!(
        a["motion"]["result"]["samples"],
        heavy["motion"]["result"]["samples"]
    );
    assert_eq!(a, session.evaluate(&p, |_| {}).unwrap());
}

#[test]
fn failed_or_unprepared_motion_retains_static_results_and_never_publishes_a_partial_path() {
    let form = form();
    let body = body(&form);
    let p = pattern();
    let mut s = motion_scenario();
    let initial = Session::new(&s, &body)
        .unwrap()
        .evaluate(&p, |_| {})
        .unwrap();
    assert_eq!(initial["motion"]["status"], "refused");
    assert_eq!(initial["surface"]["status"], "available");
    let count = form.vertices.len() + initial["positions_m"].as_array().unwrap().len();
    s.motion.as_mut().unwrap().max_sample_vertices = 3 * count;
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&s, &body))).unwrap();
    let r = session.evaluate(&p, |_| {}).unwrap();
    assert_eq!(r["motion"]["status"], "refused");
    assert!(r["motion"]["result"].is_null());
    assert_eq!(r["drape"], initial["drape"]);
    assert_eq!(r["positions_m"], initial["positions_m"]);
    assert_eq!(r["surface"], initial["surface"]);
    assert_eq!(r["coverage"], initial["coverage"]);
    assert!(session.set_motion_path(Ok(path(&s, &body))).is_err());
    s.max_vertex_frames = 10;
    let mut progress_called = false;
    assert_eq!(
        Session::new(&s, &body)
            .unwrap()
            .evaluate(&p, |_| progress_called = true)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
    assert!(!progress_called);
}

#[test]
fn motion_rejects_stale_body_policy_and_invalid_time_before_numerical_work() {
    let form = form();
    let body = body(&form);
    let s = motion_scenario();
    let mut other = motion_scenario();
    other.motion.as_mut().unwrap().keyframes[0].arm_abduction_deg = 40.0;
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path(&other, &body))).unwrap();
    assert!(session.descriptor()["motion_path"]["refusal"].is_object());
    for mode in 0..5 {
        let mut m = s.motion.clone().unwrap();
        match mode {
            0 => m.keyframes.clear(),
            1 => m.keyframes[1].time_s = 0.001,
            2 => m.keyframes[0].arm_abduction_deg = f64::NAN,
            3 => m.max_trials = 1,
            _ => m.max_sample_vertices = usize::MAX,
        }
        assert!(m.validate().is_err());
    }
    let mut other_body = form.vertices.clone();
    other_body[0].z += 0.001;
    let path = ftx_drape::study::motion::Path::new(
        s.motion.as_ref().unwrap(),
        (&other_body, &form.triangles),
        vec![other_body.clone(), other_body.clone()],
    )
    .unwrap();
    let mut session = Session::new(&s, &body).unwrap();
    session.set_motion_path(Ok(path)).unwrap();
    assert!(session.descriptor()["motion_path"]["refusal"].is_object());
}

fn heat_pattern() -> Pattern {
    let mut raw = pattern().raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["specific_heat"] = json!({
        "value":1000.,"unit":"J/(kg*K)","source":"test","evidence":"estimate",
        "method":"Synthetic software gate; not measured textile data"});
    Pattern::new(raw)
}

fn heat_scenario() -> Scenario {
    let mut s = motion_scenario();
    s.motion.as_mut().unwrap().transient_surface = Some(serde_json::from_value(json!({
        "schema":"ftx-motion-surface-heat/1","initial_temperature_c":15.,
        "cells_per_layer":1,"max_step_s":0.0005,"max_cell_steps":100000,
        "boundaries":[
            {"inside_c":40.,"outside_c":5.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2},
            {"inside_c":5.,"outside_c":40.,"inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}
        ]})).unwrap());
    s
}

fn moving(s: &Scenario, p: &Pattern) -> Value {
    let f = form();
    let b = body(&f);
    let mut session = Session::new(s, &b).unwrap();
    session.set_motion_path(Ok(path(s, &b))).unwrap();
    session.evaluate(p, |_| {}).unwrap()
}

#[test]
fn transient_motion_uses_every_actual_sample_without_temperature_restart() {
    use ftx_drape::thermal::{Boundary, surface::Surface};
    let s = heat_scenario();
    let p = heat_pattern();
    let result = moving(&s, &p);
    let motion = &result["motion"]["result"];
    let heat = &motion["transient_surface"];
    assert_eq!(heat["status"], "available", "{heat}");
    let h = &heat["result"];
    let model = Surface::from_snapshot(&h["model"]).unwrap();
    let mut state = model.initial(15.).unwrap();
    let mut time = 0.;
    let mut stored = 0.;
    let mut net = 0.;
    let samples = motion["samples"].as_array().unwrap();
    assert_eq!(h["trace"].as_array().unwrap().len(), samples.len());
    for (i, sample) in samples.iter().enumerate() {
        let t = sample["time_s"].as_f64().unwrap();
        let row = &h["trace"][i];
        assert_eq!(row["sample_index"], i);
        assert_eq!(row["time_s"], t);
        let interval = row["interval_index"].as_u64().unwrap() as usize;
        let film = s
            .motion
            .as_ref()
            .unwrap()
            .transient_surface
            .as_ref()
            .unwrap()
            .boundaries[interval];
        let boundary = Boundary {
            inside_c: film.inside_c,
            outside_c: film.outside_c,
            inside_resistance: film.inside_resistance_m2_k_w,
            outside_resistance: film.outside_resistance_m2_k_w,
        };
        let x: Vec<Vec3> = serde_json::from_value(sample["positions_m"].clone()).unwrap();
        let steps = row["thermal_substeps"].as_u64().unwrap();
        let dt = (t - time) / steps as f64;
        for _ in 0..steps {
            let next = model
                .advance(&state, &x, dt, &vec![boundary; model.column_count()])
                .unwrap();
            stored += next.stored_change_j;
            net += dt * (next.inside_heat_w - next.outside_heat_w);
            state = next.state;
        }
        assert_eq!(row["stored_change_j"], stored);
        assert_eq!(row["net_boundary_energy_j"], net);
        time = t;
    }
    assert_eq!(h["final"]["temperature_c"], json!(state.temperature_c()));
    assert_eq!(h["duration_s"], motion["duration_s"]);
    assert_eq!(h["path_sha256"], motion["path_sha256"]);
    assert_eq!(h["initial_positions_sha256"], result["positions_sha256"]);
    assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-9);
    // Independent one-cell reference-mass accounting: no current-area heat capacity.
    let expected = h["model"]["patches"]
        .as_array()
        .unwrap()
        .iter()
        .zip(state.temperature_c())
        .map(|(patch, temp)| {
            patch["reference_area_m2"].as_f64().unwrap() * 0.2 * 1000. * (temp[0] - 15.)
        })
        .sum::<f64>();
    assert!((stored - expected).abs() < 1e-10);
    let mut no_heat = s.clone();
    no_heat.motion.as_mut().unwrap().transient_surface = None;
    let ordinary = moving(&no_heat, &p);
    assert_eq!(ordinary["drape"], result["drape"]);
    assert_eq!(ordinary["motion"]["result"]["samples"], motion["samples"]);
    assert_eq!(
        ordinary["motion"]["result"]["keyframes"],
        motion["keyframes"]
    );
}

#[test]
fn transient_material_edits_change_temperatures_not_mechanics_and_repeat_cleanly() {
    let s = heat_scenario();
    let p = heat_pattern();
    let a = moving(&s, &p);
    for (key, value) in [("specific_heat", 2000.), ("thermal_conductivity", 0.1)] {
        let b = moving(&s, &changed(&p, key, value));
        let am = &a["motion"]["result"];
        let bm = &b["motion"]["result"];
        assert_eq!(bm["transient_surface"]["status"], "available");
        assert_eq!(am["samples"], bm["samples"]);
        assert_ne!(
            am["transient_surface"]["result"]["model"]["sha256"],
            bm["transient_surface"]["result"]["model"]["sha256"]
        );
        assert_ne!(
            am["transient_surface"]["result"]["final"]["temperature_c"],
            bm["transient_surface"]["result"]["final"]["temperature_c"]
        );
    }
    assert_eq!(moving(&s, &p), a);
}

#[test]
fn transient_missing_storage_and_late_budget_failure_preserve_complete_motion() {
    let mut s = heat_scenario();
    let without_capacity = moving(&s, &pattern());
    assert_eq!(without_capacity["surface"]["status"], "available");
    assert_eq!(without_capacity["motion"]["status"], "available");
    assert_eq!(
        without_capacity["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
    let p = heat_pattern();
    let good = moving(&s, &p);
    let cells = good["triangles"].as_array().unwrap().len();
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .max_cell_steps = cells * 2;
    let late = moving(&s, &p);
    assert_eq!(late["motion"]["status"], "available");
    assert_eq!(
        late["motion"]["result"]["samples"],
        good["motion"]["result"]["samples"]
    );
    assert_eq!(late["drape"], good["drape"]);
    let heat = &late["motion"]["result"]["transient_surface"];
    assert_eq!(heat["status"], "refused", "{heat}");
    assert!(heat["result"].is_null());
    assert_eq!(heat["refusal"]["code"], "budget_exceeded");
    s.motion
        .as_mut()
        .unwrap()
        .transient_surface
        .as_mut()
        .unwrap()
        .boundaries
        .pop();
    assert_eq!(
        moving(&s, &p)["motion"]["result"]["transient_surface"]["status"],
        "refused"
    );
}

fn coverage_heat_scenario() -> Scenario {
    let mut s = heat_scenario();
    let c = s.coverage.as_ref().unwrap();
    let request = json!({"schema":"ftx-motion-coverage-heat/1",
        "body_triangle_ids":c.body_triangle_ids,"instance_layers":c.instance_layers,
        "settings":c.settings,"initial_temperature_c":15.,
        "boundaries":s.motion.as_ref().unwrap().transient_surface.as_ref().unwrap().boundaries,
        "exposed_boundaries":[null,null],"max_step_s":0.0005,
        "max_cell_steps":100000,"max_geometry_operations":2000000});
    s.motion.as_mut().unwrap().transient_coverage = Some(serde_json::from_value(request).unwrap());
    s
}

#[test]
fn moving_body_coverage_heat_matches_direct_conservative_remap_and_physical_steps() {
    use ftx_drape::thermal::{
        Boundary,
        surface::{Surface, coverage::motion::Frame, projection},
    };
    let s = coverage_heat_scenario();
    let result = moving(&s, &heat_pattern());
    let motion = &result["motion"]["result"];
    let outcome = &motion["transient_coverage"];
    assert_eq!(outcome["status"], "available", "{outcome:#}");
    let h = &outcome["result"];
    let saved = &h["checkpoint"]["frame"];
    let source = Surface::from_snapshot(&saved["source"]).unwrap();
    let projection: projection::Request = serde_json::from_value(saved["request"].clone()).unwrap();
    let triangles: Vec<[usize; 3]> =
        serde_json::from_value(saved["projection"]["layout"]["body_triangles"].clone()).unwrap();
    let mut frame = Frame::new(
        source.clone(),
        serde_json::from_value(result["positions_m"].clone()).unwrap(),
        form().vertices,
        &triangles,
        projection.clone(),
    )
    .unwrap();
    assert_eq!(h["initial_frame_sha256"], frame.sha256());
    let mut state = frame.initial(15.).unwrap();
    let (mut time, mut net, mut stored, mut remap_sum) = (0., 0., 0., 0.);
    let policy = s
        .motion
        .as_ref()
        .unwrap()
        .transient_coverage
        .as_ref()
        .unwrap();
    let trace = h["trace"].as_array().unwrap();
    assert_eq!(trace.len(), motion["samples"].as_array().unwrap().len());
    assert!(
        trace
            .iter()
            .any(|v| v["covered_area_m2"].as_f64().unwrap() > 0.)
    );
    for (sample, t) in motion["samples"].as_array().unwrap().iter().zip(trace) {
        let next = Frame::new(
            source.clone(),
            serde_json::from_value(sample["positions_m"].clone()).unwrap(),
            serde_json::from_value(sample["body_positions_m"].clone()).unwrap(),
            &triangles,
            projection.clone(),
        )
        .unwrap();
        let transfer = next.remap(&frame, &state).unwrap();
        assert_eq!(t["remap"], json!(transfer.balance));
        remap_sum += transfer.balance.residual_j;
        state = transfer.state;
        frame = next;
        let b = policy.boundaries[t["interval_index"].as_u64().unwrap() as usize];
        let boundaries = vec![
            Boundary {
                inside_c: b.inside_c,
                outside_c: b.outside_c,
                inside_resistance: b.inside_resistance_m2_k_w,
                outside_resistance: b.outside_resistance_m2_k_w
            };
            triangles.len()
        ];
        let end = sample["time_s"].as_f64().unwrap();
        let n = t["thermal_substeps"].as_u64().unwrap();
        let dt = (end - time) / n as f64;
        for _ in 0..n {
            let step = frame.advance(&state, dt, &boundaries).unwrap();
            net += dt * step.net_heat_w();
            stored += step.stored_change_j();
            state = step.into_state();
        }
        time = end;
        assert_eq!(t["frame_sha256"], frame.sha256());
    }
    assert_eq!(h["net_boundary_energy_j"], net);
    assert_eq!(h["physical_stored_change_j"], stored);
    assert_eq!(h["signed_remap_residual_j"], remap_sum);
    assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-9);
    assert!(h["absolute_remap_residual_j"].as_f64().unwrap() < 1e-9);
    assert_eq!(h["checkpoint"], frame.checkpoint(&state).unwrap());
    let (restored, restored_state) = Frame::from_checkpoint(&h["checkpoint"]).unwrap();
    assert_eq!(
        restored.checkpoint(&restored_state).unwrap(),
        h["checkpoint"]
    );
    assert_eq!(moving(&s, &heat_pattern()), result);
}

#[test]
fn exposed_heat_and_coverage_refusals_are_independent_of_motion_and_other_consumers() {
    let mut s = coverage_heat_scenario();
    let p = heat_pattern();
    let base = moving(&s, &p);
    assert_eq!(
        base["motion"]["result"]["transient_coverage"]["status"],
        "available"
    );
    let film = s
        .motion
        .as_ref()
        .unwrap()
        .transient_coverage
        .as_ref()
        .unwrap()
        .boundaries[0];
    s.motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap()
        .exposed_boundaries = vec![Some(film), Some(film)];
    let exposed = moving(&s, &p);
    assert_eq!(
        exposed["motion"]["result"]["transient_coverage"]["status"],
        "available"
    );
    assert_eq!(
        base["motion"]["result"]["samples"],
        exposed["motion"]["result"]["samples"]
    );
    assert_ne!(
        base["motion"]["result"]["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"],
        exposed["motion"]["result"]["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"]
    );
    for mode in 0..5 {
        let mut bad = s.clone();
        let policy = bad
            .motion
            .as_mut()
            .unwrap()
            .transient_coverage
            .as_mut()
            .unwrap();
        match mode {
            0 => policy.body_triangle_ids.push(policy.body_triangle_ids[0]),
            1 => policy.exposed_boundaries.clear(),
            2 => policy.max_geometry_operations = 1,
            3 => {
                policy
                    .instance_layers
                    .insert("nonexistent".into(), "shell".into());
            }
            _ => policy.max_cell_steps = base["triangles"].as_array().unwrap().len() * 2,
        }
        let r = moving(&bad, &p);
        assert_eq!(r["motion"]["status"], "available");
        assert_eq!(
            r["motion"]["result"]["transient_coverage"]["status"], "refused",
            "mode {mode}"
        );
        assert!(r["motion"]["result"]["transient_coverage"]["result"].is_null());
        assert_eq!(
            r["motion"]["result"]["samples"],
            base["motion"]["result"]["samples"]
        );
        assert_eq!(r["drape"], base["drape"]);
        assert_eq!(
            r["motion"]["result"]["transient_surface"]["status"],
            "available"
        );
    }
    let missing = moving(&s, &pattern());
    assert_eq!(missing["motion"]["status"], "available");
    assert_eq!(
        missing["motion"]["result"]["transient_coverage"]["status"],
        "refused"
    );
    assert_eq!(
        missing["motion"]["result"]["keyframes"][0]["coverage"]["status"],
        "available"
    );
}

fn coverage_exterior_pattern(emissivity: f64) -> Pattern {
    let mut raw = heat_pattern().raw().clone();
    raw["fabric"]["material"]["record"]["properties"]["emissivity"] = json!({
        "value":emissivity,"unit":"1","source":"test","evidence":"estimate",
        "method":"Synthetic optical coefficient for coverage-consumer checks",
        "conditions":{"specimen":"synthetic-scenario-sheet","conditioning":"synthetic-dry",
            "face":"public","spectral_basis":"total_hemispherical",
            "temperature_min_c":"-80","temperature_max_c":"80"}});
    Pattern::new(raw)
}

fn coverage_exterior_scenario() -> Scenario {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
    ))
    .unwrap();
    let mut cold = fixture["exterior_exchange"].clone();
    cold["convection_conditions"] =
        json!("Synthetic prescribed coefficient on the accepted moving geometry");
    let mut warm = cold.clone();
    warm["air_c"] = json!(40.);
    warm["radiation"]["ambient_c"] = json!(55.);
    let mut s = coverage_heat_scenario();
    let policy = s
        .motion
        .as_mut()
        .unwrap()
        .transient_coverage
        .as_mut()
        .unwrap();
    policy.bare_boundaries = Some(policy.boundaries.iter().copied().map(Some).collect());
    for film in &mut policy.boundaries {
        film.outside_resistance_m2_k_w = 0.;
    }
    policy.exposed_boundaries = policy.boundaries.iter().copied().map(Some).collect();
    policy.exterior_exchange = Some(vec![
        Some(serde_json::from_value(cold).unwrap()),
        Some(serde_json::from_value(warm).unwrap()),
    ]);
    policy.exposed_exterior_exchange = policy.exterior_exchange.clone();
    s
}

#[test]
fn moving_coverage_exterior_consumes_frozen_weather_without_restarting_material_heat() {
    use ftx_drape::thermal::{
        Boundary,
        surface::{
            Surface,
            coverage::{
                ExteriorConditions,
                motion::{ExposedConditions, Frame},
            },
            projection,
        },
    };
    let s = coverage_exterior_scenario();
    let p = coverage_exterior_pattern(0.8);
    let result = moving(&s, &p);
    let motion = &result["motion"]["result"];
    let outcome = &motion["transient_coverage"];
    assert_eq!(outcome["status"], "available", "{outcome:#}");
    let h = &outcome["result"];
    let policy = s
        .motion
        .as_ref()
        .unwrap()
        .transient_coverage
        .as_ref()
        .unwrap();
    assert_eq!(h["request"], json!(policy));
    let saved = &h["checkpoint"]["frame"];
    let source = Surface::from_snapshot(&saved["source"]).unwrap();
    let projection: projection::Request = serde_json::from_value(saved["request"].clone()).unwrap();
    let triangles: Vec<[usize; 3]> =
        serde_json::from_value(saved["projection"]["layout"]["body_triangles"].clone()).unwrap();
    let mut frame = Frame::new(
        source.clone(),
        serde_json::from_value(result["positions_m"].clone()).unwrap(),
        form().vertices,
        &triangles,
        projection.clone(),
    )
    .unwrap();
    let mut state = frame.initial(15.).unwrap();
    let mut time = 0.;
    let mut energies = [0.; 4];
    let film = |b: ftx_drape::thermal::steady::request::Film| Boundary {
        inside_c: b.inside_c,
        outside_c: b.outside_c,
        inside_resistance: b.inside_resistance_m2_k_w,
        outside_resistance: b.outside_resistance_m2_k_w,
    };
    let trace = h["trace"].as_array().unwrap();
    for (sample, row) in motion["samples"].as_array().unwrap().iter().zip(trace) {
        let next = Frame::new(
            source.clone(),
            serde_json::from_value(sample["positions_m"].clone()).unwrap(),
            serde_json::from_value(sample["body_positions_m"].clone()).unwrap(),
            &triangles,
            projection.clone(),
        )
        .unwrap();
        if next.sha256() != frame.sha256() {
            state = next.remap(&frame, &state).unwrap().state;
        }
        frame = next;
        assert!(frame.has_exposed_cloth());
        let i = row["interval_index"].as_u64().unwrap() as usize;
        let boundaries = vec![film(policy.boundaries[i]); frame.body_triangle_count()];
        let bare = vec![
            film(policy.bare_boundaries.as_ref().unwrap()[i].unwrap());
            frame.body_triangle_count()
        ];
        let environments = vec![
            policy.exterior_exchange.as_ref().unwrap()[i]
                .clone()
                .unwrap();
            frame.body_triangle_count()
        ];
        let exposed_boundaries =
            vec![film(policy.exposed_boundaries[i].unwrap()); frame.cloth_triangle_count()];
        let exposed_environments = vec![
            policy.exposed_exterior_exchange.as_ref().unwrap()[i]
                .clone()
                .unwrap();
            frame.cloth_triangle_count()
        ];
        let end = sample["time_s"].as_f64().unwrap();
        let count = row["thermal_substeps"].as_u64().unwrap();
        let dt = (end - time) / count as f64;
        for _ in 0..count {
            let step = frame
                .advance_with_exchange(
                    &state,
                    dt,
                    &boundaries,
                    Some(ExteriorConditions {
                        environments: &environments,
                        bare_boundaries: &bare,
                    }),
                    Some(ExposedConditions {
                        boundaries: &exposed_boundaries,
                        environments: Some(&exposed_environments),
                    }),
                )
                .unwrap();
            let covered = step.heat().exterior_exchange.as_ref().unwrap();
            let exposed = step.exposed_exterior_exchange().unwrap();
            for (total, power) in energies.iter_mut().zip([
                covered.convective_heat_w,
                covered.radiative_heat_w,
                exposed.convective_heat_w,
                exposed.radiative_heat_w,
            ]) {
                *total += dt * power;
            }
            state = step.into_state();
        }
        assert_eq!(row["exterior_exchange"]["convective_energy_j"], energies[0]);
        assert_eq!(row["exterior_exchange"]["radiative_energy_j"], energies[1]);
        assert_eq!(
            row["exposed_exterior_exchange"]["convective_energy_j"],
            energies[2]
        );
        assert_eq!(
            row["exposed_exterior_exchange"]["radiative_energy_j"],
            energies[3]
        );
        time = end;
    }
    assert_eq!(h["checkpoint"], frame.checkpoint(&state).unwrap());
    let (restored, state) = Frame::from_checkpoint(&h["checkpoint"]).unwrap();
    assert_eq!(restored.checkpoint(&state).unwrap(), h["checkpoint"]);
    assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-8);
    assert!(h["absolute_remap_residual_j"].as_f64().unwrap() < 1e-8);
    assert!(
        trace[0]["exterior_exchange"]["convective_heat_w"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert!(
        trace.last().unwrap()["exposed_exterior_exchange"]["radiative_heat_w"]
            .as_f64()
            .unwrap()
            < 0.
    );
    assert!(h["final_field"]["exterior_exchange"].is_object());
    assert!(
        !h["final_field"]["exposed_exterior_exchange"]["triangles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(result, moving(&s, &p));
    let different_finish = moving(&s, &coverage_exterior_pattern(0.2));
    assert_eq!(
        different_finish["motion"]["result"]["samples"],
        motion["samples"]
    );
    assert_ne!(
        different_finish["motion"]["result"]["transient_coverage"]["result"]["checkpoint"]["exposed_temperature_c"],
        h["checkpoint"]["exposed_temperature_c"]
    );
}

#[test]
fn coverage_exterior_schedule_and_optical_refusals_preserve_other_motion_consumers() {
    let s = coverage_exterior_scenario();
    let p = coverage_exterior_pattern(0.8);
    let expected = moving(&s, &p);
    for mode in 0..8 {
        let mut bad = s.clone();
        let policy = bad
            .motion
            .as_mut()
            .unwrap()
            .transient_coverage
            .as_mut()
            .unwrap();
        match mode {
            0 => policy.exterior_exchange.as_mut().unwrap().truncate(1),
            1 => policy.bare_boundaries = None,
            2 => policy.boundaries[1].outside_resistance_m2_k_w = 0.2,
            3 => policy.exposed_boundaries[1] = None,
            4 => policy
                .exposed_exterior_exchange
                .as_mut()
                .unwrap()
                .truncate(1),
            5 => {
                policy.exterior_exchange.as_mut().unwrap()[0]
                    .as_mut()
                    .unwrap()
                    .air_c = 7.;
            }
            6 => policy.exterior_exchange.as_mut().unwrap()[0] = None,
            _ => {
                policy.exposed_exterior_exchange.as_mut().unwrap()[0]
                    .as_mut()
                    .unwrap()
                    .radiation
                    .as_mut()
                    .unwrap()
                    .exposed_face = "unqualified-face".into();
            }
        }
        let result = moving(&bad, &p);
        let motion = &result["motion"]["result"];
        assert_eq!(result["motion"]["status"], "available", "mode {mode}");
        assert_eq!(
            motion["transient_coverage"]["status"], "refused",
            "mode {mode}"
        );
        assert!(motion["transient_coverage"]["result"].is_null());
        assert_eq!(motion["samples"], expected["motion"]["result"]["samples"]);
        assert_eq!(motion["transient_surface"]["status"], "available");
    }
    let missing = moving(&s, &heat_pattern());
    assert_eq!(missing["motion"]["status"], "available");
    assert_eq!(
        missing["motion"]["result"]["transient_coverage"]["status"],
        "refused"
    );
    let legacy = moving(&coverage_heat_scenario(), &p);
    let heat = &legacy["motion"]["result"]["transient_coverage"]["result"];
    for field in [
        "exterior_exchange",
        "bare_boundaries",
        "exposed_exterior_exchange",
    ] {
        assert!(heat["request"].get(field).is_none());
        assert!(heat.get(field).is_none());
        assert!(heat["final_field"].get(field).is_none());
    }
}

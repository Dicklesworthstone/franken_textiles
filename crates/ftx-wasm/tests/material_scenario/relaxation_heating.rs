//! Real accepted Maxwell work must retain each material's own heat conversion during comparison.
//! Every source and body here is synthetic; this checks consumer composition, not measured fit.
use super::*;
use ftx_drape::study::motion;

fn maxwell(fraction: f64) -> Value {
    let mut selection: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-relaxation-heat.json"
    ))
    .unwrap();
    selection["record"]["properties"]["membrane_relaxation_heat_fraction"]["value"] =
        json!(fraction);
    selection
}

fn garment() -> Pattern {
    let width = 40. * std::f64::consts::PI;
    let pieces: Vec<_> = ["front", "back"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            json!({"id":id,"cut":{"qty":1},
                "vertices":{"a":[0,0],"b":[width,0],"c":[width,30],"d":[0,30]},
                "edges":[{"id":"bottom","from":"a","to":"b"},
                    {"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},
                    {"id":"left","from":"d","to":"a"}],
                "grain":[[0,0],[0,30]],
                "placement":{"part":"torso","radius_mm":40,"x0":width/2.,"theta_deg":180*i}})
        })
        .collect();
    Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":maxwell(0.75)},"pieces":pieces,
        "seams":[{"id":"left","a":"front/right","b":"back/left"},
            {"id":"right","a":"back/right","b":"front/left"}]}))
}

fn scenario() -> Scenario {
    // The same synthetic garment and thermal settings as the production relaxation-heating tests.
    // A completed sewing-only stage admits geometric comparisons without a gravity-fit claim.
    let boundary = json!({"inside_c":20.,"outside_c":20.,
        "inside_resistance_m2_k_w":1.,"outside_resistance_m2_k_w":1.});
    serde_json::from_value(json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":16.,"sew_steps":120,"drape_steps":0,"step":0.0005},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":4000000,
        "motion":{"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
            "max_step_s":0.0005,"max_trials":16,"max_pairs":100000,
            "max_sample_vertices":200000,
            "transient_surface":{"schema":"ftx-motion-surface-heat/1",
                "initial_temperature_c":20.,"cells_per_layer":2,
                "max_step_s":0.00025,"max_cell_steps":5000000,
                "relaxation_dissipation_heat":"uniform_reference_thickness",
                "boundaries":[boundary]},
            "transient_coverage":{"schema":"ftx-motion-coverage-heat/1",
                "body_triangle_ids":[0],"instance_layers":{"front":"shell","back":"shell"},
                "settings":{"max_distance_m":0.08,"min_normal_cosine":0.1,
                    "separation_tolerance_m":0.00001,"cells_per_layer":2,"interfaces":[]},
                "initial_temperature_c":20.,"max_step_s":0.00025,
                "max_cell_steps":5000000,"max_geometry_operations":1000000,
                "relaxation_dissipation_heat":"uniform_reference_thickness",
                "boundaries":[boundary],"exposed_boundaries":[null]}}}))
    .unwrap()
}

#[test]
fn distinct_material_heat_fractions_share_one_maxwell_law_in_comparison() {
    let cylinder = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&cylinder.vertices, &cylinder.triangles),
        display: (&cylinder.vertices, &cylinder.triangles),
        arm_deg: 35.,
    };
    let pattern = garment();
    let scenario = scenario();
    let mut session = Session::new(&scenario, &body).unwrap();
    let target = cylinder
        .vertices
        .iter()
        .map(|p| *p + Vec3::new(0., 0., 0.00005))
        .collect();
    session
        .set_motion_path(motion::Path::new(
            scenario.motion.as_ref().unwrap(),
            body.collision,
            vec![target],
        ))
        .unwrap();
    let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
        "variants":[
            {"key":"a-half","changes":{"schema":"ftx-material-bindings/1",
                "default":maxwell(0.375),"pieces":{"back":maxwell(0.)}}},
            {"key":"b-full","changes":{"schema":"ftx-material-bindings/1",
                "default":maxwell(0.75),"pieces":{"back":maxwell(0.)}}}],
        "comparison":{"basis":"nominal",
            "required_consumers":["transient_surface","transient_coverage"],
            "objectives":[
                {"metric":"transient_surface_final_max_temperature_c","direction":"minimize"},
                {"metric":"transient_coverage_final_max_temperature_c","direction":"minimize"}]}}))
    .unwrap();
    let result = request
        .evaluate_with(&pattern, &session.descriptor(), |_, p| {
            session.evaluate(p, |_| {})
        })
        .unwrap();
    let result = serde_json::to_value(result).unwrap();
    let work_field = "membrane_relaxation_dissipation_j";
    let comparisons = result["comparison"]["alternatives"].as_array().unwrap();
    let simulations = result["simulations"].as_array().unwrap();
    assert_eq!(comparisons.len(), 2);
    assert_eq!(simulations.len(), 2);
    assert_eq!(result["comparison"]["incomparable"], json!([]));
    assert_eq!(
        result["comparison"]["frontier"],
        json!(["a-half"]),
        "{}",
        result["comparison"]
    );
    for (index, (candidate, row)) in comparisons.iter().zip(simulations).enumerate() {
        let fraction = [0.375, 0.75][index];
        assert_eq!(row["evaluation"]["status"], "available", "{row}");
        let simulation = &row["evaluation"]["result"];
        assert_eq!(simulation["drape"]["settled"], true);
        assert_eq!(simulation["drape"]["gating"], json!([]));
        assert_eq!(simulation["metrics"]["simulation_gating_clear"], true);
        assert_eq!(simulation["motion"]["status"], "available");
        assert_eq!(candidate["eligible"], true, "{candidate}");
        let motion = &simulation["motion"]["result"];
        let surface = &motion["transient_surface"];
        let coverage = &motion["transient_coverage"];
        assert_eq!(surface["status"], "available", "{surface}");
        assert_eq!(coverage["status"], "available", "{coverage}");
        let surface = &surface["result"];
        let coverage = &coverage["result"];
        let material = &simulation["drape"]["material"];
        let owners = material["triangle_instance"].as_array().unwrap();
        let instances = material["instances"].as_array().unwrap();
        let initial = motion["initial_sample"][work_field].as_array().unwrap();
        let samples = motion["samples"].as_array().unwrap();
        let last = samples.last().unwrap()[work_field].as_array().unwrap();
        assert_eq!(initial.len(), owners.len());
        assert_eq!(last.len(), owners.len());
        let mut piece_work = [0.; 2];
        let mut initial_work = 0.;
        let mut expected_work = 0.;
        let mut expected_heat = 0.;
        for (triangle, owner) in owners.iter().enumerate() {
            let instance = &instances[owner.as_u64().unwrap() as usize];
            let piece = instance["piece_index"].as_u64().unwrap() as usize;
            let before = initial[triangle].as_f64().unwrap();
            let delta = last[triangle].as_f64().unwrap() - before;
            assert!(delta >= 0.);
            initial_work += before;
            expected_work += delta;
            piece_work[piece] += delta;
            if piece == 0 {
                expected_heat += fraction * delta;
            } else {
                // The back still dissipates mechanical work, but its qualified conversion is zero.
                let temperatures = &surface["final"]["temperature_c"][triangle];
                for temperature in temperatures.as_array().unwrap() {
                    assert!((temperature.as_f64().unwrap() - 20.).abs() < 1e-10);
                }
            }
        }
        assert!(initial_work > 0.);
        assert!(piece_work.iter().all(|work| *work > 0.));
        assert!(expected_heat > 0.);
        let heating = &surface["relaxation_dissipation_heat"];
        assert_eq!(heating, &coverage["relaxation_dissipation_heat"]);
        let materials = heating["material_fractions"].as_array().unwrap();
        assert_eq!(materials.len(), 2, "{heating}");
        assert_ne!(
            materials[0]["material_sha256"],
            materials[1]["material_sha256"]
        );
        assert_eq!(materials[0]["law_sha256"], materials[1]["law_sha256"]);
        for instance in instances {
            let receipt = materials
                .iter()
                .find(|entry| entry["material_sha256"] == instance["record_sha256"])
                .unwrap();
            assert_eq!(
                receipt["fraction"],
                if instance["piece_index"] == 0 {
                    fraction
                } else {
                    0.
                }
            );
        }
        for (field, expected) in [
            ("initial_viscous_dissipation_j", initial_work),
            ("viscous_dissipation_j", expected_work),
            ("supplied_heat_j", expected_heat),
            ("unconverted_dissipation_j", expected_work - expected_heat),
        ] {
            let actual = heating[field].as_f64().unwrap();
            assert!((actual - expected).abs() < 1e-12, "{field}");
        }
        for (history, stored, metric) in [
            (
                surface,
                "stored_change_j",
                "transient_surface_final_max_temperature_c",
            ),
            (
                coverage,
                "physical_stored_change_j",
                "transient_coverage_final_max_temperature_c",
            ),
        ] {
            let stored_energy = history[stored].as_f64().unwrap();
            let boundary_energy = history["net_boundary_energy_j"].as_f64().unwrap();
            assert!((stored_energy - boundary_energy - expected_heat).abs() < 1e-10);
            let final_sample = history["trace"].as_array().unwrap().last().unwrap();
            assert_eq!(
                candidate["readings"][metric]["value"],
                final_sample["temperature_range_c"][1]
            );
        }
        let reference = &simulations[0]["evaluation"]["result"]["motion"]["result"];
        assert_eq!(
            motion["initial_sample"][work_field],
            reference["initial_sample"][work_field]
        );
        let reference_samples = reference["samples"].as_array().unwrap();
        assert_eq!(samples.len(), reference_samples.len());
        for (sample, original) in samples.iter().zip(reference_samples) {
            assert_eq!(sample["positions_m"], original["positions_m"]);
            assert_eq!(sample[work_field], original[work_field]);
        }
    }
}

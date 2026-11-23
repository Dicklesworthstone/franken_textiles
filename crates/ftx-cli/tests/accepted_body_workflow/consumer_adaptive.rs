//! Actual sewn-cloth objectives guide bounded design through both CLIs and portable authority.
//! The body and source properties are synthetic software fixtures, not measured garment evidence.
use super::*;

#[path = "pareto.rs"]
mod pareto;
#[path = "worst_case.rs"]
mod worst_case;

#[test]
fn regional_guards_and_objectives_share_real_motion_and_preserve_adaptive_cache_and_scope_identity()
{
    use ftx_export::material_study::Request;
    use ftx_pattern::Pattern;
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model = ftx_wasm::fold::load_body(&bytes).unwrap();
    let body = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":35.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.0;
    pattern["pieces"][0]["vertices"]["b"] = json!([width, 0.0]);
    pattern["pieces"][0]["vertices"]["c"] = json!([width, 100.0]);
    pattern["pieces"][0]["vertices"]["d"] = json!([0.0, 100.0]);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    let regions = json!([
        {"id":"seam-side","instance":"band","selector":{"kind":"whole_triangle_centroid",
            "polygon_mm":[[-1.,-1.],[width*0.2,-1.],[width*0.2,101.],[-1.,101.]]}},
        {"id":"panel-center","instance":"band","selector":{"kind":"whole_triangle_centroid",
            "polygon_mm":[[width*0.3,-1.],[width*0.8,-1.],[width*0.8,101.],[width*0.3,101.]]}}
    ]);
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":180,"drape_steps":0,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000,
        "material_strain":{"schema":"ftx-material-strain/2","regions":regions,
            "max_triangle_samples":100000},
        "contact_loads":{"schema":"ftx-contact-loads/2","regions":regions,
            "max_vertex_samples":100000,"max_pair_evaluations":1000000},
        "motion":{"schema":"ftx-drape-motion/1",
            "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
            "max_step_s":0.0005,"max_trials":8,"max_pairs":100000,"max_sample_vertices":100000}});
    // These deliberately permissive numerical limits exercise scope routing, not failure strains
    // or wearer safety. The same metric has two distinct region guards and a union guard.
    let constraints = json!([
        {"metric":"max_warp_green_strain","maximum":10.},
        {"metric":"max_warp_green_strain","region":"seam-side","maximum":11.},
        {"metric":"max_warp_green_strain","region":"panel-center","maximum":12.},
        {"metric":"motion_max_warp_green_strain","region":"seam-side","maximum":13.},
        {"metric":"motion_max_warp_green_strain","region":"panel-center","maximum":14.},
        {"metric":"max_body_contact_vertex_force_n","region":"seam-side","maximum":1000000.},
        {"metric":"max_body_contact_vertex_force_n","region":"panel-center","maximum":2000000.},
        {"metric":"motion_max_body_contact_vertex_force_n","maximum":3000000.},
        {"metric":"motion_max_body_contact_vertex_force_n","region":"seam-side","maximum":4000000.},
        {"metric":"motion_max_body_contact_vertex_force_n","region":"panel-center","maximum":5000000.}
    ]);
    let mut study = json!({"schema":"ftx-design-study/1",
        "design_adaptive":{"objective":"consumer_comparison","material_key":"sheet",
            "case_key":"moving","constraint_restoration":"componentwise",
            "variables":[{"key":"length","bounds_mm":[-10.,0.],"initial_mm":0.,
                "initial_step_mm":10.,"minimum_step_mm":10.,
                "sites":[{"piece":"band","axis":"y","at_mm":50.,"multiplier":1.}]}],
            "max_evaluations":2,"max_designs":1},
        "design_fit":{"fit_spec":{"schema":"fabric-fit/1","points":[{
            "id":"height-band","kind":"vertical","instance":"band","from":"a","to":"d",
            "body":"stature","body_scale":0.05,"target_mm":[0.,20.,100.]}]}},
        "variants":[{"key":"sheet","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "comparison":{"basis":"nominal",
            "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
            "constraints":constraints}});
    let matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":4,
        "cases":[{"key":"moving","scenario":scenario}]});
    let evaluate = |study: &Value| -> Value {
        serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &study.to_string(),
                &matrix.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap()
    };
    // The shortened band settles on a different welded mesh. Its nodal loads are
    // individually admissible, but comparing them across those meshes needs consent.
    let strict = evaluate(&study);
    let trials = strict["manufacturing"]["design_search"]["rows"]
        .as_array()
        .unwrap();
    let short = trials
        .iter()
        .find(|row| row["design"]["variables_mm"]["length"] == -10.0)
        .unwrap();
    assert_eq!(short["consumer_cases"][0]["comparison"]["eligible"], true);
    assert_eq!(short["selected"], false);
    assert!(
        short["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("source protocols differ")
    );
    study["comparison"]["allow_mixed_protocols"] = json!(true);
    let report = evaluate(&study);
    let search = &report["manufacturing"]["design_search"];
    assert_eq!(search["selected_designs"], 1, "{search}");
    let selected = search["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap();
    assert_eq!(selected["design"]["variables_mm"]["length"], -10.0);
    let key = serde_json::to_string(&json!([selected["design"]["key"], "sheet"])).unwrap();
    let case = &report["cases"][0];
    assert_eq!(case["key"], "moving");
    assert_eq!(report["comparison"]["frontier"], json!([key]));
    let candidate = case["comparison"]["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap();
    let result = &case["simulations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == key)
        .unwrap()["evaluation"]["result"];
    assert_eq!(candidate["eligible"], true, "{candidate}");
    assert_eq!(result["motion"]["status"], "available", "{result}");
    for field in [
        "readings",
        "regional_readings",
        "eligible",
        "exclusions",
        "input_protocol_sha256",
    ] {
        assert_eq!(
            selected["consumer_cases"][0]["comparison"][field],
            candidate[field]
        );
    }
    for (prefix, source) in [("", result), ("motion_", &result["motion"]["result"])] {
        assert_eq!(source["material_strain"]["status"], "available");
        assert_eq!(source["contact_loads"]["status"], "available");
        for region in ["seam-side", "panel-center"] {
            for (metric, value, unit) in [
                (
                    format!("{prefix}max_warp_green_strain"),
                    &source["material_strain"]["result"]["regions"][region]["green_strain_range"]
                        [0][1],
                    "1",
                ),
                (
                    format!("{prefix}max_body_contact_vertex_force_n"),
                    &source["contact_loads"]["result"]["regions"][region]["max_body_vertex_force_n"],
                    "N",
                ),
            ] {
                let reading = &candidate["regional_readings"][region][&metric];
                assert!(value.is_number(), "{metric}: {source}");
                assert_eq!(reading["value"], *value);
                assert_eq!(reading["unit"], unit);
                assert!(reading.get("reported_bounds").is_none());
            }
        }
    }
    assert_eq!(
        candidate["readings"]["max_warp_green_strain"]["value"],
        result["material_strain"]["result"]["green_strain_range"][0][1]
    );
    assert_eq!(
        candidate["readings"]["motion_max_body_contact_vertex_force_n"]["value"],
        result["motion"]["result"]["contact_loads"]["result"]["max_body_vertex_force_n"]
    );
    assert_eq!(
        candidate["readings"]["motion_max_warp_green_strain"]["value"],
        result["motion"]["result"]["material_strain"]["result"]["green_strain_range"][0][1]
    );
    for trial in search["rows"].as_array().unwrap() {
        let guards = trial["constraint_violations"].as_array().unwrap();
        assert_eq!(guards.len(), constraints.as_array().unwrap().len());
        for declared in constraints.as_array().unwrap() {
            let guard = guards
                .iter()
                .find(|guard| {
                    guard["metric"] == declared["metric"]
                        && guard.get("region") == declared.get("region")
                })
                .unwrap();
            assert_eq!(guard["case_key"], "moving");
            assert_eq!(guard["metric"], declared["metric"]);
            assert_eq!(guard.get("region"), declared.get("region"));
            assert_eq!(guard["maximum_excess"], 0.0);
        }
    }
    let rows = report["manufacturing"]["alternatives"].as_array().unwrap();
    let probes = search["consumer_guidance"]["evaluations"].as_u64().unwrap() as usize;
    assert_eq!(
        report["evaluations"],
        probes + rows.len() - 1,
        "all ten local/union guards share each probe; selected production output is reused"
    );

    // Replay the exact retained production result, changing only decision requirements. A single
    // failed or absent region must not borrow the passing union or another region's reading.
    let manufacture = rows.iter().find(|row| row["key"] == key).unwrap();
    let altered = Pattern::from_json(
        manufacture["design"]["altered_pattern_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    for absent in [false, true] {
        let mut guards = constraints.clone();
        if absent {
            guards[1]["region"] = json!("missing-region");
        } else {
            // E11 = (|F_warp|^2 - 1)/2 is never below -0.5, so this is a known rejection.
            guards[1]["maximum"] = json!(-1.0);
        }
        let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
            "variants":[{"key":"replay","changes":manufacture["frozen_changes"]}],
            "comparison":{"basis":"nominal",
                "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                "constraints":guards}}))
        .unwrap();
        let mut calls = 0;
        let replay = request
            .evaluate_with(&altered, &case["scenario"], |_, bound| {
                calls += 1;
                assert_eq!(bound.raw(), &manufacture["binding"]["result"]["pattern"]);
                Ok(result.clone())
            })
            .unwrap();
        assert_eq!(calls, 1);
        let replay = serde_json::to_value(replay).unwrap();
        let rejected = &replay["comparison"]["alternatives"][0];
        assert_eq!(rejected["eligible"], false);
        assert_eq!(
            rejected["exclusions"].as_array().unwrap().len(),
            1,
            "{rejected}"
        );
        assert_eq!(
            rejected["regional_readings"]["panel-center"],
            candidate["regional_readings"]["panel-center"]
        );
        assert_eq!(rejected["readings"], candidate["readings"]);
        if absent {
            assert!(
                rejected["regional_readings"]
                    .get("missing-region")
                    .is_none()
            );
        }
    }

    // The same real producer now guides discovery on one named region, rather than its union.
    // The two CLI routes below replay this scoped search against the portable authority.
    let metric = "motion_max_warp_green_strain";
    let region = "panel-center";
    study["comparison"]["objectives"] = json!([
        {"metric":metric,"region":region,"direction":"minimize"}
    ]);
    for other_region in [Some("seam-side"), None] {
        let mut worst = study.clone();
        worst["design_adaptive"]["objective"] = json!("consumer_worst_case");
        worst["design_adaptive"]
            .as_object_mut()
            .unwrap()
            .remove("case_key");
        let mut other = matrix["cases"][0].clone();
        other["key"] = json!("other-scope");
        other["comparison"] = study["comparison"].clone();
        if let Some(other_region) = other_region {
            other["comparison"]["objectives"][0]["region"] = json!(other_region);
        } else {
            other["comparison"]["objectives"][0]
                .as_object_mut()
                .unwrap()
                .remove("region");
        }
        let mismatch = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":8,
            "cases":[matrix["cases"][0],other]});
        let error = on_model(
            &model,
            &digest,
            &pattern.to_string(),
            &worst.to_string(),
            &mismatch.to_string(),
            &body.to_string(),
            |_| {},
        )
        .unwrap_err();
        assert!(
            error.message.contains("identical metric, region"),
            "{error:?}"
        );
    }
    let report = evaluate(&study);
    let search = &report["manufacturing"]["design_search"];
    let guidance = &search["consumer_guidance"];
    assert_eq!(guidance["objective"], study["comparison"]["objectives"][0]);
    let selected = search["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["selected"] == true)
        .unwrap();
    let reading = &selected["consumer_cases"][0]["comparison"]["regional_readings"][region][metric];
    assert!(reading["value"].is_number(), "{selected}");
    assert_eq!(guidance["best_reading"], *reading);
    for trial in search["rows"].as_array().unwrap() {
        if trial["consumer_cases"][0]["comparison"]["eligible"] == true {
            let value = trial["consumer_cases"][0]["comparison"]["regional_readings"][region]
                [metric]["value"]
                .as_f64()
                .unwrap();
            assert!(reading["value"].as_f64().unwrap() <= value);
        }
    }
    let selected_key = serde_json::to_string(&json!([selected["design"]["key"], "sheet"])).unwrap();
    let final_candidate = report["cases"][0]["comparison"]["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == selected_key)
        .unwrap();
    assert_eq!(
        final_candidate["regional_readings"][region][metric],
        *reading
    );
    let retained = report["manufacturing"]["alternatives"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(
        report["evaluations"],
        guidance["evaluations"].as_u64().unwrap() as usize + retained - 1,
        "regional optimization reuses the retained physical result"
    );

    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    for (name, value) in [
        ("pattern.json", &pattern),
        ("study.json", &study),
        ("scenario.json", &matrix),
        ("body-input.json", &body),
    ] {
        save(&dir.join(name), value);
    }
    for (binary, prefix) in [
        (
            env!("CARGO_BIN_EXE_ftx"),
            &["--json", "materials", "scenario-study"][..],
        ),
        (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
    ] {
        let output = Command::new(binary)
            .current_dir(&dir)
            .args(prefix)
            .args([
                "pattern.json",
                "study.json",
                "scenario.json",
                "body-input.json",
                "model.ftxbody",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual.get("result").unwrap_or(&actual), &report);
    }
}

#[test]
fn consumer_adaptive_uses_real_scenario_mass_and_preserves_cached_incumbent_on_budget_refusal() {
    use ftx_wasm::materials::scenario::on_model;

    let bytes = model_bytes_with_rig(true);
    let digest = sha256_hex(&bytes);
    let model =
        ftx_body::AnnyModel::from_data(ftx_body::AnnyData::from_bytes(&bytes, true).unwrap())
            .unwrap();
    let body = json!({"schema":"ftx-scenario-body/1",
        "accepted_body_fit":receipt(&digest),"arm_abduction_deg":35.0,
        "measurements":{"wearer":{"stature_mm":1111.0}}});
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let width = std::f64::consts::TAU * 180.0;
    pattern["pieces"][0]["vertices"]["b"] = json!([width, 0.0]);
    pattern["pieces"][0]["vertices"]["c"] = json!([width, 100.0]);
    pattern["pieces"][0]["vertices"]["d"] = json!([0.0, 100.0]);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    let light = pattern["fabric"]["material"].clone();
    let mut heavy = light.clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let study = json!({"schema":"ftx-design-study/1",
        "design_adaptive":{"objective":"consumer_comparison","material_key":"light",
            "variables":[{"key":"length","bounds_mm":[-30.0,0.0],"initial_mm":0.0,
                "initial_step_mm":20.0,"minimum_step_mm":20.0,
                "sites":[{"piece":"band","axis":"y","at_mm":50.0,"multiplier":1.0}]}],
            "max_evaluations":8,"max_designs":1},
        "design_fit":{"fit_spec":{"schema":"fabric-fit/1","points":[{
            "id":"declared-height-band","kind":"vertical","instance":"band",
            "from":"a","to":"d","body":"stature","body_scale":0.05,
            "target_mm":[0.0,20.0,100.0]}]}},
        "variants":[
            {"key":"light","changes":{"schema":"ftx-material-bindings/1","default":light}},
            {"key":"heavy","changes":{"schema":"ftx-material-bindings/1","default":heavy}}],
        "comparison":{"basis":"nominal",
            "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}]}});
    // The existing production sewing-only fixture is settled and gating-clear. This does not
    // claim gravity-settled fit; the comparator still admits the complete real drape descriptor.
    let scenario = json!({"schema":"ftx-drape-scenario/1",
        "options":{"spacing":100.0,"sew_steps":120,"drape_steps":0,"step":0.001},
        "fit_spec":{},"max_mesh_points":4096,"max_vertex_frames":1000000});
    let dir = setup();
    std::fs::write(dir.join("model.ftxbody"), &bytes).unwrap();
    save(&dir.join("pattern.json"), &pattern);
    save(&dir.join("study.json"), &study);
    save(&dir.join("body-input.json"), &body);
    let capture = std::env::var_os("FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR").map(PathBuf::from);
    if let Some(capture) = &capture {
        std::fs::create_dir_all(capture).unwrap();
        std::fs::write(capture.join("model.ftxbody"), &bytes).unwrap();
        save(&capture.join("pattern.json"), &pattern);
        save(&capture.join("study.json"), &study);
        save(&capture.join("body-input.json"), &body);
    }
    let mut limited = scenario.clone();
    // One initial 98-point conservative reservation at 212 frames fits. Subsequent probes
    // exceed the SAME Session's quota; they cannot reset it or replace the cached incumbent.
    limited["max_vertex_frames"] = json!(25000);
    for (name, scenario) in [("complete", scenario), ("budget", limited)] {
        let report: Value = serde_json::from_str(
            &on_model(
                &model,
                &digest,
                &pattern.to_string(),
                &study.to_string(),
                &scenario.to_string(),
                &body.to_string(),
                |_| {},
            )
            .unwrap(),
        )
        .unwrap();
        let search = &report["manufacturing"]["design_search"];
        let guidance = &search["consumer_guidance"];
        assert_eq!(search["selected_designs"], 1, "{search}");
        assert_eq!(search["complete_search"], false);
        assert_eq!(search["global_optimum_claim"], false);
        assert_eq!(guidance["objective"]["metric"], "simulated_cloth_mass_kg");
        assert_eq!(guidance["reserved_evaluations"], 12);
        let selected = search["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|trial| trial["selected"] == true)
            .unwrap();
        let coordinate = selected["design"]["variables_mm"]["length"]
            .as_f64()
            .unwrap();
        assert_eq!(coordinate, if name == "complete" { -30.0 } else { 0.0 });
        let selected_key =
            serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
        let row = report["manufacturing"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        let simulation = report["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == selected_key)
            .unwrap();
        assert_eq!(
            simulation["evaluation"]["status"], "available",
            "{simulation}"
        );
        assert_eq!(simulation["evaluation"]["result"]["drape"]["settled"], true);
        assert_eq!(
            simulation["evaluation"]["result"]["pattern_sha256"],
            row["binding"]["result"]["pattern_sha256"]
        );
        assert_eq!(report["comparison"]["frontier"], json!([selected_key]));
        let candidate = &selected["consumer_comparison"];
        assert_eq!(candidate["eligible"], true);
        assert_eq!(
            guidance["best_reading"],
            candidate["readings"]["simulated_cloth_mass_kg"]
        );
        let value = guidance["best_reading"]["value"].as_f64().unwrap();
        let expected = width * (100.0 + coordinate) * 1e-6 * 0.2;
        assert!((value - expected).abs() < 1e-6, "{value} / {expected}");
        let altered = ftx_pattern::Pattern::from_json(
            row["design"]["altered_pattern_json"].as_str().unwrap(),
        )
        .unwrap();
        let frozen: ftx_pattern::material_binding::Changes =
            serde_json::from_value(row["frozen_changes"].clone()).unwrap();
        assert_eq!(
            frozen.apply(&altered).unwrap()["pattern"],
            row["binding"]["result"]["pattern"]
        );
        if name == "complete" {
            assert_eq!(search["rows"][0]["selected"], false);
            assert_eq!(search["rows"][0]["max_fit_band_violation_mm"], 0.0);
            assert!(value < search["rows"][0]["consumer_comparison"]["readings"]["simulated_cloth_mass_kg"]["value"].as_f64().unwrap());
        } else {
            assert!(
                search["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .any(|trial| {
                        trial["consumer_refusal"]["code"] == "budget_exceeded"
                            && trial["consumer_comparison"]["eligible"] == false
                    })
            );
        }
        save(&dir.join("scenario.json"), &scenario);
        for (binary, prefix) in [
            (
                env!("CARGO_BIN_EXE_ftx"),
                &["--json", "materials", "scenario-study"][..],
            ),
            (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
        ] {
            let output = Command::new(binary)
                .current_dir(&dir)
                .args(prefix)
                .args([
                    "pattern.json",
                    "study.json",
                    "scenario.json",
                    "body-input.json",
                    "model.ftxbody",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(actual.get("result").unwrap_or(&actual), &report);
        }
        if let Some(capture) = &capture {
            save(&capture.join(format!("{name}-scenario.json")), &scenario);
            save(&capture.join(format!("{name}-report.json")), &report);
        }
        if name == "complete" {
            for (label, bounded, mixed) in [
                ("matrix", false, false),
                ("bounds", true, false),
                ("mixed", true, true),
            ] {
                let metric = if bounded {
                    "assembly_mass_kg"
                } else {
                    "simulated_cloth_mass_kg"
                };
                let mut matrix_study = study.clone();
                matrix_study["design_adaptive"]["case_key"] = json!("weight");
                // The second case has an independent authored constraint. Its actual 0.017 kg
                // threshold admits the 80 mm band but excludes the lighter 70 mm candidate.
                let mut matrix = json!({"schema":"ftx-drape-scenario-matrix/1","max_evaluations":24,
                "cases":[{"key":"weight","scenario":scenario},
                    {"key":"guard","scenario":scenario,"comparison":{"basis":"nominal",
                        "objectives":[{"metric":"simulated_cloth_mass_kg","direction":"minimize"}],
                        "constraints":[{"metric":"simulated_cloth_mass_kg","minimum":0.017}]}}]});
                if bounded {
                    // Declared synthetic source ranges reach the actual manufacturing mass consumer.
                    // Net cut area is explicit; this does not infer retained seam/finishing losses.
                    matrix_study["assembly_mass"] = json!({"schema":"ftx-assembly-mass/1",
                    "inventory_complete":true,"seam_allowance_mm":0,"extras":[]});
                    for (index, bounds) in [(0, [0.19, 0.21]), (1, [0.39, 0.41])] {
                        matrix_study["variants"][index]["changes"]["default"]["record"]["properties"]
                            ["areal_density"]["bounds"] = json!(bounds);
                    }
                    matrix_study["comparison"] = json!({"basis":"reported_bounds",
                    "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}],
                    "required_consumers":["drape"]});
                    matrix["cases"][1]["comparison"] = matrix_study["comparison"].clone();
                    matrix["cases"][1]["comparison"]["constraints"] = json!([
                    {"metric":"assembly_mass_kg","minimum":0.0155}]);
                }
                if mixed {
                    // One actual scenario supplies both bounded manufacturing mass and nominal
                    // cloth/strain guards. These authored synthetic limits are not physical fit.
                    matrix_study["comparison"]["constraints"] = json!([
                        {"metric":"simulated_cloth_mass_kg","basis":"nominal","minimum":0.017},
                        {"metric":"max_absolute_edge_strain_pct","basis":"nominal","maximum":100}]);
                    matrix["cases"] = json!([{"key":"weight","scenario":scenario}]);
                    matrix["max_evaluations"] = json!(12);
                }
                let case_keys: &[&str] = if mixed {
                    &["weight"]
                } else {
                    &["weight", "guard"]
                };
                let matrix_report: Value = serde_json::from_str(
                    &on_model(
                        &model,
                        &digest,
                        &pattern.to_string(),
                        &matrix_study.to_string(),
                        &matrix.to_string(),
                        &body.to_string(),
                        |_| {},
                    )
                    .unwrap(),
                )
                .unwrap();
                let search = &matrix_report["manufacturing"]["design_search"];
                let guidance = &search["consumer_guidance"];
                assert_eq!(guidance["case_key"], "weight");
                assert_eq!(guidance["objective"]["metric"], metric);
                if bounded {
                    assert_eq!(guidance["basis"], "reported_bounds");
                } else {
                    assert!(guidance.get("basis").is_none());
                }
                assert_eq!(guidance["case_order"], json!(case_keys));
                assert_eq!(guidance["reserved_evaluations"], 12 * case_keys.len());
                assert_eq!(matrix_report["planned_evaluations"], 12 * case_keys.len());
                assert_eq!(search["selected_designs"], 1);
                let trials = search["rows"].as_array().unwrap();
                let selected = trials
                    .iter()
                    .find(|trial| trial["selected"] == true)
                    .unwrap();
                assert_eq!(selected["design"]["variables_mm"]["length"], -20.0);
                let shorter = trials
                    .iter()
                    .find(|trial| trial["design"]["variables_mm"]["length"] == -30.0)
                    .unwrap();
                assert_eq!(shorter["consumer_comparison"]["eligible"], !mixed);
                assert_eq!(
                    shorter["consumer_cases"][0]["comparison"]["eligible"],
                    !mixed
                );
                if !mixed {
                    assert_eq!(
                        shorter["consumer_cases"][1]["comparison"]["eligible"],
                        false
                    );
                }
                assert_eq!(shorter["selected"], false);
                assert!(
                    shorter["refusal"]["message"]
                        .as_str()
                        .unwrap()
                        .contains(if mixed { "weight" } else { "guard" })
                );
                let mass = guidance["best_reading"]["value"].as_f64().unwrap();
                assert!((mass - width * 80.0 * 1e-6 * 0.2).abs() < 1e-6);
                if bounded {
                    let reading = &shorter["consumer_comparison"]["readings"][metric];
                    // The 70 mm candidate passes this nominal minimum but fails its full source range.
                    assert!(reading["value"].as_f64().unwrap() > 0.0155);
                    assert!(reading["reported_bounds"][0].as_f64().unwrap() < 0.0155);
                    for (endpoint, density) in [0.19, 0.21].into_iter().enumerate() {
                        let expected = width * 80.0 * 1e-6 * density;
                        assert!(
                            (guidance["best_reading"]["reported_bounds"][endpoint]
                                .as_f64()
                                .unwrap()
                                - expected)
                                .abs()
                                < 1e-6
                        );
                    }
                }
                if mixed {
                    assert_eq!(matrix_report["cases"].as_array().unwrap().len(), 1);
                    let rejected = &shorter["consumer_comparison"]["readings"];
                    assert!(
                        rejected["simulated_cloth_mass_kg"]["value"]
                            .as_f64()
                            .unwrap()
                            < 0.017
                    );
                    for candidate in [selected, shorter] {
                        let readings = &candidate["consumer_comparison"]["readings"];
                        for name in ["simulated_cloth_mass_kg", "max_absolute_edge_strain_pct"] {
                            assert!(readings[name]["value"].is_number());
                            assert!(readings[name].get("reported_bounds").is_none());
                        }
                    }
                    assert_eq!(
                        matrix_report["cases"][0]["comparison"]["policy"]["constraints"][0]["basis"],
                        "nominal"
                    );
                }
                let selected_key =
                    serde_json::to_string(&json!([selected["design"]["key"], "light"])).unwrap();
                assert_eq!(
                    matrix_report["comparison"]["frontier"],
                    json!([selected_key])
                );
                if bounded {
                    let manufacture = matrix_report["manufacturing"]["alternatives"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|row| row["key"] == selected_key)
                        .unwrap();
                    assert_eq!(
                        guidance["best_reading"]["reported_bounds"],
                        manufacture["assembly_mass"]["result"]["assembly_mass"]["reported_bounds_kg"]
                    );
                }
                for (index, key) in case_keys.iter().enumerate() {
                    let case = &matrix_report["cases"][index];
                    assert_eq!(case["key"], *key);
                    assert_eq!(selected["consumer_cases"][index]["key"], *key);
                    let candidate = case["comparison"]["alternatives"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|candidate| candidate["key"] == selected_key)
                        .unwrap();
                    for field in [
                        "readings",
                        "eligible",
                        "exclusions",
                        "input_protocol_sha256",
                    ] {
                        assert_eq!(
                            selected["consumer_cases"][index]["comparison"][field],
                            candidate[field]
                        );
                    }
                    let simulation = case["simulations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|simulation| simulation["key"] == selected_key)
                        .unwrap();
                    assert_eq!(simulation["evaluation"]["status"], "available");
                    assert_eq!(simulation["evaluation"]["result"]["drape"]["settled"], true);
                    assert!(
                        (simulation["evaluation"]["result"]["drape"]["mass_kg"]
                            .as_f64()
                            .unwrap()
                            - mass)
                            .abs()
                            < 1e-12
                    );
                }
                // Every probe calls each actual case session once. Adding nominal guards does not
                // add cases/callbacks; selected light-material outputs are reused in every case.
                let rows = matrix_report["manufacturing"]["alternatives"]
                    .as_array()
                    .unwrap()
                    .len();
                let probes = guidance["evaluations"].as_u64().unwrap() as usize;
                assert_eq!(
                    matrix_report["evaluations"],
                    case_keys.len() * (probes + rows - 1)
                );
                assert!(
                    matrix_report["evaluations"].as_u64().unwrap() <= (12 * case_keys.len()) as u64
                );
                let study_file = format!("{label}-study.json");
                let scenario_file = format!("{label}-scenario.json");
                save(&dir.join(&study_file), &matrix_study);
                save(&dir.join(&scenario_file), &matrix);
                for (binary, prefix) in [
                    (
                        env!("CARGO_BIN_EXE_ftx"),
                        &["--json", "materials", "scenario-study"][..],
                    ),
                    (env!("CARGO_BIN_EXE_ftx-materials"), &["scenario-study"][..]),
                ] {
                    let output = Command::new(binary)
                        .current_dir(&dir)
                        .args(prefix)
                        .args([
                            "pattern.json",
                            &study_file,
                            &scenario_file,
                            "body-input.json",
                            "model.ftxbody",
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{} / {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(actual.get("result").unwrap_or(&actual), &matrix_report);
                }
                if let Some(capture) = &capture {
                    save(&capture.join(study_file), &matrix_study);
                    save(&capture.join(scenario_file), &matrix);
                    save(
                        &capture.join(format!("{label}-report.json")),
                        &matrix_report,
                    );
                }
            }
            worst_case::replay(
                &model,
                &digest,
                [&pattern, &study, &scenario, &body],
                &dir,
                capture.as_deref(),
            );
            pareto::replay(
                &model,
                &digest,
                [&pattern, &study, &scenario, &body],
                &dir,
                capture.as_deref(),
            );
        }
    }
}

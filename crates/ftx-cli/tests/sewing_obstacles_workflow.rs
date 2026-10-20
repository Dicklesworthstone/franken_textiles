//! Actual-binary checks of source cutouts in offset paths and folded-ply correspondence.
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn inputs() -> (Value, Value) {
    (
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-options.json"
        ))
        .unwrap(),
    )
}
fn files(p: &Value, o: &Value) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "ftx-obstacles-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let a = dir.join("pattern.json");
    let b = dir.join("options.json");
    fs::write(&a, serde_json::to_vec(p).unwrap()).unwrap();
    fs::write(&b, serde_json::to_vec(o).unwrap()).unwrap();
    (a, b)
}
fn compare(p: Value, o: Value, accepted: bool) {
    let (pf, of) = files(&p, &o);
    let pattern = Pattern::new(p.clone());
    let options: SewOptions = serde_json::from_value(o.clone()).unwrap();
    for cmd in ["sew-plan", "sew-stitches"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .arg(cmd)
            .arg(&pf)
            .arg(&of)
            .output()
            .unwrap();
        assert_eq!(
            out.status.success(),
            accepted,
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if accepted {
            let actual: Value = serde_json::from_slice(&out.stdout).unwrap();
            let expected = if cmd == "sew-plan" {
                serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
            } else {
                serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
            };
            assert_eq!(actual, expected);
            assert!(
                !String::from_utf8_lossy(&out.stdout)
                    .contains("\"hardware_execution_authorized\": true")
            );
        } else {
            assert!(
                out.stdout.is_empty(),
                "a refusal must not publish a partial plan"
            );
            assert!(!out.stderr.is_empty());
        }
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(pf).unwrap()).unwrap(),
        p
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(of).unwrap()).unwrap(),
        o
    );
}

#[test]
fn offset_obstacles_reach_the_real_native_plan_and_stitch_program() {
    let (mut p, mut o) = inputs();
    o["passes"][0]["stitch_path"] =
        json!({"offset_mm":[3,4],"mitre_limit":4,"minimum_cut_clearance_mm":1});
    p["pieces"][0]["internal"] =
        json!([{"kind":"cut","closed":true,"pts":[[5,30],[9,30],[9,40],[5,40]]}]);
    p["pieces"][0]["drills"] = json!([{"p":[12,70],"d":2}]);
    compare(p.clone(), o.clone(), true);
    p["pieces"][0]["drills"] = json!([{"p":[17,1.25],"d":0.2}]);
    compare(p, o, false);
}

#[test]
fn folded_cutout_changes_actual_stack_in_native_outputs_and_never_hides_missing_seam_material() {
    let (mut p, mut o) = inputs();
    o["passes"][0].as_object_mut().unwrap().remove("plies");
    o["passes"][0]["assembly"] = json!({
        "pieces":[{"instance":"front","origin_mm":[0,0],"x_axis":[1,0]},
            {"instance":"back","origin_mm":[40,0],"x_axis":[-1,0],"face_down":true}],
        "folds":[{"line_mm":[[30,-20],[30,120]],"moving_side":"right","stack":"over"}],
        "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1
    });
    p["pieces"][1]["internal"] =
        json!([{"kind":"cut","closed":true,"pts":[[17,-4],[23,-4],[23,104],[17,104]]}]);
    o["machine"]["max_stack_mm"] = json!(0.9);
    let plan = SewPlan::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.seams[0].assembly.as_ref().unwrap().layers.len(), 2);
    compare(p.clone(), o.clone(), true);
    p["pieces"][1]["internal"] =
        json!([{"kind":"cut","closed":true,"pts":[[-3,-4],[3,-4],[3,104],[-3,104]]}]);
    compare(p, o, false);
}

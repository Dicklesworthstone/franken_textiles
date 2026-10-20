//! Synthetic flat folds with removed source material, not calibrated sewing/tool-access tests.
use ftx_core::Vec2;
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn inputs(height: f64) -> (Value, Value) {
    let mut p: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for piece in p["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(height);
        piece["vertices"]["d"][1] = json!(height);
    }
    let mut o: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap();
    o["passes"][0].as_object_mut().unwrap().remove("plies");
    o["passes"][0]["assembly"] = json!({
        "pieces":[
            {"instance":"front","origin_mm":[0,0],"x_axis":[1,0]},
            {"instance":"back","origin_mm":[40,0],"x_axis":[-1,0],"face_down":true}
        ],
        "folds":[{"line_mm":[[30,-20],[30,120]],"moving_side":"right","stack":"over"}],
        "registration_tolerance_mm":1e-6,"minimum_boundary_clearance_mm":1.0
    });
    (p, o)
}
fn build(p: &Value, o: &Value) -> Result<SewPlan, ftx_core::Refusal> {
    SewPlan::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value::<SewOptions>(o.clone()).unwrap(),
    )
}
fn aperture(x: f64) -> Value {
    json!([{"kind":"cut","closed":true,"pts":[[x-3.,-4.],[x+3.,-4.],[x+3.,104.],[x-3.,104.]]}])
}

#[test]
fn folded_aperture_removes_only_its_actual_ply_and_rechecks_machine_stack() {
    let (mut p, mut o) = inputs(100.);
    let baseline = build(&p, &o).unwrap();
    p["pieces"][1]["internal"] = aperture(20.);
    o["machine"]["max_stack_mm"] = json!(0.9);
    let result = build(&p, &o).unwrap();
    let a = result.seams[0].assembly.as_ref().unwrap();
    assert_eq!(baseline.seams[0].stations, result.seams[0].stations);
    assert_eq!(baseline.seams[0].assembly.as_ref().unwrap().layers.len(), 3);
    assert_eq!(a.layers.len(), 2);
    assert_eq!(a.side_layers, [0, 1]);
    assert!((a.stack_mm - 0.8).abs() < 1e-12);
    assert!((a.minimum_boundary_clearance_mm - 3.).abs() < 1e-12);
    assert_eq!(a.layers[1].source_stations_mm[0], Vec2::ZERO);
    assert_ne!(baseline.pattern_sha256, result.pattern_sha256);
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(build(&p, &o).unwrap()).unwrap()
    );
    assert!(!result.hardware_execution_authorized && !result.physical_seam_quality_validated);
    let program = SewProgram::build(
        &Pattern::new(p.clone()),
        &serde_json::from_value(o.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        program.plan.seams[0]
            .assembly
            .as_ref()
            .unwrap()
            .layers
            .len(),
        2
    );
    // Closing the aperture must not keep the previously accepted two-ply thickness.
    p["pieces"][1]["internal"] = json!([]);
    assert!(
        build(&p, &o)
            .unwrap_err()
            .message
            .contains("stack thickness")
    );
}

#[test]
fn intended_seam_targets_in_removed_material_never_become_valid_lower_stacks() {
    for (piece, x) in [(0, 20.), (1, 0.)] {
        let (mut p, o) = inputs(100.);
        p["pieces"][piece]["internal"] = aperture(x);
        assert!(
            build(&p, &o)
                .unwrap_err()
                .message
                .contains("target missing")
        );
    }
}

#[test]
fn circular_drill_voids_follow_folds_and_keep_the_full_chord_clear_of_the_rim() {
    let (mut p, mut o) = inputs(4.);
    p["pieces"][1]["drills"] = json!([{"p":[20,2],"d":6}]);
    let result = build(&p, &o).unwrap();
    let a = result.seams[0].assembly.as_ref().unwrap();
    assert_eq!(a.layers.len(), 2);
    assert!((a.minimum_boundary_clearance_mm - 1.).abs() < 1e-12);
    o["passes"][0]["assembly"]["minimum_boundary_clearance_mm"] = json!(1.001);
    assert!(build(&p, &o).unwrap_err().message.contains("rim clearance"));
}

#[test]
fn folded_needle_chords_cannot_skip_tiny_holes_or_slits_between_samples() {
    for mark in [
        json!({"kind":"cut","closed":false,"pts":[[19,1.25],[21,1.25]]}),
        json!({"kind":"cut","closed":true,"pts":[[19.9,1.1],[20.1,1.1],[20.1,1.4],[19.9,1.4]]}),
    ] {
        let (mut p, o) = inputs(100.);
        p["pieces"][1]["internal"] = json!([mark]);
        assert!(build(&p, &o).unwrap_err().message.contains("clearance"));
    }
    let (mut p, o) = inputs(100.);
    p["pieces"][1]["drills"] = json!([{"p":[20,1.25],"d":0.2}]);
    assert!(build(&p, &o).unwrap_err().message.contains("drill-rim"));
}

#[test]
fn concave_aperture_endpoint_membership_does_not_prove_absent_material_between_stations() {
    let (mut p, o) = inputs(100.);
    // Every needle station x=20 is in the void; a narrow remaining tongue lies at y=1.2..1.3.
    p["pieces"][1]["internal"] = json!([{"kind":"cut","closed":true,"pts":[
        [17,-4],[23,-4],[23,1.2],[19,1.2],[19,1.3],[23,1.3],[23,104],[17,104]
    ]}]);
    assert!(
        build(&p, &o)
            .unwrap_err()
            .message
            .contains("extra material ply")
    );
}

#[test]
fn harmless_openings_preserve_assembly_and_source_snapshots_but_bind_new_pattern_identity() {
    let (mut p, o) = inputs(100.);
    let baseline = build(&p, &o).unwrap();
    p["pieces"][0]["internal"] =
        json!([{"kind":"cut","closed":true,"pts":[[2,30],[6,30],[6,40],[2,40]]}]);
    p["pieces"][0]["drills"] = json!([{"p":[5,70],"d":2}]);
    let result = build(&p, &o).unwrap();
    assert_eq!(
        serde_json::to_value(&baseline.seams[0].assembly).unwrap(),
        serde_json::to_value(&result.seams[0].assembly).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&baseline.materials).unwrap(),
        serde_json::to_value(&result.materials).unwrap()
    );
    assert_ne!(baseline.pattern_sha256, result.pattern_sha256);
}

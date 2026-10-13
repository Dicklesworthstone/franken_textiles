//! End-to-end paired drapes on a synthetic body: no Anny assets or wearer data needed.

use ftx_core::Vec3;
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::PartFrames;
use ftx_drape::two_start::{self, TwoStartOptions};
use ftx_drape::{Body, PartFrame};
use ftx_pattern::Pattern;
use serde_json::json;

struct Form;

impl PartFrames for Form {
    fn frame(&self, _: &str) -> Option<PartFrame> {
        Some(PartFrame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        })
    }
    fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
        Some(20.0)
    }
    fn section_radius(&self, _: &str, _: f64) -> f64 {
        30.0
    }
}

fn form() -> Body {
    let mut vertices = Vec::new();
    for z in [-0.02, 0.12] {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(i as f64 * std::f64::consts::TAU / 16.0);
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

fn pattern() -> Pattern {
    let w = 40.0 * std::f64::consts::TAU;
    Pattern::new(json!({
        "schema": "fabric-pattern/1", "units": "mm", "fabric": {"tensile_N_m": 100},
        "pieces": [{"id": "band", "name": "band", "cut": {"qty": 1},
            "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, 20], "d": [0, 20]},
            "edges": [{"id":"bottom", "from":"a", "to":"b"},
                {"id":"right", "from":"b", "to":"c"},
                {"id":"top", "from":"c", "to":"d"},
                {"id":"left", "from":"d", "to":"a"}],
            "notches": [], "grain": [[0, 0], [0, 20]],
            "placement": {"part":"torso", "radius_mm":40, "x0": w / 2.0}
        }],
        "seams": [{"id":"join", "a":"band/right", "b":"band/left"}]
    }))
}

#[test]
fn paired_pipeline_preserves_primary_and_records_both_paths() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 3,
        step: 1e-3,
        ..DrapeOptions::default()
    };
    let policy = TwoStartOptions {
        alternate_sew_steps: 5,
        ..TwoStartOptions::default()
    };
    let pattern = pattern();
    let ordinary = drape(&pattern, &json!({}), &body, &opts, |_| {}).unwrap();
    let mut progress = Vec::new();
    let checked = two_start::drape(&pattern, &json!({}), &body, &opts, &policy, |line| {
        progress.push(line.to_string());
    })
    .unwrap();
    assert_eq!(checked.fold.frames, ordinary.fold.frames);
    assert_eq!(checked.scene.solver.x, ordinary.scene.solver.x);
    assert_eq!(checked.report["seams"], ordinary.report["seams"]);
    let study = &checked.report["two_start"];
    // Short runs intentionally retain initial motion: agreement must not promote them to settled.
    assert_eq!(study["status"], "REVIEW_REQUIRED", "{study}");
    assert_eq!(study["schema"], "ftx-two-start/1");
    assert_eq!(study["primary"]["sew_steps"], 3);
    assert_eq!(study["alternate"]["sew_steps"], 5);
    assert_eq!(study["primary"]["drape_steps"], 3);
    assert_eq!(study["alternate"]["drape_steps"], 3);
    assert!(study["alternate_report"].get("animation").is_none());
    assert!(!study["metrics"].as_array().unwrap().is_empty());
    assert!(study["positions"]["max_mm"].as_f64().unwrap().is_finite());
    assert!(
        progress
            .iter()
            .any(|p| p == "two-start alternate: 5 sewing steps")
    );
    if study["status"] == "REVIEW_REQUIRED" {
        assert!(!checked.gating().is_empty());
        assert!(
            study["flags"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| checked.report["gating"].as_array().unwrap().contains(f))
        );
    }
    let repeated = two_start::drape(&pattern, &json!({}), &body, &opts, &policy, |_| {}).unwrap();
    assert_eq!(repeated.report, checked.report);
}

#[test]
fn an_unsettled_pair_is_gating_even_when_both_paths_agree() {
    let form = form();
    let body = DrapeBody {
        frames: &Form,
        collision: (&form.vertices, &form.triangles),
        display: (&form.vertices, &form.triangles),
        arm_deg: 35.0,
    };
    // The loose ring falls along the cylinder; agreement between falling runs is not settledness.
    let opts = DrapeOptions {
        spacing: 24.0,
        sew_steps: 3,
        drape_steps: 9,
        step: 0.02,
        ..DrapeOptions::default()
    };
    let policy = TwoStartOptions {
        alternate_sew_steps: 5,
        ..TwoStartOptions::default()
    };
    let run = two_start::drape(&pattern(), &json!({}), &body, &opts, &policy, |_| {}).unwrap();
    assert_eq!(run.report["two_start"]["status"], "REVIEW_REQUIRED");
    assert!(run.gating().iter().any(|s| s.contains("has not settled")));
    assert!(
        run.report["tightness_gating"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| !s.as_str().unwrap().starts_with("two-start"))
    );
}

#[test]
fn preflight_rejects_identical_paths_before_progress_or_meshing() {
    let empty = Vec::new();
    let triangles = Vec::new();
    let body = DrapeBody {
        frames: &Form,
        collision: (&empty, &triangles),
        display: (&empty, &triangles),
        arm_deg: 35.0,
    };
    let opts = DrapeOptions::default();
    let policy = TwoStartOptions {
        alternate_sew_steps: opts.sew_steps,
        ..TwoStartOptions::default()
    };
    let mut called = false;
    let result = two_start::drape(
        &Pattern::new(json!({})),
        &json!({}),
        &body,
        &opts,
        &policy,
        |_| called = true,
    );
    assert!(result.is_err());
    assert!(!called);
}

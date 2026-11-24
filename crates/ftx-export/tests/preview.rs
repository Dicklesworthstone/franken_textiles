//! Diagnostic geometry uses pinned public-reference patterns, not candidate-generated oracles.

use std::path::Path;

use ftx_export::diagnostic_svg;
use ftx_pattern::{Measurements, Pattern, Report, alter, validate};
use serde_json::{Value, json};

fn fixture(id: &str) -> (Value, Value, Measurements) {
    let doc: Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/oracle/draft/programs.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let c = doc["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    (
        c["output"]["pattern"].clone(),
        c["output"]["fit_spec"].clone(),
        Measurements(c["input"]["measurements"].clone()),
    )
}

#[test]
fn every_physical_instance_is_shown_without_modifying_pattern_or_report() {
    for kind in ["fez", "zouave_bolero", "zouave_trousers"] {
        let (doc, fit, m) = fixture(&format!("{kind}-920"));
        let pattern = Pattern::new(doc.clone());
        let report = validate(&pattern, &fit, &m);
        let before = report.clone();
        for dark in [false, true] {
            let svg = diagnostic_svg(&pattern, &report, dark).unwrap();
            assert_eq!(
                svg.matches("data-instance=").count(),
                pattern.instances.len()
            );
            assert!(!svg.contains("data-failing=\"true\""));
            assert!(svg.contains("not for cutting"));
            assert!(!svg.contains("width=\"920mm\""));
            assert!(!svg.contains("<script"));
            assert!(!svg.contains("NaN"));
            assert_eq!(svg, diagnostic_svg(&pattern, &report, dark).unwrap());
        }
        assert_eq!(report, before);
        assert_eq!(pattern.raw(), &doc);
    }
}

#[test]
fn both_mirrored_seams_turn_red_on_damage_and_clear_on_repair() {
    let (doc, fit, m) = fixture("zouave_bolero-920");
    let damaged = alter::move_vertex(&doc, "front", "side_hem", 0.0, -20.0).unwrap();
    let pattern = Pattern::new(damaged.clone());
    let report = validate(&pattern, &fit, &m);
    let svg = diagnostic_svg(&pattern, &report, false).unwrap();
    for seam in ["side_R", "side_L"] {
        for side in ["a", "b"] {
            let marker = format!("data-seam=\"{seam}\" data-side=\"{side}\" data-failing=\"true\"");
            assert!(svg.contains(&marker), "missing {marker}");
        }
    }
    // Length-only truing can leave the hem corners wrong. Those must stay visibly red.
    let length_fixed = Pattern::new(alter::true_seam(&damaged, "side_R", "start").unwrap());
    let report = validate(&length_fixed, &fit, &m);
    let Report::Full { seams, truing, .. } = &report else {
        panic!()
    };
    assert!(
        seams
            .iter()
            .filter(|s| s.seam == "side_R" || s.seam == "side_L")
            .all(|s| s.status == "PASS")
    );
    assert!(
        truing
            .iter()
            .any(|s| s.seam == "side_R" && s.status == "FAIL")
    );
    assert!(
        diagnostic_svg(&length_fixed, &report, false)
            .unwrap()
            .contains("data-seam=\"side_R\" data-side=\"a\" data-failing=\"true\"")
    );
    let fixed = alter::move_vertex(&damaged, "front", "side_hem", 0.0, 20.0).unwrap();
    let pattern = Pattern::new(fixed);
    let fixed_svg = diagnostic_svg(&pattern, &validate(&pattern, &fit, &m), false).unwrap();
    for line in fixed_svg
        .lines()
        .filter(|l| l.contains("data-seam=\"side_"))
    {
        assert!(line.contains("data-failing=\"false\""), "{line}");
    }
}

#[test]
fn truing_failures_are_visible_even_with_a_passing_seam_walk() {
    let (doc, fit, m) = fixture("zouave_bolero-920");
    let pattern = Pattern::new(doc);
    let mut report = validate(&pattern, &fit, &m);
    let Report::Full { seams, truing, .. } = &mut report else {
        panic!()
    };
    assert!(seams.iter().all(|s| s.status == "PASS"));
    let id = truing[0].seam.clone();
    truing[0].status = "FAIL".into();
    let svg = diagnostic_svg(&pattern, &report, false).unwrap();
    for side in ["a", "b"] {
        assert!(svg.contains(&format!(
            "data-seam=\"{}\" data-side=\"{side}\" data-failing=\"true\"",
            id.as_str().unwrap()
        )));
    }
}

#[test]
fn blocked_and_unscaled_inputs_do_not_look_like_manufacturing_patterns() {
    let (mut doc, fit, m) = fixture("fez-920");
    doc["meta"]["scale_status"] = json!("UNSCALED");
    let pattern = Pattern::new(doc.clone());
    let svg = diagnostic_svg(&pattern, &validate(&pattern, &fit, &m), false).unwrap();
    assert!(svg.contains("UNSCALED"));
    assert!(svg.contains("no physical dimensions"));
    assert!(!svg.contains("mm\""));
    doc["pieces"][0]["edges"][0]["from"] = json!("<script>alert(1)</script>");
    let pattern = Pattern::new(doc);
    let svg = diagnostic_svg(&pattern, &validate(&pattern, &fit, &m), true).unwrap();
    assert!(svg.contains("BLOCKED"));
    assert!(svg.contains("&lt;script&gt;"));
    assert!(!svg.contains("<script>"));
    assert!(!svg.contains("data-instance="));
}

#[test]
fn diagnostic_svg_renders_internal_and_seam_creases_on_physical_instances() {
    let doc = json!({
        "schema": "fabric-pattern/1",
        "units": "mm",
        "meta": {"title": "Creased test", "sa_mm": 10},
        "pieces": [
            {
                "id": "front",
                "name": "FRONT",
                "cut": {"qty": 2},
                "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 80], "d": [0, 80]},
                "edges": [
                    {"id": "bot", "from": "a", "to": "b"},
                    {"id": "side", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "cf", "from": "d", "to": "a"}
                ],
                "grain": [[20, 10], [20, 70]],
                "internal": [
                    {"kind": "draw", "pts": [[30, 10], [30, 70]], "label": "PLEAT <A>", "crease_angle_deg": 45.0},
                    {"kind": "draw", "pts": [[60, 20], [80, 20], [70, 60]], "closed": true, "crease_angle_deg": -60.5},
                    {"kind": "draw", "pts": [[10, 10], [10, 70]], "crease_angle_deg": 0.0},
                    {"kind": "cut", "pts": [[50, 10], [50, 70]], "crease_angle_deg": 30.0}
                ]
            },
            {
                "id": "back",
                "name": "BACK",
                "cut": {"qty": 1, "on_fold": "cb"},
                "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 80], "d": [0, 80]},
                "edges": [
                    {"id": "bot", "from": "a", "to": "b"},
                    {"id": "side", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "cb", "from": "d", "to": "a"}
                ],
                "grain": [[20, 10], [20, 70]],
                "internal": [
                    {"kind": "draw", "pts": [[40, 10], [40, 70]], "crease_angle_deg": -35.0}
                ]
            }
        ],
        "seams": [
            {"id": "side_R", "a": "front.R/side", "b": "back/side.R", "reverse": true, "crease_angle_deg": 45.0},
            {"id": "side_L", "a": "front.L/side", "b": "back/side.L", "reverse": true, "crease_angle_deg": 0.0}
        ]
    });
    let pattern = Pattern::new(doc);
    let report = validate(&pattern, &json!({"points": []}), &Measurements(json!({})));
    for dark in [false, true] {
        let svg = diagnostic_svg(&pattern, &report, dark).unwrap();
        // front.R and front.L each get 2 valid internal creases (valley + closed mountain);
        // back is unfolded on_fold so its off-fold crease appears on both halves (2 paths).
        assert_eq!(svg.matches("data-crease-instance=\"front.R\"").count(), 2);
        assert_eq!(svg.matches("data-crease-instance=\"front.L\"").count(), 2);
        assert_eq!(svg.matches("data-crease-instance=\"back\"").count(), 2);
        assert!(svg.contains("data-crease-angle-deg=\"+45\" data-crease-fold=\"valley\""));
        assert!(svg.contains("data-crease-angle-deg=\"-60.5\" data-crease-fold=\"mountain\""));
        assert!(svg.contains("data-crease-angle-deg=\"-35\" data-crease-fold=\"mountain\""));
        assert!(svg.contains("PLEAT &lt;A&gt;: CREASE +45 deg (valley)"));
        assert!(!svg.contains("PLEAT <A>"));
        // Cut mark crease and 0-degree crease are not emitted as crease paths.
        assert!(!svg.contains("data-crease-angle-deg=\"+30\""));
        assert!(!svg.contains("data-crease-angle-deg=\"+0\""));
        // Creased seam carries angle attribute and title; 0-degree seam does not.
        assert!(svg.contains("data-seam=\"side_R\" data-side=\"a\" data-failing=\"false\" data-crease-angle-deg=\"+45\""));
        assert!(svg.contains("front.R/side (crease +45 deg (valley))"));
        assert!(!svg.contains(
            "data-seam=\"side_L\" data-side=\"a\" data-failing=\"false\" data-crease-angle-deg="
        ));
        assert_eq!(svg, diagnostic_svg(&pattern, &report, dark).unwrap());
    }
}

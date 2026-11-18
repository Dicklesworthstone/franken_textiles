//! Measured parametric drafting (`docs/spec/02` Parts B–C): the reference's analytic body and its three
//! programs, each `program(measurements, style) → (pattern, fit spec)`. The body owns size; the style owns
//! character.

pub mod bolero;
pub mod builder;
pub mod fez;
pub mod form;
pub mod geos;
pub mod trousers;

use ftx_core::{Refusal, RefusalCode, Vec3};
use serde_json::Value;

/// The programs, in the reference's registry order (`PROGRAMS`).
pub const PROGRAMS: [&str; 3] = ["zouave_bolero", "zouave_trousers", "fez"];

/// Dimensions and placement geometry from one explicitly selected drafting body.
/// Values, frame origins and landmark heights are millimetres. A body source must refuse
/// unavailable geometry; programs do not substitute analytic sections or other measurements.
pub trait DraftBody {
    /// Whether dimensions come from the analytic reference form, with its original drafting rules.
    fn is_analytic_form(&self) -> bool {
        false
    }

    /// A named dimension in millimetres.
    ///
    /// # Errors
    /// The selected source cannot supply the dimension.
    fn get(&self, name: &str) -> Result<f64, Refusal>;

    /// A named landmark's height in the requested part frame.
    ///
    /// # Errors
    /// The part or landmark is unavailable.
    fn landmark_h(&self, part: &str, name: &str) -> Result<f64, Refusal>;

    /// The part frame's origin in body coordinates.
    ///
    /// # Errors
    /// The part frame is unavailable.
    fn frame_origin(&self, part: &str) -> Result<Vec3, Refusal>;

    /// Largest distance from a part axis to the selected body's section at this height.
    ///
    /// # Errors
    /// The body has no usable section at the requested height.
    fn section_radius(&self, part: &str, h_mm: f64) -> Result<f64, Refusal>;
}

struct CheckedBody<'a>(&'a dyn DraftBody);

fn positive(value: f64, what: &str) -> Result<f64, Refusal> {
    if !value.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            format!("{what} must be finite"),
        ));
    }
    if value <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("{what} must be positive"),
        ));
    }
    Ok(value)
}

impl DraftBody for CheckedBody<'_> {
    fn is_analytic_form(&self) -> bool {
        self.0.is_analytic_form()
    }

    fn get(&self, name: &str) -> Result<f64, Refusal> {
        positive(self.0.get(name)?, &format!("drafting dimension '{name}'"))
    }

    fn landmark_h(&self, part: &str, name: &str) -> Result<f64, Refusal> {
        let value = self.0.landmark_h(part, name)?;
        if !value.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("drafting landmark '{part}/{name}' must be finite"),
            ));
        }
        Ok(value)
    }

    fn frame_origin(&self, part: &str) -> Result<Vec3, Refusal> {
        let origin = self.0.frame_origin(part)?;
        if !origin.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!("drafting frame '{part}' must be finite"),
            ));
        }
        Ok(origin)
    }

    fn section_radius(&self, part: &str, h_mm: f64) -> Result<f64, Refusal> {
        if !h_mm.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "drafting section height must be finite",
            ));
        }
        positive(
            self.0.section_radius(part, h_mm)?,
            &format!("drafting section radius for '{part}'"),
        )
    }
}

/// Runs a drafting program against an explicitly selected body source.
/// The legacy measurement-document entry point remains unchanged.
///
/// # Errors
/// Missing dimensions or geometry, non-finite or nonpositive dimensions/sections, invalid style,
/// unknown program, or a drafting solve that cannot construct the requested geometry.
pub fn draft_on_body(
    program: &str,
    body: &dyn DraftBody,
    style: Option<&Value>,
) -> Result<(Value, Value), Refusal> {
    let body = CheckedBody(body);
    match program {
        "zouave_bolero" => bolero::on_body(&body, style),
        "zouave_trousers" => trousers::on_body(&body, style),
        "fez" => fez::on_body(&body, style),
        _ => Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown program {program:?}"),
        )
        .repair(format!("programs: {}", PROGRAMS.join(", ")))),
    }
}

/// Runs a drafting program on a measurement document (`fabric-measurements/1` or flat) and style overrides.
///
/// # Errors
/// `unknown_parameter` for an unknown program or style key; `out_of_range` for a fez style out of its
/// ranges; `invalid_document` for a missing measurement; `degenerate_geometry` when the draft cannot be built.
pub fn draft(
    program: &str,
    meas: &Value,
    style: Option<&Value>,
) -> Result<(Value, Value), Refusal> {
    match program {
        "zouave_bolero" => bolero::zouave_bolero(meas, style),
        "zouave_trousers" => trousers::zouave_trousers(meas, style),
        "fez" => fez::fez(meas, style),
        _ => Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown program {program:?}"),
        )
        .repair(format!("programs: {}", PROGRAMS.join(", ")))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    struct SelectedBody {
        values: BTreeMap<&'static str, f64>,
        landmarks: BTreeMap<&'static str, f64>,
        leg_origin: Vec3,
        radius: f64,
    }

    fn missing(name: &str) -> Refusal {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("source has no {name}"),
        )
    }

    impl DraftBody for SelectedBody {
        fn get(&self, name: &str) -> Result<f64, Refusal> {
            self.values.get(name).copied().ok_or_else(|| missing(name))
        }

        fn landmark_h(&self, part: &str, name: &str) -> Result<f64, Refusal> {
            let key = format!("{part}/{name}");
            self.landmarks
                .get(key.as_str())
                .copied()
                .ok_or_else(|| missing(&key))
        }

        fn frame_origin(&self, part: &str) -> Result<Vec3, Refusal> {
            if part == "leg_R" {
                Ok(self.leg_origin)
            } else {
                Err(missing(part))
            }
        }

        fn section_radius(&self, part: &str, _: f64) -> Result<f64, Refusal> {
            if matches!(part, "arm_R" | "leg_R") {
                Ok(self.radius)
            } else {
                Err(missing(part))
            }
        }
    }

    fn selected() -> SelectedBody {
        // Public reference dimensions, deliberately without analytic width, depth or slope inputs.
        SelectedBody {
            values: BTreeMap::from([
                ("stature", 920.0),
                ("chest", 540.0),
                ("neck_base", 251.2),
                ("shoulder_length", 80.0),
                ("back_neck_to_waist", 225.0),
                ("across_back", 225.7),
                ("upper_arm", 156.8),
                ("arm_length", 320.0),
                ("hand_pass", 142.7),
                ("wrist", 109.8),
                ("head", 483.8),
                ("hip", 580.0),
                ("waist", 530.0),
                ("body_rise", 172.5),
                ("total_crotch_length", 396.6),
                ("calf", 204.5),
                ("foot_pass", 205.7),
            ]),
            landmarks: BTreeMap::from([
                ("torso/waist", 530.0),
                ("torso/chest", 640.0),
                ("torso/across_back", 675.0),
                ("torso/hip", 455.0),
                ("torso/calf", 180.0),
                ("leg_R/waist", 160.0),
            ]),
            leg_origin: Vec3::new(-30.0, 0.0, 400.0),
            radius: 100.0,
        }
    }

    fn piece<'a>(pattern: &'a Value, id: &str) -> &'a Value {
        pattern["pieces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|piece| piece["id"] == id)
            .unwrap()
    }

    #[test]
    fn selected_body_controls_bodice_levels_and_sleeve_reach() {
        let body = selected();
        let (pattern, _) =
            draft_on_body("zouave_bolero", &body, Some(&json!({"soutache": false}))).unwrap();
        assert_eq!(piece(&pattern, "back")["vertices"]["ua"][1], json!(98.0));
        assert_eq!(
            piece(&pattern, "sleeve")["placement"]["radius_mm"],
            json!(106.0)
        );
        assert!(!body.values.contains_key("shoulder_slope_deg"));
        assert!(!body.values.contains_key("chest_width"));
    }

    #[test]
    fn fitted_bolero_allows_for_the_cutaway_at_the_finished_chest() {
        let mut body = selected();
        body.values.insert("chest", 507.4);
        body.values.insert("back_neck_to_waist", 238.4);
        body.landmarks.insert("torso/chest", 611.832);
        let style = json!({"soutache":false,"front_cutaway":75.0});
        let finished_ease = |style: &Value| {
            let (doc, fit) = draft_on_body("zouave_bolero", &body, Some(style)).unwrap();
            let chest = &fit["points"][0];
            let y = body.landmarks["torso/chest"] - body.landmarks["torso/waist"]
                + chest["dy"].as_f64().unwrap();
            let pattern = crate::Pattern::new(doc);
            let total: f64 = chest["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    let panel = pattern.instance(id.as_str().unwrap()).unwrap();
                    ftx_core::polyline::horizontal_width(
                        &panel.outline(ftx_core::curve::MEASURE_DENSE).unwrap(),
                        y,
                    )
                })
                .sum();
            total - body.values["chest"]
        };
        assert!((finished_ease(&style) - 100.0).abs() < 0.05);
        let mut tight = style;
        tight["chest_ease"] = json!(10.0);
        assert!((finished_ease(&tight) - 10.0).abs() < 0.05);
    }

    #[test]
    fn selected_body_controls_cuff_height_leg_axis_and_wrap_radius() {
        let body = selected();
        let (pattern, _) = draft_on_body("zouave_trousers", &body, None).unwrap();
        let front = piece(&pattern, "tr_front");
        assert_eq!(front["vertices"]["hem_side"][1], json!(-375.0));
        assert_eq!(front["placement"]["radius_mm"], json!(122.0));
        assert_eq!(
            front["placement"]["theta_deg"],
            json!(ftx_core::py::round(
                ftx_core::det::asin(30.0 / 122.0).to_degrees(),
                2
            ))
        );
        assert_eq!(
            pattern["meta"]["solved"]["leg_to_cuff_top_mm"],
            json!(375.0)
        );
    }

    #[test]
    fn fitted_trousers_walk_the_requested_crotch_even_outside_the_reference_bracket() {
        let mut body = selected();
        body.values.insert("hip", 569.855);
        body.values.insert("body_rise", 132.845);
        body.landmarks.insert("torso/hip", 456.935);
        for (body_length, longer) in [(403.490, true), (304.0, false)] {
            body.values.insert("total_crotch_length", body_length);
            let (raw, _) = draft_on_body("zouave_trousers", &body, None).unwrap();
            let ext = raw["meta"]["solved"]["back_crotch_ext_mm"]
                .as_f64()
                .unwrap();
            if longer {
                assert!(ext > 0.20 * body.values["hip"]);
            } else {
                assert!(ext > 0.0 && ext < 0.04 * body.values["hip"]);
            }
            let pattern = crate::Pattern::new(raw);
            let front = pattern.instance("tr_front.R").unwrap();
            let back = pattern.instance("tr_back.R").unwrap();
            let walked = front.edge_length("cf").unwrap()
                + front.edge_length("crotch").unwrap()
                + back.edge_length("cb").unwrap()
                + back.edge_length("crotch").unwrap();
            assert!((walked - (body_length + 30.0)).abs() <= 0.01, "{walked}");
            for edge in ["inseam", "outseam"] {
                assert!(
                    (front.edge_length(edge).unwrap() - back.edge_length(edge).unwrap()).abs()
                        < 0.05
                );
            }
        }
        body.values.insert("total_crotch_length", 200.0);
        let error = draft_on_body("zouave_trousers", &body, None).unwrap_err();
        assert_eq!(error.code, RefusalCode::NoRoot);
        assert!(error.message.contains("230.00") && !error.repairs.is_empty());
    }

    #[test]
    fn head_only_source_drafts_and_missing_passage_is_not_filled_in() {
        let mut body = selected();
        body.values = BTreeMap::from([("head", 400.0)]);
        let (pattern, _) = draft_on_body("fez", &body, None).unwrap();
        assert_eq!(pattern["meta"]["solved"]["base_girth_mm"], json!(410.0));
        assert_eq!(pattern["meta"]["size_label"], json!("custom"));

        let mut body = selected();
        body.values.remove("hand_pass");
        let error =
            draft_on_body("zouave_bolero", &body, Some(&json!({"soutache": false}))).unwrap_err();
        assert!(error.to_string().contains("hand_pass"));
        let mut body = selected();
        body.values.remove("foot_pass");
        assert!(
            draft_on_body("zouave_trousers", &body, None)
                .unwrap_err()
                .to_string()
                .contains("foot_pass")
        );
    }

    #[test]
    fn selected_body_refuses_invalid_dimensions_landmarks_frames_and_sections() {
        let mut body = selected();
        body.values.insert("head", f64::NAN);
        assert!(draft_on_body("fez", &body, None).is_err());
        body.values.insert("head", 0.0);
        assert!(draft_on_body("fez", &body, None).is_err());

        let mut body = selected();
        body.landmarks.insert("torso/chest", f64::INFINITY);
        assert!(draft_on_body("zouave_bolero", &body, None).is_err());

        let mut body = selected();
        body.leg_origin.x = f64::NAN;
        assert!(draft_on_body("zouave_trousers", &body, None).is_err());

        let mut body = selected();
        body.radius = 0.0;
        assert!(draft_on_body("zouave_trousers", &body, None).is_err());
        body.radius = f64::INFINITY;
        assert!(draft_on_body("zouave_trousers", &body, None).is_err());
        assert!(
            CheckedBody(&selected())
                .section_radius("leg_R", f64::NAN)
                .is_err()
        );
    }

    #[test]
    fn analytic_adapter_preserves_all_three_programs_for_reference_wearers() {
        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/oracle/draft/body.json"
        )))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap().iter().take(3) {
            let measurements = &case["input"]["measurements"];
            for program in PROGRAMS {
                assert_eq!(
                    draft(program, measurements, None).unwrap(),
                    draft_on_body(program, &form::Meas(measurements), None).unwrap(),
                    "{} / {program}",
                    case["id"]
                );
            }
        }
    }
}

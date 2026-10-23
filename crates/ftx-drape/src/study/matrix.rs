//! Bounded named operating cases for one frozen pattern and one fixed body shape.
//! Each case uses the ordinary scenario session; only work accounting is shared.

use std::collections::BTreeSet;

use ftx_core::Refusal;
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Scenario, Session, budget, heat, invalid};
use crate::study_input::PreparedInput;

/// One explicitly authored environment, numerical policy and optional motion program.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Nonempty unique case identity, at most 128 UTF-8 bytes.
    pub key: String,
    /// Existing production scenario, with unchanged physical and source-data admission.
    pub scenario: Scenario,
    /// Optional application-level comparison policy. The export comparator validates this value;
    /// this numerical crate never interprets preferences or manufactures a weighted score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<Value>,
}

/// Named sensitivities on the same pattern and fixed body. Initial body fitting is shared;
/// cloth, constitutive memory, coverage and temperatures always start fresh in each evaluation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-drape-scenario-matrix/1`.
    pub schema: String,
    /// Maximum case × alternative count, in 1..=64, checked before any case callback starts.
    pub max_evaluations: usize,
    /// One to eight cases, evaluated in this exact declared order.
    pub cases: Vec<Case>,
}

impl Request {
    /// Validate all cases before fitting, meshing or solving. Per-case vertex-frame allocations
    /// must sum to at most the ordinary 64000000 study allowance; motion trials share 32768.
    ///
    /// # Errors
    /// Unknown schema, duplicate/empty cases, invalid scenarios or excessive aggregate allocations.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-drape-scenario-matrix/1" {
            return Err(invalid("expected ftx-drape-scenario-matrix/1"));
        }
        if !(1..=8).contains(&self.cases.len())
            || !(1..=64).contains(&self.max_evaluations)
            || self.cases.len() > self.max_evaluations
        {
            return Err(budget(
                "scenario matrix needs 1..8 cases and a 1..64 evaluation allowance covering every case",
            ));
        }
        let mut keys = BTreeSet::new();
        let mut frames = 0usize;
        let mut trials = 0usize;
        for case in &self.cases {
            if case.key.trim().is_empty() || case.key.len() > 128 || !keys.insert(&case.key) {
                return Err(invalid(
                    "scenario matrix case keys must be unique and nonempty, at most 128 bytes",
                ));
            }
            case.scenario.validate()?;
            frames = frames.saturating_add(case.scenario.max_vertex_frames);
            if case.scenario.motion.is_some() {
                trials = trials.saturating_add(case.scenario.max_motion_trials);
            }
        }
        if frames > 64_000_000 || trials > 32_768 {
            return Err(budget(
                "scenario matrix allocations exceed 64000000 vertex frames or 32768 motion trials across cases",
            ));
        }
        Ok(())
    }

    /// Compose the original pattern/outfit once for manufacturing. Every case must independently
    /// admit the same source under its own options and yield that exact frozen geometry/materials.
    ///
    /// # Errors
    /// Invalid matrix/source, an outfit refusal in any case, or case-dependent source composition.
    pub fn prepare(&self, source: Value) -> Result<PreparedInput, Refusal> {
        self.validate()?;
        let first = PreparedInput::new(source.clone(), &self.cases[0].scenario.options)?;
        for case in &self.cases[1..] {
            let prepared = PreparedInput::new(source.clone(), &case.scenario.options)?;
            if prepared.pattern().raw() != first.pattern().raw() {
                return Err(invalid(
                    "scenario matrix cases must preserve one identical prepared pattern",
                ));
            }
        }
        Ok(first)
    }
}

/// Cross-case reservations for retained motion geometry and independent heat/vapor work.
/// No physical state, material history or solver start is shared between sessions.
#[derive(Default)]
pub struct Budget {
    heat: heat::Budget,
    retained_motion_vertices: usize,
}

impl Budget {
    /// Run one ordinary fresh evaluation while charging the shared work allowances, including
    /// refused attempts. Mechanical frame/trial allowances remain the pre-admitted case portions.
    ///
    /// # Errors
    /// The ordinary scenario refusals, with heat and retained-motion budgets shared across cases.
    pub fn evaluate(
        &mut self,
        session: &mut Session<'_, '_>,
        pattern: &Pattern,
        progress: impl FnMut(&str),
    ) -> Result<Value, Refusal> {
        std::mem::swap(&mut self.heat, &mut session.heat_budget);
        std::mem::swap(
            &mut self.retained_motion_vertices,
            &mut session.charged_motion_vertices,
        );
        let result = session.evaluate(pattern, progress);
        std::mem::swap(&mut self.heat, &mut session.heat_budget);
        std::mem::swap(
            &mut self.retained_motion_vertices,
            &mut session.charged_motion_vertices,
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PartFrame, run::DrapeBody, scene::PartFrames};
    use ftx_core::{RefusalCode, Vec3};
    use serde_json::json;

    fn scenario() -> Scenario {
        let mut scenario: Scenario = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario.json"
        ))
        .unwrap();
        scenario.max_vertex_frames = 32_000_000;
        scenario.max_motion_trials = 16_384;
        scenario.motion = Some(
            serde_json::from_value(json!({"schema":"ftx-drape-motion/1",
                "keyframes":[{"time_s":0.001,"arm_abduction_deg":35.}],
                "max_step_s":0.001,"max_trials":1,"max_pairs":100}))
            .unwrap(),
        );
        scenario
    }

    #[test]
    fn case_frame_and_active_trial_allocations_obey_exact_aggregate_limits() {
        let mut request = Request {
            schema: "ftx-drape-scenario-matrix/1".into(),
            max_evaluations: 2,
            cases: ["first", "second"]
                .into_iter()
                .map(|key| Case {
                    key: key.into(),
                    scenario: scenario(),
                    comparison: None,
                })
                .collect(),
        };
        request.validate().unwrap();
        request.cases[1].scenario.max_vertex_frames += 1;
        assert_eq!(
            request.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        request.cases[1].scenario.max_vertex_frames -= 1;
        request.cases[1].scenario.max_motion_trials += 1;
        assert_eq!(
            request.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        request.cases[1].scenario.motion = None;
        request.cases[1].scenario.max_motion_trials = 32_768;
        request.validate().unwrap();
    }

    struct UnusedFrames;
    impl PartFrames for UnusedFrames {
        fn frame(&self, _: &str) -> Option<PartFrame> {
            None
        }
        fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
            None
        }
        fn section_radius(&self, _: &str, _: f64) -> f64 {
            0.
        }
    }

    #[test]
    fn refused_attempts_keep_shared_reservations_and_separate_case_counters() {
        let scenario = scenario();
        let positions = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        let triangles = [[0, 1, 2]];
        let body = DrapeBody {
            frames: &UnusedFrames,
            collision: (&positions, &triangles),
            display: (&positions, &triangles),
            arm_deg: 35.,
        };
        let mut source: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
        ))
        .unwrap();
        // A real source admission refusal occurs after the work reservation, before any solve.
        source["fabric"]["material"]["record"]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("warp_stiffness");
        let pattern = Pattern::new(source);
        let mut first = Session::new(&scenario, &body).unwrap();
        let mut second = Session::new(&scenario, &body).unwrap();
        let mut work = Budget::default();
        work.heat.reserve_vapor_geometry(40_000_000).unwrap();
        assert!(work.evaluate(&mut first, &pattern, |_| {}).is_err());
        let retained = work.retained_motion_vertices;
        assert!(retained > 0);
        assert_eq!(first.charged_motion_vertices, 0);
        assert!(work.evaluate(&mut second, &pattern, |_| {}).is_err());
        assert_eq!(work.retained_motion_vertices, 2 * retained);
        assert_eq!(second.charged_motion_vertices, 0);
        assert!(first.charged_vertex_frames > 0);
        assert_eq!(first.charged_vertex_frames, second.charged_vertex_frames);
        assert_eq!(first.charged_motion_frames, second.charged_motion_frames);
        assert_eq!(first.charged_motion_trials, 1);
        assert_eq!(second.charged_motion_trials, 1);
        assert!(work.heat.reserve_vapor_geometry(1).is_err());
        first
            .heat_budget
            .reserve_vapor_geometry(40_000_000)
            .unwrap();
        second
            .heat_budget
            .reserve_vapor_geometry(40_000_000)
            .unwrap();
    }
}

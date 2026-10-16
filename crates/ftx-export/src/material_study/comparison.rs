//! Declared Pareto objectives over production study outputs; no surrogate solver or weighted score.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::{material_catalog::property_unit, material_screen::Direction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Alternative, Request, hash, invalid};

/// Uncertainty admission. Source intervals cannot be propagated through planners by relabeling them.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Explicitly compare nominal computed results, without a propagated uncertainty claim.
    Nominal,
}

/// Consumer whose successful completion can be required independently of the chosen objectives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consumer {
    /// Production roll cutting, not hypothetical net pattern area.
    Cutting,
    /// Production seam/stack/recipe admission, not physical seam qualification.
    Sewing,
    /// Complete marker-derived whole-order offer coverage in the request's dated market scope.
    Purchasing,
}

/// Fixed-dimension output names; values are read only from the actual corresponding consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Gross stock including marker waste/margins, in square metres.
    StockAreaM2,
    /// Outer cut area, in square metres; holes are not assumed recovered.
    ExternalCutAreaM2,
    /// Complete gross stock mass, in kilograms, never a partial sum.
    StockMassKg,
    /// Least observed independent whole-order total in the explicit market currency.
    ObservedOrderCost,
    /// Traversed stitch intervals, including declared locks and repeated passes.
    StitchIntervals,
    /// Feed-only time in seconds; not a robot cycle-time estimate.
    FeedSeconds,
}

impl Metric {
    fn consumer(self) -> Consumer {
        match self {
            Self::StockAreaM2 | Self::ExternalCutAreaM2 | Self::StockMassKg => Consumer::Cutting,
            Self::ObservedOrderCost => Consumer::Purchasing,
            Self::StitchIntervals | Self::FeedSeconds => Consumer::Sewing,
        }
    }
    fn unit(self, currency: Option<&str>) -> &str {
        match self {
            Self::StockAreaM2 | Self::ExternalCutAreaM2 => "m2",
            Self::StockMassKg => "kg",
            Self::ObservedOrderCost => currency.unwrap_or("unavailable currency"),
            Self::StitchIntervals => "count",
            Self::FeedSeconds => "s",
        }
    }
    fn value(self, row: &Alternative) -> Option<f64> {
        match self {
            Self::StockAreaM2 => row.metrics.stock_area_m2,
            Self::ExternalCutAreaM2 => row.metrics.external_cut_area_m2,
            Self::StockMassKg => row.metrics.stock_mass_kg,
            Self::ObservedOrderCost => row.metrics.least_observed_independent_order_total,
            Self::StitchIntervals => row.metrics.stitch_intervals.map(|n| n as f64),
            Self::FeedSeconds => row.metrics.feed_seconds,
        }
    }
}

/// One independent preference; every objective must be available for a row to be eligible.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    /// Computed output and its fixed physical dimension.
    pub metric: Metric,
    /// Smaller or larger, explicitly chosen even for signed quantities.
    pub direction: Direction,
}

/// Inclusive constraints on computed outputs, in the metric's fixed units.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    /// Actual consumer output to constrain.
    pub metric: Metric,
    /// Inclusive lower bound; at least one bound is required.
    pub minimum: Option<f64>,
    /// Inclusive upper bound; no unit inference or clamping is performed.
    pub maximum: Option<f64>,
}

/// An opt-in nominal comparison of alternatives evaluated in ONE study and process context.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Required explicit admission of nominal (not uncertainty-propagated) computed objectives.
    pub basis: Basis,
    /// One to sixteen unique objective metrics. No hidden scalarization or automatic winner.
    pub objectives: Vec<Objective>,
    /// At most 64 unique-metric inclusive constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Additional consumers required even when they contribute no objective.
    #[serde(default)]
    pub required_consumers: Vec<Consumer>,
    /// False keeps different reported input methods/conditions/evidence incomparable. True is an
    /// explicit relaxation, not evidence of experimental comparability or permission for estimates.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
}

/// One admitted numerical output. The source plans and observations remain in the containing study.
#[derive(Clone, Debug, Serialize)]
pub struct Reading {
    /// Finite computed value, including genuine zero; unknown values have no reading.
    pub value: f64,
    /// Fixed physical unit or exact currency; no conversions occur in the comparator.
    pub unit: String,
    /// Required production consumer, not a value copied from scalar screening.
    pub consumer: Consumer,
}

/// Explicit eligibility and dominance, independent of the source report's consumer outcomes.
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    /// Stable alternative key.
    pub key: String,
    /// Available objective/constraint readings; missing values are not inserted as zero.
    pub readings: BTreeMap<Metric, Reading>,
    /// Required consumer, missing metric or failed constraint reasons.
    pub exclusions: Vec<Refusal>,
    /// All requested objectives, constraints and additional consumers were admitted.
    pub eligible: bool,
    /// Identity of relevant reported input protocols by source piece. A match is not validation.
    pub input_protocol_sha256: Option<String>,
    /// Eligible comparable alternatives no worse in all objectives and strictly better in at least one.
    pub dominated_by: Vec<String>,
}

/// An eligible pair which cannot be ordered under the chosen reported-protocol policy.
#[derive(Clone, Debug, Serialize)]
pub struct Incomparable {
    /// Stable keys in deterministic order.
    pub alternatives: [String; 2],
    /// Why numerical values alone did not establish dominance.
    pub reason: &'static str,
}

/// Nominal Pareto set among evaluated, eligible alternatives, never a global material optimum.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-material-comparison/1`.
    pub schema: &'static str,
    /// Exact source geometry/material document used by the containing study.
    pub source_pattern_sha256: String,
    /// Entire study/process policy identity, not merely the objective list.
    pub context_sha256: String,
    /// The user's complete comparison policy, including uncertainty and protocol admission.
    pub policy: Policy,
    /// Key-ordered candidate outcomes; excluded alternatives stay visible.
    pub alternatives: Vec<Candidate>,
    /// Eligible non-dominated keys; ties and incomparable candidates remain alternatives.
    pub frontier: Vec<String>,
    /// Explicit reported-protocol incompatibilities, not silently discarded comparisons.
    pub incomparable: Vec<Incomparable>,
    /// Always false. Reported input ranges are not propagated numerical-result intervals.
    pub uncertainty_propagated: bool,
    /// Always false, including when all computed constraints pass.
    pub physical_validation_claim: bool,
    /// Boundaries of the comparison and its source-data admission.
    pub notes: Vec<&'static str>,
}

impl Policy {
    pub(super) fn validate(&self, request: &Request) -> Result<(), Refusal> {
        if self.objectives.is_empty()
            || self.objectives.len() > 16
            || self.constraints.len() > 64
            || self.required_consumers.len() > 3
        {
            return Err(invalid(
                "comparison needs 1..16 objectives, at most 64 constraints and 3 consumers",
            ));
        }
        let mut metrics = BTreeSet::new();
        if self.objectives.iter().any(|o| !metrics.insert(o.metric)) {
            return Err(invalid("comparison objective metrics must be unique"));
        }
        metrics.clear();
        for c in &self.constraints {
            if !metrics.insert(c.metric)
                || (c.minimum.is_none() && c.maximum.is_none())
                || c.minimum.is_some_and(|x| !x.is_finite())
                || c.maximum.is_some_and(|x| !x.is_finite())
                || matches!((c.minimum, c.maximum), (Some(a), Some(b)) if a > b)
            {
                return Err(invalid(
                    "comparison constraints need unique metrics and finite ordered bounds",
                ));
            }
        }
        let unique: BTreeSet<_> = self.required_consumers.iter().collect();
        if unique.len() != self.required_consumers.len() {
            return Err(invalid("comparison required consumers must be unique"));
        }
        for consumer in self.consumers() {
            let requested = match consumer {
                Consumer::Cutting => request.cutting.is_some(),
                Consumer::Sewing => request.sewing.is_some(),
                Consumer::Purchasing => request.purchasing.is_some(),
            };
            if !requested {
                return Err(invalid(format!(
                    "comparison requires unrequested {consumer:?} consumer"
                )));
            }
        }
        Ok(())
    }

    fn metrics(&self) -> BTreeSet<Metric> {
        self.objectives
            .iter()
            .map(|o| o.metric)
            .chain(self.constraints.iter().map(|c| c.metric))
            .collect()
    }
    fn consumers(&self) -> BTreeSet<Consumer> {
        self.metrics()
            .iter()
            .map(|m| m.consumer())
            .chain(self.required_consumers.iter().copied())
            .collect()
    }

    pub(super) fn evaluate(
        &self,
        rows: &[Alternative],
        source: &str,
        context: &str,
        currency: Option<&str>,
    ) -> Report {
        let mut candidates: Vec<_> = rows
            .iter()
            .map(|row| self.candidate(row, currency))
            .collect();
        let mut incomparable = Vec::new();
        for i in 0..candidates.len() {
            for j in i + 1..candidates.len() {
                if !candidates[i].eligible || !candidates[j].eligible {
                    continue;
                }
                if !self.allow_mixed_protocols
                    && candidates[i].input_protocol_sha256 != candidates[j].input_protocol_sha256
                {
                    incomparable.push(Incomparable { alternatives: [candidates[i].key.clone(), candidates[j].key.clone()],
                        reason: "Relevant reported methods, conditions or evidence differ; no dominance inferred." });
                    continue;
                }
                if self.dominates(&candidates[i], &candidates[j]) {
                    let key = candidates[i].key.clone();
                    candidates[j].dominated_by.push(key);
                } else if self.dominates(&candidates[j], &candidates[i]) {
                    let key = candidates[j].key.clone();
                    candidates[i].dominated_by.push(key);
                }
            }
        }
        let frontier = candidates
            .iter()
            .filter(|r| r.eligible && r.dominated_by.is_empty())
            .map(|r| r.key.clone())
            .collect();
        Report {
            schema: "ftx-material-comparison/1",
            source_pattern_sha256: source.into(),
            context_sha256: context.into(),
            policy: self.clone(),
            alternatives: candidates,
            frontier,
            incomparable,
            uncertainty_propagated: false,
            physical_validation_claim: false,
            notes: vec![
                "Nominal computed outputs only; source bounds are not propagated uncertainty intervals. Estimate permission stays with each source selection.",
                "The frontier is conditional on the evaluated alternatives, constraints, process policy and reported-protocol admission. Scalar pre-screening may have removed better garment alternatives.",
                "Matching reported protocols is a software comparison rule, not proof that specimens or experiments are comparable. Missing or refused required outputs exclude a row without erasing other consumers.",
                "Stock mass includes waste, costs are dated complete independent order observations, and feed time excludes handling. No empirical fit, warmth, hardware authorization or global optimum is claimed.",
            ],
        }
    }

    fn candidate(&self, row: &Alternative, currency: Option<&str>) -> Candidate {
        let mut result = Candidate {
            key: row.key.clone(),
            readings: BTreeMap::new(),
            exclusions: Vec::new(),
            eligible: false,
            input_protocol_sha256: None,
            dominated_by: Vec::new(),
        };
        if row.binding.value().is_none() {
            result.exclusions.push(invalid(
                "comparison requires successful source-locked binding",
            ));
        }
        for consumer in self.consumers() {
            if !consumer_available(row, consumer) {
                result.exclusions.push(invalid(format!(
                    "comparison requires available {consumer:?} with complete required coverage"
                )));
            }
        }
        for metric in self.metrics() {
            match metric.value(row).filter(|v| v.is_finite() && *v >= 0.) {
                Some(value) if consumer_available(row, metric.consumer()) => {
                    result.readings.insert(
                        metric,
                        Reading {
                            value,
                            unit: metric.unit(currency).into(),
                            consumer: metric.consumer(),
                        },
                    );
                    for c in self.constraints.iter().filter(|c| c.metric == metric) {
                        if c.minimum.is_some_and(|min| value < min)
                            || c.maximum.is_some_and(|max| value > max)
                        {
                            result.exclusions.push(invalid(format!("{metric:?} = {value} {} violates the inclusive comparison constraint", metric.unit(currency))));
                        }
                    }
                }
                _ => result.exclusions.push(invalid(format!(
                    "{metric:?} is unavailable; no zero or partial value substituted"
                ))),
            }
        }
        match self.protocol(row) {
            Ok(protocol) => result.input_protocol_sha256 = Some(protocol),
            Err(e) => result.exclusions.push(e),
        }
        result.eligible = result.exclusions.is_empty();
        result
    }

    fn dominates(&self, a: &Candidate, b: &Candidate) -> bool {
        let mut strictly = false;
        for objective in &self.objectives {
            let x = a.readings[&objective.metric].value;
            let y = b.readings[&objective.metric].value;
            match objective.direction {
                Direction::Minimize if x > y => return false,
                Direction::Maximize if x < y => return false,
                _ => strictly |= x != y,
            }
        }
        strictly
    }

    fn protocol(&self, row: &Alternative) -> Result<String, Refusal> {
        let bound = row
            .binding
            .value()
            .ok_or_else(|| invalid("binding protocol unavailable"))?;
        let frozen = row
            .frozen_changes
            .as_ref()
            .ok_or_else(|| invalid("frozen source selections unavailable"))?;
        let consumers = self.consumers();
        let mut keys = BTreeSet::new();
        if consumers.contains(&Consumer::Cutting) || consumers.contains(&Consumer::Purchasing) {
            keys.insert("width");
        }
        if consumers.contains(&Consumer::Sewing) {
            keys.insert("thickness");
        }
        if self.metrics().contains(&Metric::StockMassKg) {
            keys.insert("areal_density");
        }
        let pieces = bound["pattern"]["pieces"]
            .as_array()
            .ok_or_else(|| invalid("bound source pieces unavailable"))?;
        let mut protocols = BTreeMap::new();
        for piece in pieces {
            let id = piece["id"]
                .as_str()
                .ok_or_else(|| invalid("bound source piece identity unavailable"))?;
            let selection = frozen
                .pieces
                .get(id)
                .or(frozen.default.as_ref())
                .ok_or_else(|| invalid("frozen piece selection unavailable"))?;
            let material = selection
                .record
                .as_ref()
                .ok_or_else(|| invalid("frozen source record unavailable"))?;
            let profile: BTreeMap<_, _> = keys
                .iter()
                .map(|&key| {
                    let p = material.properties.get(key).map_or(Value::Null, |q| {
                        json!({
                    "unit":property_unit(key).unwrap_or(&q.unit), "method":q.method,
                    "conditions":q.conditions, "evidence":q.evidence })
                    });
                    (key, p)
                })
                .collect();
            protocols.insert(id, profile);
        }
        hash(&protocols)
    }
}

fn consumer_available(row: &Alternative, consumer: Consumer) -> bool {
    match consumer {
        Consumer::Cutting => row.cutting.value().is_some(),
        Consumer::Sewing => row.sewing.value().is_some(),
        Consumer::Purchasing => row
            .purchasing
            .value()
            .is_some_and(|p| p["complete_offer_coverage"] == true),
    }
}

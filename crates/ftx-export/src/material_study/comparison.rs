//! Declared Pareto objectives over production study outputs; no surrogate solver or weighted score.
use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::{material_catalog::property_unit, material_screen::Direction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Alternative, Request, ScenarioAlternative, hash, invalid};

/// Uncertainty admission. Source intervals cannot be propagated through planners by relabeling them.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Explicitly compare nominal computed results, without a propagated uncertainty claim.
    Nominal,
    /// Require complete reported assembly-mass endpoints at fixed authored geometry/inventory.
    /// Other computed metrics have no supported output intervals and refuse this policy.
    ReportedBounds,
}

/// Consumer whose successful completion can be required independently of the chosen objectives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consumer {
    /// Production roll cutting, not hypothetical net pattern area.
    Cutting,
    /// Complete allocation on the shared, individually identified hide/remnant inventory.
    FiniteStock,
    /// Production seam/stack/recipe admission, not physical seam qualification.
    Sewing,
    /// Complete marker-derived whole-order offer coverage in the request's dated market scope.
    Purchasing,
    /// Complete declared pre-finishing assembly inventory, not a purchased or simulated subset.
    AssemblyMass,
    /// Fresh, geometrically settled, gating-clear production drape in the retained scenario.
    Drape,
    /// Successful independent sheet-column heat on an admitted drape, not whole-body heat.
    Surface,
    /// Successful declared body-patch/layer coverage heat on an admitted drape.
    Coverage,
    /// Complete prescribed motion starting at this admitted drape, not independent static poses.
    Motion,
    /// Complete independent-sheet temperature history on every admitted mechanical sample.
    TransientSurface,
    /// Complete moving body-domain/exposed-cloth history, with its explicit layer/film policy.
    TransientCoverage,
}

/// Fixed-dimension output names; values are read only from the actual corresponding consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Gross roll stock including marker waste/margins, in square metres.
    StockAreaM2,
    /// Outer cut area, in square metres; holes are not assumed recovered.
    ExternalCutAreaM2,
    /// Complete gross stock mass, in kilograms, never a partial sum.
    StockMassKg,
    /// All retained physical cut pieces plus sourced extras for ONE declared assembly (kg).
    AssemblyMassKg,
    /// Individually identified finite-stock items engaged by the complete allocation.
    UsedStockCount,
    /// Whole polygon area of used finite stock, including defects and prior occupied regions.
    UsedStockGrossAreaM2,
    /// Complete mass of used finite stock, including unusable/prior-cut regions, in kilograms.
    UsedStockMassKg,
    /// Newly allocated external cutter area on finite stock, in square metres.
    FiniteStockExternalCutAreaM2,
    /// Least observed independent whole-order total in the explicit market currency.
    ObservedOrderCost,
    /// Traversed stitch intervals, including declared locks and repeated passes.
    StitchIntervals,
    /// Feed-only time in seconds; not a robot cycle-time estimate.
    FeedSeconds,
    /// Mass of simulated cloth only (kg); omitted pieces, trims and hardware are not included.
    SimulatedClothMassKg,
    /// Maximum absolute reported edge strain (%) across compression and extension, at report precision.
    MaxAbsoluteEdgeStrainPct,
    /// Signed independent sheet-column power (W), not an absolute value or warmth rating.
    IndependentSurfacePowerW,
    /// Signed heat through the declared body coverage domain (W), not assumed whole-wearer heat.
    BodyCoveragePowerW,
    /// Peak absolute original-panel edge strain (%) over the initial state and accepted motion samples.
    MotionMaxAbsoluteEdgeStrainPct,
    /// Signed incoming-side energy (J) for all independent sheets over the whole requested motion.
    TransientSurfaceInsideEnergyJ,
    /// Signed incoming-side energy (J) of declared body paths AND exposed-cloth films, not wearer loss.
    TransientCoverageInsideEnergyJ,
    /// Lowest sheet temperature (C) among the initial state and retained mechanical sample endpoints.
    TransientSurfaceMinSampleTemperatureC,
    /// Highest sheet temperature (C) among the initial state and retained mechanical sample endpoints.
    TransientSurfaceMaxSampleTemperatureC,
    /// Lowest covered/exposed cloth temperature (C) at the initial and retained sample endpoints.
    TransientCoverageMinSampleTemperatureC,
    /// Highest covered/exposed cloth temperature (C) at the initial and retained sample endpoints.
    TransientCoverageMaxSampleTemperatureC,
    /// Signed net boundary energy of independent sheets over the complete motion (J).
    TransientSurfaceNetBoundaryEnergyJ,
    /// Signed physical boundary energy into covered/exposed cloth storage, excluding remap error (J).
    TransientCoverageNetBoundaryEnergyJ,
    /// Lowest independent-sheet cell temperature (C) at the final accepted mechanical sample.
    TransientSurfaceFinalMinTemperatureC,
    /// Highest independent-sheet cell temperature (C) at the final accepted mechanical sample.
    TransientSurfaceFinalMaxTemperatureC,
    /// Lowest covered/exposed cloth-cell temperature (C) at the final accepted mechanical sample.
    TransientCoverageFinalMinTemperatureC,
    /// Highest covered/exposed cloth-cell temperature (C) at the final accepted mechanical sample.
    TransientCoverageFinalMaxTemperatureC,
}

impl Metric {
    fn consumer(self) -> Consumer {
        match self {
            Self::StockAreaM2 | Self::ExternalCutAreaM2 | Self::StockMassKg => Consumer::Cutting,
            Self::AssemblyMassKg => Consumer::AssemblyMass,
            Self::UsedStockCount
            | Self::UsedStockGrossAreaM2
            | Self::UsedStockMassKg
            | Self::FiniteStockExternalCutAreaM2 => Consumer::FiniteStock,
            Self::ObservedOrderCost => Consumer::Purchasing,
            Self::StitchIntervals | Self::FeedSeconds => Consumer::Sewing,
            Self::SimulatedClothMassKg | Self::MaxAbsoluteEdgeStrainPct => Consumer::Drape,
            Self::IndependentSurfacePowerW => Consumer::Surface,
            Self::BodyCoveragePowerW => Consumer::Coverage,
            Self::MotionMaxAbsoluteEdgeStrainPct => Consumer::Motion,
            Self::TransientSurfaceInsideEnergyJ
            | Self::TransientSurfaceMinSampleTemperatureC
            | Self::TransientSurfaceMaxSampleTemperatureC
            | Self::TransientSurfaceNetBoundaryEnergyJ
            | Self::TransientSurfaceFinalMinTemperatureC
            | Self::TransientSurfaceFinalMaxTemperatureC => Consumer::TransientSurface,
            Self::TransientCoverageInsideEnergyJ
            | Self::TransientCoverageMinSampleTemperatureC
            | Self::TransientCoverageMaxSampleTemperatureC
            | Self::TransientCoverageNetBoundaryEnergyJ
            | Self::TransientCoverageFinalMinTemperatureC
            | Self::TransientCoverageFinalMaxTemperatureC => Consumer::TransientCoverage,
        }
    }
    fn unit(self, currency: Option<&str>) -> &str {
        match self {
            Self::StockAreaM2
            | Self::ExternalCutAreaM2
            | Self::UsedStockGrossAreaM2
            | Self::FiniteStockExternalCutAreaM2 => "m2",
            Self::StockMassKg | Self::AssemblyMassKg | Self::UsedStockMassKg => "kg",
            Self::ObservedOrderCost => currency.unwrap_or("unavailable currency"),
            Self::StitchIntervals | Self::UsedStockCount => "count",
            Self::FeedSeconds => "s",
            Self::SimulatedClothMassKg => "kg",
            Self::MaxAbsoluteEdgeStrainPct | Self::MotionMaxAbsoluteEdgeStrainPct => "%",
            Self::IndependentSurfacePowerW | Self::BodyCoveragePowerW => "W",
            Self::TransientSurfaceInsideEnergyJ
            | Self::TransientCoverageInsideEnergyJ
            | Self::TransientSurfaceNetBoundaryEnergyJ
            | Self::TransientCoverageNetBoundaryEnergyJ => "J",
            Self::TransientSurfaceMinSampleTemperatureC
            | Self::TransientSurfaceMaxSampleTemperatureC
            | Self::TransientCoverageMinSampleTemperatureC
            | Self::TransientCoverageMaxSampleTemperatureC
            | Self::TransientSurfaceFinalMinTemperatureC
            | Self::TransientSurfaceFinalMaxTemperatureC
            | Self::TransientCoverageFinalMinTemperatureC
            | Self::TransientCoverageFinalMaxTemperatureC => "C",
        }
    }
    fn value(self, row: &Alternative, admitted: &Numerical<'_>) -> Option<f64> {
        let simulation = admitted.drape;
        match self {
            Self::StockAreaM2 => row.metrics.stock_area_m2,
            Self::ExternalCutAreaM2 => row.metrics.external_cut_area_m2,
            Self::StockMassKg => row.metrics.stock_mass_kg,
            Self::AssemblyMassKg => row.assembly_mass.value().and_then(|r| r.assembly_mass.kg),
            Self::UsedStockCount => row.metrics.used_stock_count.map(|n| n as f64),
            Self::UsedStockGrossAreaM2 => row.metrics.used_stock_gross_area_m2,
            Self::UsedStockMassKg => row.metrics.used_stock_mass_kg,
            Self::FiniteStockExternalCutAreaM2 => row.metrics.finite_stock_external_cut_area_m2,
            Self::ObservedOrderCost => row.metrics.least_observed_independent_order_total,
            Self::StitchIntervals => row.metrics.stitch_intervals.map(|n| n as f64),
            Self::FeedSeconds => row.metrics.feed_seconds,
            Self::SimulatedClothMassKg => simulation["drape"]["mass_kg"].as_f64(),
            Self::MaxAbsoluteEdgeStrainPct => {
                let strain = &simulation["drape"]["global_strain_pct"];
                let lo = strain["min"].as_f64()?;
                let hi = strain["max"].as_f64()?;
                (lo.is_finite() && hi.is_finite() && lo <= hi).then(|| lo.abs().max(hi.abs()))
            }
            Self::IndependentSurfacePowerW => {
                simulation["surface"]["result"]["steady"]["power_w"].as_f64()
            }
            Self::BodyCoveragePowerW => {
                simulation["coverage"]["result"]["steady"]["power_w"].as_f64()
            }
            Self::MotionMaxAbsoluteEdgeStrainPct => {
                admitted.motion["metrics"]["max_absolute_edge_strain_pct"].as_f64()
            }
            Self::TransientSurfaceInsideEnergyJ => {
                admitted.transient_surface.as_ref()?.result["inside_energy_j"].as_f64()
            }
            Self::TransientCoverageInsideEnergyJ => {
                admitted.transient_coverage.as_ref()?.result["inside_energy_j"].as_f64()
            }
            Self::TransientSurfaceMinSampleTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.range_c[0])
            }
            Self::TransientSurfaceMaxSampleTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.range_c[1])
            }
            Self::TransientCoverageMinSampleTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.range_c[0])
            }
            Self::TransientCoverageMaxSampleTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.range_c[1])
            }
            Self::TransientSurfaceNetBoundaryEnergyJ => {
                admitted.transient_surface.as_ref()?.result["net_boundary_energy_j"].as_f64()
            }
            Self::TransientCoverageNetBoundaryEnergyJ => {
                admitted.transient_coverage.as_ref()?.result["net_boundary_energy_j"].as_f64()
            }
            Self::TransientSurfaceFinalMinTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.final_range_c[0])
            }
            Self::TransientSurfaceFinalMaxTemperatureC => {
                Some(admitted.transient_surface.as_ref()?.final_range_c[1])
            }
            Self::TransientCoverageFinalMinTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.final_range_c[0])
            }
            Self::TransientCoverageFinalMaxTemperatureC => {
                Some(admitted.transient_coverage.as_ref()?.final_range_c[1])
            }
        }
    }
    fn signed(self) -> bool {
        matches!(
            self,
            Self::IndependentSurfacePowerW
                | Self::BodyCoveragePowerW
                | Self::TransientSurfaceInsideEnergyJ
                | Self::TransientCoverageInsideEnergyJ
                | Self::TransientSurfaceMinSampleTemperatureC
                | Self::TransientSurfaceMaxSampleTemperatureC
                | Self::TransientCoverageMinSampleTemperatureC
                | Self::TransientCoverageMaxSampleTemperatureC
                | Self::TransientSurfaceNetBoundaryEnergyJ
                | Self::TransientCoverageNetBoundaryEnergyJ
                | Self::TransientSurfaceFinalMinTemperatureC
                | Self::TransientSurfaceFinalMaxTemperatureC
                | Self::TransientCoverageFinalMinTemperatureC
                | Self::TransientCoverageFinalMaxTemperatureC
        )
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

/// An opt-in comparison of alternatives evaluated in ONE study and process context.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Explicit nominal outputs or supported, conditionally propagated reported mass bounds.
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
    /// Complete conditional endpoints in `unit`, only under a reported-bounds policy. An absent
    /// range excludes that candidate; it never implies an exact nominal observation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_bounds: Option<[f64; 2]>,
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

/// An eligible pair whose protocols or reported ranges do not establish an ordering.
#[derive(Clone, Debug, Serialize)]
pub struct Incomparable {
    /// Stable keys in deterministic order.
    pub alternatives: [String; 2],
    /// Why numerical values alone did not establish dominance.
    pub reason: &'static str,
}

/// Conditional Pareto set among evaluated, eligible alternatives, never a global material optimum.
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
    /// Explicit protocol incompatibilities or unresolved reported-range overlap.
    pub incomparable: Vec<Incomparable>,
    /// Always false. Conditional mass endpoints are not a general uncertainty estimate for the
    /// geometry, inventory, numerical solvers or physical garment.
    pub uncertainty_propagated: bool,
    /// Always false, including when all computed constraints pass.
    pub physical_validation_claim: bool,
    /// True only when the policy requires complete motion, directly or through a motion objective.
    /// False means a static/manufacturing frontier, even if the report also contains a trajectory.
    pub motion_required: bool,
    /// Boundaries of the comparison and its source-data admission.
    pub notes: Vec<&'static str>,
}

impl Policy {
    /// Validate the declared comparison before expensive fitting/planning. Scenario outputs are
    /// admitted only through an explicitly requested additional production consumer.
    ///
    /// # Errors
    /// Invalid bounds, duplicate or missing objectives, or a required unrequested consumer.
    pub fn validate(&self, request: &Request, additional_consumer: bool) -> Result<(), Refusal> {
        if self.objectives.is_empty()
            || self.objectives.len() > 16
            || self.constraints.len() > 64
            || self.required_consumers.len() > 11
        {
            return Err(invalid(
                "comparison needs 1..16 objectives, at most 64 constraints and 11 consumers",
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
        if matches!(self.basis, Basis::ReportedBounds)
            && self.metrics().iter().any(|m| *m != Metric::AssemblyMassKg)
        {
            return Err(invalid(
                "reported_bounds comparison supports only assembly_mass_kg; other computed objectives and constraints have no propagated reported bounds",
            ));
        }
        let unique: BTreeSet<_> = self.required_consumers.iter().collect();
        if unique.len() != self.required_consumers.len() {
            return Err(invalid("comparison required consumers must be unique"));
        }
        for consumer in self.consumers() {
            let requested = match consumer {
                Consumer::Cutting => request.cutting.is_some(),
                Consumer::FiniteStock => request.finite_stock.is_some(),
                Consumer::Sewing => request.sewing.is_some(),
                Consumer::Purchasing => request.purchasing.is_some(),
                Consumer::AssemblyMass => request.assembly_mass.is_some(),
                Consumer::Drape
                | Consumer::Surface
                | Consumer::Coverage
                | Consumer::Motion
                | Consumer::TransientSurface
                | Consumer::TransientCoverage => additional_consumer,
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
    fn needs_simulation(&self) -> bool {
        self.consumers().iter().any(|c| {
            matches!(
                c,
                Consumer::Drape
                    | Consumer::Surface
                    | Consumer::Coverage
                    | Consumer::Motion
                    | Consumer::TransientSurface
                    | Consumer::TransientCoverage
            )
        })
    }

    fn needs_motion(&self) -> bool {
        self.consumers().iter().any(|c| {
            matches!(
                c,
                Consumer::Motion | Consumer::TransientSurface | Consumer::TransientCoverage
            )
        })
    }

    pub(super) fn evaluate(
        &self,
        rows: &[Alternative],
        source: &str,
        context: &str,
        currency: Option<&str>,
    ) -> Report {
        let candidates: Vec<_> = rows
            .iter()
            .map(|row| self.candidate(row, currency, &Value::Null, &Value::Null))
            .collect();
        self.finish(candidates, source, context)
    }

    pub(super) fn evaluate_scenarios<T: Serialize>(
        &self,
        rows: &[Alternative],
        simulations: &[ScenarioAlternative<T>],
        descriptor: &Value,
        source: &str,
        context: &str,
        currency: Option<&str>,
    ) -> Result<Report, Refusal> {
        let by_key: BTreeMap<_, _> = simulations
            .iter()
            .map(|s| (s.key.as_str(), &s.evaluation))
            .collect();
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            // At most one transient JSON representation; do not duplicate the entire mesh batch.
            let simulation = if self.needs_simulation() {
                by_key
                    .get(row.key.as_str())
                    .and_then(|outcome| outcome.value())
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|e| invalid(e.to_string()))?
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            candidates.push(self.candidate(row, currency, descriptor, &simulation));
        }
        Ok(self.finish(candidates, source, context))
    }

    fn finish(&self, mut candidates: Vec<Candidate>, source: &str, context: &str) -> Report {
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
                } else if matches!(self.basis, Basis::ReportedBounds) {
                    incomparable.push(Incomparable {
                        alternatives: [candidates[i].key.clone(), candidates[j].key.clone()],
                        reason: "Reported assembly-mass ranges overlap or touch; no guaranteed strict dominance inferred.",
                    });
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
            motion_required: self.needs_motion(),
            notes: vec![
                match self.basis {
                    Basis::Nominal => {
                        "Nominal computed outputs only; source bounds are not propagated uncertainty intervals. Estimate permission stays with each source selection."
                    }
                    Basis::ReportedBounds => {
                        "Assembly-mass constraints must contain the entire reported range. Dominance requires no worse endpoints with a strict improvement; overlapping or touching ranges remain unresolved. Bounds propagate only through fixed authored areas and additive inventory, not finishing, omitted components, geometry uncertainty or a confidence model. Estimate permission stays with each source selection."
                    }
                },
                "The frontier is conditional on the evaluated alternatives, constraints, process policy and reported-protocol admission. Scalar pre-screening may have removed better garment alternatives.",
                "Matching reported protocols is a software comparison rule, not proof that specimens or experiments are comparable. Missing or refused required outputs exclude a row without erasing other consumers.",
                "Stock mass includes waste, costs are dated complete independent order observations, and feed time excludes handling. No empirical fit, warmth, hardware authorization or global optimum is claimed.",
                "Scenario objectives require matching pattern/body/scenario identities, geometric settledness and clear gating. Geometric settledness is not viscoelastic stress equilibrium or empirical fit; simulated cloth mass excludes omitted components.",
                "Heat powers keep their signed directions and distinct sheet/domain bases. Strain uses the production report's edge extrema and precision, not an invented fit score. Different nonlinear constitutive records are conservatively protocol-incomparable unless explicitly admitted.",
                "Motion is required only when declared by the comparison policy. Motion objectives use a complete source/path/time-matched trajectory from a settled, gating-clear initial drape; the final moving state need not be at rest. Peak strain is over original-panel edges and retained samples, not continuous-time rupture or anatomical fit.",
                "Transient objectives require a complete policy/path/geometry/time-matched history at every accepted mechanical sample. Energy retains its incoming-side sign and model domain; coverage includes bare paths and explicitly exposed-cloth films, not certified whole-wearer loss. Sheet and coverage energies must not be added together.",
                "Sample temperature extrema include the declared initial state and retained mechanical sample endpoints; final temperature extrema use only the final accepted sample. Neither is a continuous-time bound or the extrema of unreported thermal substeps. Net boundary energy excludes remap residual and cancelling bare-body paths. Numerical energy/remap residuals remain in source reports; nominal admission is not empirical accuracy, a comfort threshold, or propagated uncertainty.",
            ],
        }
    }

    fn candidate(
        &self,
        row: &Alternative,
        currency: Option<&str>,
        descriptor: &Value,
        simulation: &Value,
    ) -> Candidate {
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
        let simulation = if self.needs_simulation() {
            match admit_simulation(row, descriptor, simulation) {
                Ok(()) => simulation,
                Err(e) => {
                    result.exclusions.push(e);
                    &Value::Null
                }
            }
        } else {
            &Value::Null
        };
        let mut admitted = Numerical {
            drape: simulation,
            motion: &Value::Null,
            transient_surface: None,
            transient_coverage: None,
        };
        if self.needs_motion() && !simulation.is_null() {
            match admit_motion(descriptor, simulation) {
                Ok(motion) => admitted.motion = motion,
                Err(e) => result.exclusions.push(e),
            }
        }
        if !admitted.motion.is_null() {
            for (consumer, target) in [
                (Consumer::TransientSurface, &mut admitted.transient_surface),
                (
                    Consumer::TransientCoverage,
                    &mut admitted.transient_coverage,
                ),
            ] {
                if self.consumers().contains(&consumer) {
                    match admit_heat(row, descriptor, admitted.motion, consumer) {
                        Ok(heat) => *target = Some(heat),
                        Err(e) => result.exclusions.push(e),
                    }
                }
            }
        }
        for consumer in self.consumers() {
            if !consumer_available(row, consumer, &admitted) {
                result.exclusions.push(invalid(format!(
                    "comparison requires available {consumer:?} with complete required coverage"
                )));
            }
        }
        for metric in self.metrics() {
            match metric
                .value(row, &admitted)
                .filter(|v| v.is_finite() && (metric.signed() || *v >= 0.))
            {
                Some(value) if consumer_available(row, metric.consumer(), &admitted) => {
                    let reported_bounds = match self.basis {
                        Basis::Nominal => None,
                        Basis::ReportedBounds => row
                            .assembly_mass
                            .value()
                            .and_then(|r| r.assembly_mass.reported_bounds_kg)
                            .filter(|[lo, hi]| {
                                lo.is_finite()
                                    && hi.is_finite()
                                    && *lo >= 0.
                                    && *lo <= value
                                    && value <= *hi
                            }),
                    };
                    result.readings.insert(
                        metric,
                        Reading {
                            value,
                            reported_bounds,
                            unit: metric.unit(currency).into(),
                            consumer: metric.consumer(),
                        },
                    );
                    let [lo, hi] = match self.basis {
                        Basis::Nominal => [value, value],
                        Basis::ReportedBounds => {
                            let Some(bounds) = reported_bounds else {
                                result.exclusions.push(invalid(format!(
                                    "{metric:?} has no complete valid reported bounds; nominal mass remains visible but no exact range is substituted"
                                )));
                                continue;
                            };
                            bounds
                        }
                    };
                    for c in self.constraints.iter().filter(|c| c.metric == metric) {
                        if c.minimum.is_some_and(|min| lo < min)
                            || c.maximum.is_some_and(|max| hi > max)
                        {
                            let observed = match self.basis {
                                Basis::Nominal => value.to_string(),
                                Basis::ReportedBounds => format!("reported range [{lo}, {hi}]"),
                            };
                            result.exclusions.push(invalid(format!("{metric:?} = {observed} {} violates the inclusive comparison constraint", metric.unit(currency))));
                        }
                    }
                }
                _ => result.exclusions.push(invalid(format!(
                    "{metric:?} is unavailable; no zero or partial value substituted"
                ))),
            }
        }
        match self.protocol(row, descriptor) {
            Ok(protocol) => result.input_protocol_sha256 = Some(protocol),
            Err(e) => result.exclusions.push(e),
        }
        result.eligible = result.exclusions.is_empty();
        result
    }

    fn dominates(&self, a: &Candidate, b: &Candidate) -> bool {
        let mut strictly = false;
        for objective in &self.objectives {
            let interval = |candidate: &Candidate| {
                let reading = &candidate.readings[&objective.metric];
                match self.basis {
                    Basis::Nominal => [reading.value, reading.value],
                    Basis::ReportedBounds => reading.reported_bounds.expect("admitted mass range"),
                }
            };
            let [al, ah] = interval(a);
            let [bl, bh] = interval(b);
            match objective.direction {
                Direction::Minimize => {
                    if ah > bl {
                        return false;
                    }
                    strictly |= ah < bl;
                }
                Direction::Maximize => {
                    if al < bh {
                        return false;
                    }
                    strictly |= al > bh;
                }
            }
        }
        strictly
    }

    fn protocol(&self, row: &Alternative, descriptor: &Value) -> Result<String, Refusal> {
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
        if self
            .metrics()
            .iter()
            .any(|m| matches!(m, Metric::StockMassKg | Metric::UsedStockMassKg))
            || consumers.contains(&Consumer::AssemblyMass)
        {
            keys.insert("areal_density");
        }
        let mechanics = self.needs_simulation();
        let transient = consumers.contains(&Consumer::TransientSurface)
            || consumers.contains(&Consumer::TransientCoverage);
        let thermal = transient
            || consumers.contains(&Consumer::Surface)
            || consumers.contains(&Consumer::Coverage);
        if mechanics {
            keys.extend(
                ftx_pattern::materials::MECHANICAL_PROPERTIES
                    .iter()
                    .map(|(key, _)| *key),
            );
        }
        if thermal {
            keys.extend(["thermal_conductivity", "thermal_resistance", "emissivity"]);
        }
        if transient {
            keys.extend(["areal_density", "specific_heat", "thickness"]);
        }
        if [
            (Consumer::TransientSurface, "transient_surface"),
            (Consumer::TransientCoverage, "transient_coverage"),
        ]
        .into_iter()
        .any(|(consumer, name)| {
            consumers.contains(&consumer)
                && descriptor["scenario"]["motion"][name]["plastic_dissipation_heat"]
                    == "uniform_reference_thickness"
        }) {
            keys.insert("membrane_plastic_heat_fraction");
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
            let mut relevant = keys.clone();
            if mechanics {
                relevant.extend(
                    material
                        .properties
                        .keys()
                        .filter(|k| k.starts_with("bending_"))
                        .map(String::as_str),
                );
            }
            let profile: BTreeMap<_, _> = relevant
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
            // Scalar protocols compare qualifiers, not their values. Complex response records use
            // conservative whole-record equality rather than guessing which curve fields are metadata.
            let models: BTreeMap<_, _> = material
                .structure
                .iter()
                .filter(|(key, _)| {
                    mechanics && (key.starts_with("bending_") || key.starts_with("membrane_"))
                })
                .collect();
            protocols.insert(id, json!({"properties":profile, "constitutive_models":models,
                "membrane_response":if mechanics { serde_json::to_value(&material.membrane_response).map_err(|e| invalid(e.to_string()))? } else { Value::Null },
                "membrane_relaxation":if mechanics { serde_json::to_value(&material.membrane_relaxation).map_err(|e| invalid(e.to_string()))? } else { Value::Null },
                "thermal_response":if thermal { serde_json::to_value(&material.thermal_response).map_err(|e| invalid(e.to_string()))? } else { Value::Null }}));
        }
        hash(&(
            protocols,
            if mechanics {
                bound["pattern"]["fabric"]["interfaces"].clone()
            } else {
                Value::Null
            },
        ))
    }
}

// Each reference is either independently admitted or null. A failed motion must not make a valid
// static reading vanish, and an available status alone must not admit stale numerical results.
struct Numerical<'a> {
    drape: &'a Value,
    motion: &'a Value,
    transient_surface: Option<Heat<'a>>,
    transient_coverage: Option<Heat<'a>>,
}

struct Heat<'a> {
    result: &'a Value,
    range_c: [f64; 2],
    final_range_c: [f64; 2],
}

fn consumer_available(row: &Alternative, consumer: Consumer, admitted: &Numerical<'_>) -> bool {
    match consumer {
        Consumer::Cutting => row.cutting.value().is_some(),
        Consumer::FiniteStock => row.finite_stock.value().is_some(),
        Consumer::Sewing => row.sewing.value().is_some(),
        Consumer::Purchasing => row
            .purchasing
            .value()
            .is_some_and(|p| p["complete_offer_coverage"] == true),
        Consumer::AssemblyMass => row.assembly_mass.value().is_some_and(|r| {
            r.assembly_mass.kg.is_some()
                && row.binding.value().is_some_and(|b| {
                    b["pattern_sha256"].as_str() == Some(r.pattern_sha256.as_str())
                })
        }),
        Consumer::Drape => !admitted.drape.is_null(),
        Consumer::Surface => admitted.drape["surface"]["status"] == "available",
        Consumer::Coverage => admitted.drape["coverage"]["status"] == "available",
        Consumer::Motion => !admitted.motion.is_null(),
        Consumer::TransientSurface => admitted.transient_surface.is_some(),
        Consumer::TransientCoverage => admitted.transient_coverage.is_some(),
    }
}

fn admit_simulation(
    row: &Alternative,
    descriptor: &Value,
    simulation: &Value,
) -> Result<(), Refusal> {
    let bound = row
        .binding
        .value()
        .ok_or_else(|| invalid("scenario comparison requires successful binding"))?;
    let same = |key: &str, expected: &Value| {
        expected
            .as_str()
            .is_some_and(|s| !s.is_empty() && simulation[key].as_str() == Some(s))
    };
    if simulation["schema"] != "ftx-drape-scenario-result/1"
        || !same("pattern_sha256", &bound["pattern_sha256"])
        || !same("scenario_sha256", &descriptor["scenario_sha256"])
        || !same("body_geometry_sha256", &descriptor["body_geometry_sha256"])
    {
        return Err(invalid(
            "scenario comparison needs fresh production results matching this bound pattern, body and scenario; missing/stale identities refuse",
        ));
    }
    if simulation["drape"]["settled"] != true
        || simulation["metrics"]["simulation_gating_clear"] != true
        || !simulation["drape"]["gating"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(invalid(
            "scenario comparison requires a geometrically settled, gating-clear drape; review retained flags rather than ranking an unfinished frame",
        ));
    }
    Ok(())
}

fn same_identity(actual: &Value, expected: &Value) -> bool {
    expected
        .as_str()
        .is_some_and(|s| !s.is_empty() && actual.as_str() == Some(s))
}

fn admit_motion<'a>(descriptor: &Value, simulation: &'a Value) -> Result<&'a Value, Refusal> {
    let request = &descriptor["scenario"]["motion"];
    let motion = &simulation["motion"]["result"];
    if request["schema"] != "ftx-drape-motion/1"
        || simulation["motion"]["status"] != "available"
        || motion["schema"] != "ftx-drape-motion-result/1"
        || !same_identity(&motion["path_sha256"], &descriptor["motion_path"]["sha256"])
        || !same_identity(
            &motion["initial_positions_sha256"],
            &simulation["positions_sha256"],
        )
        || motion["continuous_contact_checked"] != true
        || motion["initial_drape_settled"] != true
        || !motion["initial_drape_gating"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(invalid(
            "motion comparison needs successful checked motion matching the requested path and admitted initial drape",
        ));
    }
    let frames = request["keyframes"]
        .as_array()
        .filter(|f| !f.is_empty() && f.len() <= 16)
        .ok_or_else(|| invalid("motion comparison has no bounded requested keyframes"))?;
    let samples = motion["samples"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(|| invalid("motion comparison needs the complete accepted sample sequence"))?;
    let endpoints = motion["keyframes"]
        .as_array()
        .filter(|f| f.len() == frames.len())
        .ok_or_else(|| invalid("motion comparison needs every requested endpoint"))?;
    let duration = motion["duration_s"]
        .as_f64()
        .filter(|t| t.is_finite() && *t > 0.)
        .ok_or_else(|| invalid("motion comparison has no finite physical duration"))?;
    if frames.last().and_then(|f| f["time_s"].as_f64()) != Some(duration)
        || samples.last().and_then(|s| s["time_s"].as_f64()) != Some(duration)
        || !motion["trials"].as_u64().is_some_and(|n| {
            n >= samples.len() as u64 && request["max_trials"].as_u64().is_some_and(|max| n <= max)
        })
    {
        return Err(invalid(
            "motion comparison refuses partial or mismatched-duration trajectories",
        ));
    }
    let mut previous = 0.;
    for sample in samples {
        let time = sample["time_s"]
            .as_f64()
            .filter(|t| t.is_finite() && *t > previous && *t <= duration)
            .ok_or_else(|| invalid("motion comparison requires ordered physical sample times"))?;
        if sample["solve"]["converged"] != true {
            return Err(invalid(
                "motion comparison refuses an unconverged accepted substep",
            ));
        }
        previous = time;
    }
    let mut next_sample = 0;
    for (frame, endpoint) in frames.iter().zip(endpoints) {
        let index = endpoint["sample_index"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n >= next_sample && n < samples.len())
            .ok_or_else(|| {
                invalid("motion comparison requires ordered, retained endpoint samples")
            })?;
        let sample = &samples[index];
        if endpoint["time_s"] != frame["time_s"]
            || endpoint["time_s"] != sample["time_s"]
            || endpoint["arm_abduction_deg"] != frame["arm_abduction_deg"]
            || !sample["positions_m"].is_array()
            || !sample["body_positions_m"].is_array()
            || endpoint["positions_sha256"].as_str() != Some(hash(&sample["positions_m"])?.as_str())
            || endpoint["body_collision_sha256"].as_str()
                != Some(hash(&(&sample["body_positions_m"], &motion["body_triangles"]))?.as_str())
        {
            return Err(invalid(
                "motion comparison refuses stale endpoint pose, time or geometry",
            ));
        }
        next_sample = index + 1;
    }
    if next_sample != samples.len() {
        return Err(invalid(
            "motion comparison needs the final requested endpoint, without extra samples",
        ));
    }
    Ok(motion)
}

fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|x| x.is_finite())
}

fn admit_heat<'a>(
    alternative: &Alternative,
    descriptor: &Value,
    motion: &'a Value,
    consumer: Consumer,
) -> Result<Heat<'a>, Refusal> {
    let (name, schema, request_schema, coverage) = match consumer {
        Consumer::TransientSurface => (
            "transient_surface",
            "ftx-motion-surface-heat-result/1",
            "ftx-motion-surface-heat/1",
            false,
        ),
        Consumer::TransientCoverage => (
            "transient_coverage",
            "ftx-motion-coverage-heat-result/1",
            "ftx-motion-coverage-heat/1",
            true,
        ),
        _ => return Err(invalid("not a transient comparison consumer")),
    };
    let request = &descriptor["scenario"]["motion"][name];
    let h = &motion[name]["result"];
    let fail = |message: &str| invalid(format!("{consumer:?}: {message}"));
    if request["schema"] != request_schema
        || motion[name]["status"] != "available"
        || h["schema"] != schema
        || &h["request"] != request
        || h["duration_s"] != motion["duration_s"]
        || !same_identity(&h["path_sha256"], &motion["path_sha256"])
        || !same_identity(
            &h["initial_positions_sha256"],
            &motion["initial_positions_sha256"],
        )
    {
        return Err(fail(
            "complete thermal history must match the requested policy and admitted motion",
        ));
    }
    // Admission is deliberately tied to the actual accepted sequence, not an old scalar summary.
    // This is consistency checking of trusted production output, not an authenticity certificate.
    let samples = motion["samples"]
        .as_array()
        .ok_or_else(|| fail("missing admitted mechanical sequence"))?;
    let endpoints = motion["keyframes"]
        .as_array()
        .ok_or_else(|| fail("missing admitted mechanical endpoints"))?;
    let trace = h["trace"]
        .as_array()
        .filter(|t| !t.is_empty() && t.len() == samples.len())
        .ok_or_else(|| fail("thermal history must cover every accepted mechanical sample"))?;
    if !request["boundaries"]
        .as_array()
        .is_some_and(|b| b.len() == endpoints.len())
        || (coverage
            && !request["exposed_boundaries"]
                .as_array()
                .is_some_and(|b| b.len() == endpoints.len()))
    {
        return Err(fail("missing explicit interval boundary policy"));
    }
    let initial = finite(&request["initial_temperature_c"])
        .filter(|t| *t >= -273.15)
        .ok_or_else(|| fail("initial temperature must be finite and above absolute zero"))?;
    let mut range = [initial, initial];
    let mut final_range = None;
    let mut steps = 0_u64;
    let mut interval = 0;
    for (i, (row, sample)) in trace.iter().zip(samples).enumerate() {
        while endpoints[interval]["sample_index"]
            .as_u64()
            .is_some_and(|last| last < i as u64)
        {
            interval += 1;
            if interval >= endpoints.len() {
                return Err(fail("thermal history extends beyond the requested motion"));
            }
        }
        if row["sample_index"].as_u64() != Some(i as u64)
            || row["interval_index"].as_u64() != Some(interval as u64)
            || row["time_s"] != sample["time_s"]
            || row["positions_sha256"].as_str() != Some(hash(&sample["positions_m"])?.as_str())
            || (coverage
                && row["body_positions_sha256"].as_str()
                    != Some(hash(&sample["body_positions_m"])?.as_str()))
        {
            return Err(fail(
                "stale or missing thermal sample time, interval or geometry",
            ));
        }
        let temperatures = row["temperature_range_c"]
            .as_array()
            .filter(|v| v.len() == 2)
            .ok_or_else(|| fail("missing sampled temperature range"))?;
        let lo = finite(&temperatures[0]);
        let hi = finite(&temperatures[1]);
        match (lo, hi) {
            (Some(lo), Some(hi)) if lo >= -273.15 && lo <= hi => {
                range[0] = range[0].min(lo);
                range[1] = range[1].max(hi);
                final_range = Some([lo, hi]);
            }
            _ => {
                return Err(fail(
                    "sampled temperatures must be finite, ordered and physical",
                ));
            }
        }
        steps = row["thermal_substeps"]
            .as_u64()
            .filter(|&n| n > 0)
            .and_then(|n| steps.checked_add(n))
            .ok_or_else(|| fail("missing or overflowing thermal substep count"))?;
    }
    if h["thermal_steps"].as_u64() != Some(steps)
        || !h["cell_steps"].as_u64().is_some_and(|n| {
            n >= steps
                && request["max_cell_steps"]
                    .as_u64()
                    .is_some_and(|max| n <= max)
        })
    {
        return Err(fail(
            "thermal work counts do not describe the complete admitted history",
        ));
    }
    // Keep physical storage separate from remap error. Neither residual becomes incoming heat.
    let stored = if coverage {
        "physical_stored_change_j"
    } else {
        "stored_change_j"
    };
    for field in [
        "inside_energy_j",
        "outside_energy_j",
        "net_boundary_energy_j",
        stored,
        "balance_error_j",
        "absolute_step_balance_error_j",
    ] {
        if finite(&h[field]).is_none() {
            return Err(fail("missing finite full-history energy accounting"));
        }
    }
    if coverage {
        for field in [
            "bare_energy_j",
            "signed_remap_residual_j",
            "absolute_remap_residual_j",
        ] {
            if finite(&h[field]).is_none() {
                return Err(fail("missing separate bare-body and remap accounting"));
            }
        }
    }
    match request
        .get("plastic_dissipation_heat")
        .filter(|v| !v.is_null())
    {
        Some(policy) if policy.as_str() == Some("uniform_reference_thickness") => {
            admit_plastic_heat(alternative, motion, h, consumer, stored)?;
        }
        Some(_) => {
            return Err(fail(
                "unsupported plastic heat policy for this thermal domain",
            ));
        }
        None if !h["plastic_dissipation_heat"].is_null()
            || trace
                .iter()
                .any(|r| !r["plastic_dissipation_heat"].is_null()) =>
        {
            return Err(fail(
                "plastic heating was not requested for this thermal domain",
            ));
        }
        None => {}
    }
    Ok(Heat {
        result: h,
        range_c: range,
        final_range_c: final_range
            .ok_or_else(|| fail("missing final sampled temperature range"))?,
    })
}

// Reconcile the published source terms with accepted work. This does not solve mechanics or heat
// again, nor certify an imported report: the existing consumer owns constitutive qualification.
fn admit_plastic_heat(
    alternative: &Alternative,
    motion: &Value,
    heat: &Value,
    consumer: Consumer,
    stored_field: &str,
) -> Result<(), Refusal> {
    use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
    let fail = || {
        invalid(format!(
            "{consumer:?}: plastic heat needs source-matched accepted work and complete supplied-energy accounting"
        ))
    };
    let frozen = alternative.frozen_changes.as_ref().ok_or_else(fail)?;
    let receipts = heat["plastic_dissipation_heat"]["material_fractions"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(fail)?;
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let mut laws = BTreeMap::new();
    for receipt in receipts {
        let identity = receipt["material_sha256"].as_str().ok_or_else(fail)?;
        let selection = frozen
            .pieces
            .values()
            .chain(frozen.default.iter())
            .find(|s| s.expected_sha256.as_deref() == Some(identity))
            .ok_or_else(fail)?;
        let material = catalog.resolve(selection)?;
        let fraction = material
            .membrane_plastic_heat_fraction()?
            .ok_or_else(fail)?;
        let law = material
            .membrane_plasticity()?
            .ok_or_else(fail)?
            .fingerprint();
        let expected = json!({"material_sha256":material.sha256(),"law_sha256":law,
            "fraction":fraction.fraction,"temperature_range_c":fraction.temperature_range_c,
            "observation":material.authored().properties["membrane_plastic_heat_fraction"]});
        if receipt != &expected || laws.insert(law, fraction.fraction).is_some() {
            return Err(fail());
        }
    }
    let triangles = motion["triangles"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 65_536)
        .ok_or_else(fail)?;
    let values = |sample: &Value| -> Result<Vec<f64>, Refusal> {
        sample["membrane_plastic_dissipation_j"]
            .as_array()
            .filter(|v| v.len() == triangles.len())
            .ok_or_else(fail)?
            .iter()
            .map(|v| finite(v).filter(|&v| v >= 0.).ok_or_else(fail))
            .collect()
    };
    let mut previous = values(&motion["initial_sample"])?;
    let initial: f64 = previous.iter().sum();
    let near = |a: f64, b: f64| {
        a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.)
    };
    if finite(&motion["initial_sample"]["time_s"]) != Some(0.)
        || !finite(&motion["initial_membrane_plasticity_history"]["plastic_dissipation_j"])
            .is_some_and(|v| near(v, initial))
    {
        return Err(fail());
    }
    let mut fractions = vec![None; triangles.len()];
    let mut used = BTreeSet::new();
    for element in motion["initial_membrane_plasticity_history"]["elements"]
        .as_array()
        .ok_or_else(fail)?
    {
        let index = element["triangle"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < triangles.len())
            .ok_or_else(fail)?;
        let law = element["law_sha256"].as_str().ok_or_else(fail)?;
        let fraction = *laws.get(law).ok_or_else(fail)?;
        if fractions[index].replace(fraction).is_some()
            || element["vertices"] != triangles[index]
            || !finite(&element["plastic_dissipation_j"]).is_some_and(|v| near(v, previous[index]))
        {
            return Err(fail());
        }
        used.insert(law);
    }
    if used.len() != laws.len()
        || fractions
            .iter()
            .zip(&previous)
            .any(|(f, &j)| f.is_none() && j != 0.)
    {
        return Err(fail());
    }
    let check = |row: &Value, dissipated: f64, supplied: f64| -> Result<(), Refusal> {
        let work = &row["plastic_dissipation_heat"];
        if work["distribution"] != "uniform_reference_thickness"
            || work["temporal_distribution"] != "uniform_over_accepted_interval"
        {
            return Err(fail());
        }
        for (name, expected) in [
            ("plastic_dissipation_j", dissipated),
            ("supplied_heat_j", supplied),
            ("unconverted_dissipation_j", dissipated - supplied),
            ("initial_plastic_dissipation_j", initial),
        ] {
            if !finite(&work[name]).is_some_and(|v| v >= 0. && near(v, expected)) {
                return Err(fail());
            }
        }
        let inside = finite(&row["inside_energy_j"]).ok_or_else(fail)?;
        let outside = finite(&row["outside_energy_j"]).ok_or_else(fail)?;
        let net = finite(&row["net_boundary_energy_j"]).ok_or_else(fail)?;
        let stored = finite(&row[stored_field]).ok_or_else(fail)?;
        let residual = finite(&row["balance_error_j"]).ok_or_else(fail)?;
        if !near(net, inside - outside) || !near(residual, stored - net - supplied) {
            return Err(fail());
        }
        Ok(())
    };
    let mut dissipated = 0.;
    let mut supplied = 0.;
    for (sample, row) in motion["samples"]
        .as_array()
        .ok_or_else(fail)?
        .iter()
        .zip(heat["trace"].as_array().ok_or_else(fail)?)
    {
        let current = values(sample)?;
        let mut step_work = 0.;
        let mut step_heat = 0.;
        for ((&value, &old), fraction) in current.iter().zip(&previous).zip(&fractions) {
            if value < old || (fraction.is_none() && value != 0.) {
                return Err(fail());
            }
            step_work += value - old;
            step_heat += fraction.unwrap_or(0.) * (value - old);
        }
        dissipated += step_work;
        supplied += step_heat;
        check(row, dissipated, supplied)?;
        previous = current;
    }
    if !finite(&motion["membrane_plasticity_history"]["plastic_dissipation_j"])
        .is_some_and(|v| near(v, initial + dissipated))
    {
        return Err(fail());
    }
    check(heat, dissipated, supplied)
}

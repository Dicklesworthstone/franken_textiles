//! Material alternatives and explicit bounded alterations evaluated by production consumers.
//!
//! Every variant starts from the same source, never the preceding variant. Screening is optional
//! and does not hide per-consumer failures: a missing modulus cannot block cutting, a missing offer
//! cannot make stock free, and a sewing refusal cannot erase a valid marker. No hardware is driven.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{
    Pattern, material_binding::Changes, material_catalog::Selection, material_cost::QuoteBook,
    material_screen, textile_import,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Ctx, ExportOptions, assembly_mass,
    cutting::{
        CutOptions, CutPlan,
        geometry::Work,
        job,
        procurement::{self, Market},
        stock,
    },
    sewing::{SewOptions, SewPlan},
    sewing_selection,
};

pub mod combinations;
pub mod comparison;
pub mod design;
pub mod matrix;
pub mod recipes;
pub mod vapor;

/// Explicit substitution, including heterogeneous source-piece assignments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// Unique alternative identity, independent of the material's display name.
    pub key: String,
    /// Existing atomic material-binding operations; geometry/process parameters cannot be edited.
    pub changes: Changes,
}

struct PreparedVariant {
    variant: Variant,
    choices: BTreeMap<String, String>,
    design: Option<design::Prepared>,
}

struct PreparedStudy {
    variants: Vec<PreparedVariant>,
    screening: Option<material_screen::Report>,
    design_search: Option<design::SearchReceipt>,
}

impl From<Variant> for PreparedVariant {
    fn from(variant: Variant) -> Self {
        Self {
            variant,
            choices: BTreeMap::new(),
            design: None,
        }
    }
}

/// Where every material shortlisted by screening is substituted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// Replace only the default; explicitly assigned lining/cuff materials remain unchanged.
    Default,
    /// Replace these exact source-piece IDs, not instance IDs or cutting labels.
    Pieces {
        /// Nonempty unique source IDs, validated even when screening returns no eligible materials.
        ids: Vec<String>,
    },
}

/// Bridge from scalar screening to actual garment manufacturing. No silent Pareto pruning.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screened {
    /// Existing source-qualified constraints/objectives and explicit evidence policy.
    pub request: material_screen::Request,
    /// Explicit placement of the selected alternatives on the original pattern.
    pub target: Target,
    /// Required choice: false evaluates every eligible record. True deliberately prunes by scalar
    /// objectives before manufacturing, which can remove a better marker/order alternative.
    pub frontier_only: bool,
}

/// Dated source-bearing offers and comparable market scope; no network or checkout operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purchasing {
    /// Immutable offer observations; original history and source hashes are retained.
    pub quotes: QuoteBook,
    /// The same currency/region/quality/terms/date scope for every variant.
    pub market: Market,
}

/// Bounded, file-free manufacturing study over a single source pattern.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-material-study/1` keeps geometry fixed; `ftx-design-study/1` explicitly varies it.
    pub schema: String,
    /// Explicit independent alteration sequences, crossed with the material alternatives. Only
    /// design studies admit this field; the unaltered `baseline` is always retained automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designs: Option<Vec<design::Variant>>,
    /// Bounded linked alteration variables; exclusive with authored `designs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_grid: Option<design::grid::Request>,
    /// Adaptive linked alterations under explicit finite bounds, steps and fit-evaluation budget.
    /// Exclusive with authored designs and grids; requires explicit selected-body fit targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_adaptive: Option<design::adaptive::Request>,
    /// Explicit flat fit targets evaluated against the caller's selected live body authority.
    /// Requested targets without that authority refuse every design, including the baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_fit: Option<design::fit::Request>,
    /// Explicit alternatives; exclusive with screening/combinations. At most 32 are evaluated.
    #[serde(default)]
    pub variants: Vec<Variant>,
    /// Optional screening directly over the canonical catalog or embedded material selections.
    #[serde(default)]
    pub screened: Option<Screened>,
    /// Exhaustive bounded combinations of independent material choices on declared targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combinations: Option<combinations::Request>,
    /// Shared cutting policy; omission means not requested, not failed or successful.
    #[serde(default)]
    pub cutting: Option<CutOptions>,
    /// Shared physical hide/remnant inventory and policy. Every alternative starts from the same
    /// inventory; evaluating alternatives does not reserve stock or consume earlier rows' remnants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_stock: Option<stock::Request>,
    /// Complete explicitly routed roll/finite-stock job. Exclusive with the separate legacy
    /// cutting, finite_stock and purchasing requests, so no physical part is ordered twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutting_job: Option<job::Request>,
    /// Shared machine/assembly/seam policy, checked separately for every substituted material.
    #[serde(default)]
    pub sewing: Option<SewOptions>,
    /// Optional qualified recipe search, rerun on each substituted material. Exclusive with
    /// `sewing`; its selected production plan supplies the same sewing metrics and comparisons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sewing_selection: Option<sewing_selection::Request>,
    /// Optional pricing requires cutting, because purchased area comes from the actual marker.
    #[serde(default)]
    pub purchasing: Option<Purchasing>,
    /// Optional complete cut-piece inventory plus declared extras for ONE assembly, not stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_mass: Option<assembly_mass::Request>,
    /// Potential vapor transport through an explicit common-area stack of final piece/default
    /// assignments. Its source observations and boundary assumptions remain independent of heat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vapor: Option<vapor::Request>,
    /// Optional explicit objectives over actual consumer outputs, not scalar catalog screening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Policy>,
}

/// Separate result for one consumer. A numerical plan is never hardware authorization.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome<T> {
    /// Consumer was omitted explicitly.
    NotRequested,
    /// The existing consumer returned its normal source-bearing result.
    Available {
        /// Unmodified consumer output.
        result: T,
    },
    /// The consumer or one of its required dependencies refused this alternative.
    Refused {
        /// Structured reason; unrelated consumers are still evaluated.
        refusal: Refusal,
    },
}

impl<T> Outcome<T> {
    fn is_not_requested(&self) -> bool {
        matches!(self, Self::NotRequested)
    }
    fn from_result(result: Result<T, Refusal>) -> Self {
        match result {
            Ok(result) => Self::Available { result },
            Err(refusal) => Self::Refused { refusal },
        }
    }
    fn value(&self) -> Option<&T> {
        match self {
            Self::Available { result } => Some(result),
            _ => None,
        }
    }
}

/// Comparable outputs of actual consumers, not inferred textile or wearer performance.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    /// Gross roll area including waste/margins; absent if cutting is unavailable.
    pub stock_area_m2: Option<f64>,
    /// Sum of outer cut-piece areas; holes are not subtracted or counted as recovered stock.
    pub external_cut_area_m2: Option<f64>,
    /// Complete purchased-stock mass only; absent if any roll lacks admitted areal density.
    pub stock_mass_kg: Option<f64>,
    /// Complete declared pre-finishing cut assembly, including unsimulated pieces and extras.
    /// Missing contributions or an incomplete inventory yield no value, not a subtotal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assembly_mass_kg: Option<f64>,
    /// Number of individually identified finite-stock items used, each counted once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_stock_count: Option<usize>,
    /// Whole polygon area of used finite-stock items, including defects and prior occupied regions.
    /// This is inventory engaged by the plan, not newly consumed area or a purchase requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_stock_gross_area_m2: Option<f64>,
    /// Complete mass of used finite-stock items; missing density on any used item keeps it unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_stock_mass_kg: Option<f64>,
    /// New external cutter area allocated by the finite-stock planner, separate from roll cutting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finite_stock_external_cut_area_m2: Option<f64>,
    /// Complete mixed-job new external cut area, across its roll and finite-stock routes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutting_job_external_cut_area_m2: Option<f64>,
    /// Gross roll area plus whole used finite-item area, including waste, defects and prior cuts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutting_job_stock_area_m2: Option<f64>,
    /// Complete engaged mixed-job stock mass; unknown if any used stock lacks mass evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutting_job_stock_mass_kg: Option<f64>,
    /// Complete same-market whole-job order total; never a subtotal of only available prices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutting_job_observed_order_cost: Option<f64>,
    /// Complete feasible mixed-job order cost; a bounded allocation need not prove its minimum.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutting_job_selected_order_cost: Option<f64>,
    /// Least observed independent whole-order total in the request currency; absent for any
    /// missing offer. Neither a live quote guarantee nor a market-population percentile.
    pub least_observed_independent_order_total: Option<f64>,
    /// Complete feasible roll-order cost, including a bounded split-supplier allocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_observed_order_total: Option<f64>,
    /// Sew-plan stitch intervals, including its declared directed passes/backtacks.
    pub stitch_intervals: Option<usize>,
    /// Declared stitch-rate feed time; excludes handling, robot movement and machine setup.
    pub feed_seconds: Option<f64>,
    /// Selected production schedule's source-qualified setup time in seconds. Unknown without
    /// admitted directed timing evidence; setup-change counts never supply a time estimate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sewing_setup_seconds: Option<f64>,
    /// Selected production plan's feed plus source-qualified setup duration in seconds.
    /// Excludes handling, acceleration, final reset and other unmodeled production work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sewing_feed_and_setup_seconds: Option<f64>,
    /// Complete source-qualified thread demand for the selected sewing plan, across every channel.
    /// Cutting copies do not multiply this one declared sewing job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_required_length_m: Option<f64>,
    /// Complete independent thread orders in the sewing consumer's own declared currency.
    /// Separate from cut-stock purchasing; missing channels or offers never produce a subtotal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_observed_order_cost: Option<f64>,
}

/// One source-locked alternative, with independent production-planner results.
#[derive(Clone, Debug, Serialize)]
pub struct Alternative {
    /// Original material key, or the canonical `[design_key, material_key]` pair in a design study.
    pub key: String,
    /// Selected choice key for every declared combination group; absent for explicit/screened rows.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub choices: BTreeMap<String, String>,
    /// Present only in design studies, including refused designs and the unaltered baseline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<design::Receipt>,
    /// Source-complete binding report, including lossless `pattern_json` for later draping/exports.
    pub binding: Outcome<Value>,
    /// Source-locked changes which reproduce this binding on the original pattern, or on this
    /// design's exact `altered_pattern_json`. Present only after successful binding; unchanged
    /// inherited/per-piece assignments are also frozen.
    pub frozen_changes: Option<Changes>,
    /// Real roll marker output or refusal; no mechanical constants are required just to cut.
    pub cutting: Outcome<CutPlan>,
    /// Real finite-hide/remnant allocation, independent of roll cutting, sewing and roll pricing.
    #[serde(skip_serializing_if = "Outcome::is_not_requested")]
    pub finite_stock: Outcome<stock::Plan>,
    /// Complete mixed roll/hide/remnant cutting, with exact expanded inventory and source replay.
    #[serde(skip_serializing_if = "Outcome::is_not_requested")]
    pub cutting_job: Outcome<job::Plan>,
    /// Real sewing plan output or refusal; preserves uncompressed-stack and recipe limitations.
    pub sewing: Outcome<SewPlan>,
    /// Complete finite recipe-search receipt; the selected plan is stored once in `sewing`.
    /// A valid search can be available without finding any admissible sewing plan.
    #[serde(skip_serializing_if = "Outcome::is_not_requested")]
    pub sewing_selection: Outcome<recipes::Selection>,
    /// Real marker-derived purchasing output or refusal; missing offers remain explicit within it.
    pub purchasing: Outcome<Value>,
    /// Every physical cut piece plus sourced extras; an available report can still have unknown mass.
    #[serde(skip_serializing_if = "Outcome::is_not_requested")]
    pub assembly_mass: Outcome<assembly_mass::Report>,
    /// Canonical potential vapor transport after this alternative's material binding; never an
    /// inferred garment coverage, actual sweating rate or heat source for another consumer.
    #[serde(skip_serializing_if = "Outcome::is_not_requested")]
    pub vapor: Outcome<ftx_pattern::material_vapor::Report>,
    /// Selected actual-plan numbers for side-by-side comparison, never fabricated for failed cells.
    pub metrics: Metrics,
}

/// Recomputed study. No cached body pose, preceding material or old machine feasibility is reused.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Stable report format.
    pub schema: &'static str,
    /// Original pattern identity, before any optional design alteration or material binding.
    pub source_pattern_sha256: String,
    /// Full authored study request identity, including machine policy and offer history.
    pub request_sha256: String,
    /// Retained screening result, including excluded/unknown candidates, when screening was requested.
    pub screening: Option<material_screen::Report>,
    /// Finite-grid coverage or bounded adaptive discovery; absent for hand-authored alternatives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_search: Option<design::SearchReceipt>,
    /// Key-ordered alternatives, never sorted by a fabricated weighted score.
    pub alternatives: Vec<Alternative>,
    /// Present only when a comparison was requested; failed/unknown cells are never zero scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Report>,
    /// True when any retained design produced changed piece geometry/marks, even if later refused.
    /// Always false in fixed-pattern material studies.
    pub geometry_changed: bool,
    /// Always false, regardless of whether individual planners returned successfully.
    pub hardware_execution_authorized: bool,
    /// Always false: this is software evaluation, not measured seam/garment validation.
    pub physical_validation_claim: bool,
    /// Scope of the metrics and the intentionally missing empirical claims.
    pub notes: Vec<&'static str>,
}

/// Additional production-consumer results, evaluated on the same source-locked alternatives.
/// The export crate knows nothing about cloth/body/browser types; application layers supply the
/// existing consumer, not another binding or manufacturing implementation.
#[derive(Debug, Serialize)]
pub struct ScenarioStudy<T> {
    /// `ftx-material-scenario-study/1`, or `ftx-design-scenario-study/1` for explicit alterations.
    pub schema: &'static str,
    /// Identity of the original pattern, study policy and exact supplied scenario descriptor.
    pub request_sha256: String,
    /// Body, pose, environment and numerical policy retained by the caller's production consumer.
    pub scenario: Value,
    /// Unmodified manufacturing outcomes; unavailable simulation never erases them.
    pub manufacturing: Report,
    /// Key-ordered additional results, with binding failures retained rather than skipped.
    pub simulations: Vec<ScenarioAlternative<T>>,
    /// Declared comparison across the actual independent manufacturing and scenario consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<comparison::Report>,
}

/// One additional evaluation on exactly the pattern retained in its manufacturing binding.
#[derive(Debug, Serialize)]
pub struct ScenarioAlternative<T> {
    /// The matching manufacturing alternative's key.
    pub key: String,
    /// Independent production-consumer output or structured refusal.
    pub evaluation: Outcome<T>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}
fn hash<T: Serialize>(value: &T) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?,
    ))
}

impl Request {
    fn is_design(&self) -> bool {
        self.schema == "ftx-design-study/1"
    }

    fn sewing_options(&self) -> Option<&SewOptions> {
        self.sewing.as_ref().or_else(|| {
            self.sewing_selection
                .as_ref()
                .map(|selection| &selection.options)
        })
    }

    fn manufacturing_identity(
        &self,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<String, Refusal> {
        if self.design_fit.is_some() {
            hash(&(self, fit.map(|context| context.identity)))
        } else {
            hash(self)
        }
    }

    fn scenario_identity(
        &self,
        pattern: &Pattern,
        scenario: &Value,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<String, Refusal> {
        if self.design_fit.is_some() {
            hash(&(
                pattern.raw(),
                self,
                scenario,
                fit.map(|context| context.identity),
            ))
        } else {
            hash(&(pattern.raw(), self, scenario))
        }
    }

    fn purchasing_currency(&self) -> Option<&str> {
        self.purchasing
            .as_ref()
            .map(|p| p.market.currency.as_str())
            .or_else(|| {
                self.cutting_job
                    .as_ref()?
                    .purchasing
                    .as_ref()
                    .map(|p| p.market.currency.as_str())
            })
    }

    fn variants(
        &self,
        pattern: &Pattern,
        additional_consumer: bool,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<PreparedStudy, Refusal> {
        if let Some(policy) = &self.comparison {
            policy.validate(self, additional_consumer)?;
        }
        if let Some(mass) = &self.assembly_mass {
            mass.validate()?;
        }
        design::validate_request(self)?;
        if (!self.is_design() && self.schema != "ftx-material-study/1")
            || usize::from(self.screened.is_some())
                + usize::from(self.combinations.is_some())
                + usize::from(!self.variants.is_empty())
                != 1
            || (!additional_consumer
                && self.cutting.is_none()
                && self.finite_stock.is_none()
                && self.cutting_job.is_none()
                && self.sewing.is_none()
                && self.sewing_selection.is_none()
                && self.assembly_mass.is_none()
                && self.vapor.is_none())
            || (self.purchasing.is_some() && self.cutting.is_none())
            || (self.cutting_job.is_some()
                && (self.cutting.is_some()
                    || self.finite_stock.is_some()
                    || self.purchasing.is_some()))
            || (self.sewing.is_some() && self.sewing_selection.is_some())
        {
            return Err(invalid(
                "study needs its schema, exactly one variant source, at least one consumer, cutting when pricing, at most one of sewing or sewing_selection, and cutting_job exclusive with separate cutting/finite_stock/purchasing",
            ));
        }
        if self.variants.len() > 32 {
            return Err(budget("material study exceeds 32 alternatives"));
        }
        if let Some(combinations) = &self.combinations {
            let (variants, design_search) =
                design::expand(self, pattern, combinations.variants(pattern)?, fit)?;
            return Ok(PreparedStudy {
                variants,
                screening: None,
                design_search,
            });
        }
        let Some(screened) = &self.screened else {
            let (variants, design_search) = design::expand(
                self,
                pattern,
                self.variants.iter().cloned().map(Into::into).collect(),
                fit,
            )?;
            return Ok(PreparedStudy {
                variants,
                screening: None,
                design_search,
            });
        };
        if let Target::Pieces { ids } = &screened.target {
            let mut unique = BTreeSet::new();
            if ids.is_empty()
                || ids.len() > 4096
                || ids.iter().any(|id| {
                    !unique.insert(id)
                        || !pattern.raw()["pieces"]
                            .as_array()
                            .is_some_and(|pieces| pieces.iter().any(|p| p["id"] == *id))
                })
            {
                return Err(invalid(
                    "screening target needs unique, existing source-piece IDs",
                ));
            }
        }
        let report = screened.request.evaluate(&textile_import::catalog()?)?;
        let variants = report
            .alternatives
            .iter()
            .filter(|a| a.eligible && (!screened.frontier_only || a.dominated_by.is_empty()))
            .map(|a| {
                let selection = a
                    .selection
                    .as_ref()
                    .expect("eligible screening selection")
                    .clone();
                let mut changes = Changes {
                    schema: "ftx-material-bindings/1".into(),
                    ..Changes::default()
                };
                match &screened.target {
                    Target::Default => changes.default = Some(selection),
                    Target::Pieces { ids } => {
                        changes.pieces = ids
                            .iter()
                            .map(|id| (id.clone(), selection.clone()))
                            .collect()
                    }
                }
                Variant {
                    key: a.key.clone(),
                    changes,
                }
                .into()
            })
            .collect();
        let (variants, design_search) = design::expand(self, pattern, variants, fit)?;
        Ok(PreparedStudy {
            variants,
            screening: Some(report),
            design_search,
        })
    }

    /// Run each substitution and optional explicit alteration from the same source and process policy.
    ///
    /// # Errors
    /// Invalid source/request, ambiguous identities or excessive aggregate work. Material/geometry
    /// feasibility errors belong to the individual consumer outcomes, not an invented aggregate pass.
    pub fn evaluate(&self, pattern: &Pattern) -> Result<Report, Refusal> {
        self.evaluate_manufacturing(pattern, false, None)
    }

    /// Evaluate manufacturing and explicit flat targets against one fixed caller-supplied body.
    /// Production adapters supply the selected model's reference-pose measurements and identity.
    ///
    /// # Errors
    /// The same request/budget refusals as [`Self::evaluate`]. Unavailable or out-of-band fit
    /// evidence stays in each design's retained refusal and cannot supply a winning plan.
    pub fn evaluate_with_fit_body(
        &self,
        pattern: &Pattern,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<Report, Refusal> {
        self.evaluate_manufacturing(pattern, false, fit)
    }

    /// Compose an additional production consumer with the existing immutable material study.
    /// Each successful binding is evaluated once, in key order, on its retained source-complete
    /// pattern. The callback must start fresh, not warm-start from the preceding alternative.
    /// Manufacturing can be omitted because an additional consumer is explicitly requested here.
    ///
    /// # Errors
    /// The same structural/aggregate-budget refusals as [`Self::evaluate`], or a scenario that
    /// cannot serialize. Binding and callback failures remain independent per-alternative outcomes.
    pub fn evaluate_with<T: Serialize, S: Serialize>(
        &self,
        pattern: &Pattern,
        scenario: &S,
        evaluate: impl FnMut(&str, &Pattern) -> Result<T, Refusal>,
    ) -> Result<ScenarioStudy<T>, Refusal> {
        self.evaluate_with_fit(pattern, scenario, None, evaluate)
    }

    /// Compose production consumers after the same fixed-body flat-fit gate used by manufacturing.
    /// Each design/material candidate uses the original source and the same immutable body context.
    ///
    /// # Errors
    /// The same structural, numerical-budget and serialization refusals as [`Self::evaluate_with`].
    pub fn evaluate_with_fit<T: Serialize, S: Serialize>(
        &self,
        pattern: &Pattern,
        scenario: &S,
        fit: Option<&design::fit::Context<'_>>,
        mut evaluate: impl FnMut(&str, &Pattern) -> Result<T, Refusal>,
    ) -> Result<ScenarioStudy<T>, Refusal> {
        let manufacturing = self.evaluate_manufacturing(pattern, true, fit)?;
        let scenario = serde_json::to_value(scenario).map_err(|e| invalid(e.to_string()))?;
        let request_sha256 = self.scenario_identity(pattern, &scenario, fit)?;
        let mut simulations = Vec::with_capacity(manufacturing.alternatives.len());
        for row in &manufacturing.alternatives {
            let evaluation = match &row.binding {
                Outcome::Available { result } => {
                    // Keep Rust's parsed number representation. A JavaScript round-trip through
                    // stringify would alter source identities such as 1.0 or negative zero.
                    let bound = Pattern::new(result["pattern"].clone());
                    Outcome::from_result(evaluate(&row.key, &bound))
                }
                Outcome::Refused { refusal } => Outcome::Refused {
                    refusal: refusal.clone(),
                },
                Outcome::NotRequested => Outcome::Refused {
                    refusal: invalid("additional evaluation requires a successful binding"),
                },
            };
            simulations.push(ScenarioAlternative {
                key: row.key.clone(),
                evaluation,
            });
        }
        let comparison = self
            .comparison
            .as_ref()
            .map(|policy| {
                policy.evaluate_scenarios(
                    &manufacturing.alternatives,
                    &simulations,
                    &scenario,
                    &manufacturing.source_pattern_sha256,
                    &request_sha256,
                    self.purchasing_currency(),
                )
            })
            .transpose()?;
        Ok(ScenarioStudy {
            schema: if self.is_design() {
                "ftx-design-scenario-study/1"
            } else {
                "ftx-material-scenario-study/1"
            },
            request_sha256,
            scenario,
            manufacturing,
            simulations,
            comparison,
        })
    }

    fn evaluate_manufacturing(
        &self,
        pattern: &Pattern,
        additional_consumer: bool,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<Report, Refusal> {
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        let prepared = self.variants(pattern, additional_consumer, fit)?;
        self.evaluate_prepared(pattern, additional_consumer, prepared, fit)
    }

    fn evaluate_prepared(
        &self,
        pattern: &Pattern,
        additional_consumer: bool,
        prepared: PreparedStudy,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<Report, Refusal> {
        let PreparedStudy {
            mut variants,
            screening,
            design_search,
        } = prepared;
        if variants.len() > 32 {
            return Err(budget(
                "constrain screening or supply at most 32 material alternatives",
            ));
        }
        let mut keys = BTreeSet::new();
        if variants
            .iter()
            .any(|v| v.variant.key.trim().is_empty() || !keys.insert(v.variant.key.clone()))
        {
            return Err(invalid("material variant keys must be unique and nonempty"));
        }
        // A malformed zero-copy cutting request must not bypass admission of other consumers.
        let copies = self
            .cutting
            .as_ref()
            .map_or(0, |c| c.copies)
            .saturating_add(self.finite_stock.as_ref().map_or(0, |s| s.options.copies))
            .saturating_add(self.cutting_job.as_ref().map_or(0, job::Request::copies))
            .max(1);
        if pattern
            .instances
            .len()
            .checked_mul(copies)
            .and_then(|n| n.checked_mul(variants.len()))
            .is_none_or(|n| n > 16_384)
            || self
                .sewing
                .as_ref()
                .is_some_and(|s| s.passes.len().saturating_mul(variants.len()) > 4096)
            || self.assembly_mass.as_ref().is_some_and(|m| {
                // Invalid attachment inventories remain independent per-row mass refusals.
                // Retain the existing explicit-extra budget when no merged work can run.
                m.merged_inventory_count(pattern)
                    .unwrap_or(m.extras.len())
                    .saturating_mul(variants.len())
                    > 4096
            })
        {
            return Err(budget(
                "material study exceeds aggregate instance/pass/extra inventory budget",
            ));
        }
        if self.finite_stock.as_ref().is_some_and(|stock| {
            stock.options.work_limit.saturating_mul(variants.len()) > 20_000_000
        }) || self
            .cutting_job
            .as_ref()
            .and_then(|j| j.finite_stock.as_ref())
            .is_some_and(|stock| {
                stock.options.work_limit.saturating_mul(variants.len()) > 20_000_000
            })
        {
            return Err(budget(
                "material study exceeds 20000000 aggregate finite-stock work units; reduce alternatives or the per-plan work limit",
            ));
        }
        if let Some(selection) = &self.sewing_selection {
            recipes::check_budget(selection, variants.len())?;
        }
        if let Some(options) = self.sewing_options() {
            let configured_work = self.sewing_selection.as_ref().map_or_else(
                || crate::sewing::consumables::purchasing_work_bound(options),
                sewing_selection::Request::purchasing_work_bound,
            );
            let work = match configured_work {
                Ok(work) => work,
                Err(refusal) if refusal.code == RefusalCode::BudgetExceeded => return Err(refusal),
                // Invalid source/market options remain the independent sewing consumer's refusal;
                // its admission fails before the purchasing evaluator can perform quote work.
                Err(_) => 0,
            };
            if work.saturating_mul(variants.len()) > 200_000 {
                return Err(budget(
                    "material/design study exceeds 200000 aggregate thread-purchasing work units; reduce variants, recipe trials or quote observations",
                ));
            }
        }
        let allocation_work = [
            self.purchasing.as_ref().map(|p| &p.market),
            self.cutting_job
                .as_ref()
                .and_then(|j| j.purchasing.as_ref())
                .map(|p| &p.market),
        ]
        .into_iter()
        .flatten()
        .filter_map(|market| market.split_orders.as_ref())
        .fold(0usize, |sum, policy| {
            sum.saturating_add(policy.max_search_nodes.saturating_mul(variants.len()))
        });
        if allocation_work > 200_000 {
            return Err(budget(
                "material study exceeds 200000 aggregate supplier-allocation work units; reduce alternatives or each split_orders limit",
            ));
        }
        variants.sort_by(|a, b| a.variant.key.cmp(&b.variant.key));
        let mut alternatives = Vec::new();
        let mut stations = 0usize;
        let mut mass_work = Work::new(40_000_000);
        let mut design_work = Work::new(40_000_000);
        for variant in variants {
            let row =
                self.evaluate_variant(pattern, variant, &mut mass_work, &mut design_work, fit)?;
            if design_work.exhausted {
                return Err(budget(
                    "design study exceeds 40000000 aggregate outline-validation work units",
                ));
            }
            if mass_work.exhausted {
                return Err(budget(
                    "material study exceeds aggregate mass geometry budget",
                ));
            }
            if let Some(sewing) = row.sewing.value() {
                stations = stations.saturating_add(
                    sewing
                        .seams
                        .iter()
                        .map(|s| s.stations.a.len())
                        .sum::<usize>(),
                );
                if stations > 250_000 {
                    return Err(budget(
                        "material study exceeds 250000 aggregate sewing stations",
                    ));
                }
            }
            alternatives.push(row);
        }
        let request_sha256 = self.manufacturing_identity(fit)?;
        let source_pattern_sha256 = hash(pattern.raw())?;
        let comparison = self
            .comparison
            .as_ref()
            .filter(|_| !additional_consumer)
            .map(|policy| {
                policy.evaluate(
                    &alternatives,
                    &source_pattern_sha256,
                    &request_sha256,
                    self.purchasing_currency(),
                )
            });
        let mut notes = vec![
            if self.is_design() {
                "Each explicit alteration sequence and its unaltered baseline start from the same original pattern, then cross every requested material alternative. Canonical alteration operators, flat validation and sampled outline checks run before manufacturing or additional consumers; no seam repair or fit objective is inferred. Frozen changes replay on the retained altered_pattern_json."
            } else {
                "All variants start from the same source geometry and process options. Default replacements preserve explicit piece assignments; named-piece replacements do not change unrelated pieces."
            },
            "Scalar-screening dominance is not garment/marker dominance. Frontier-only pruning happens solely when explicitly requested; manufacturing outputs are not assigned a hidden aggregate score.",
            "Missing mass or offer coverage stays unknown. Stock mass includes waste, feed time excludes handling, and purchased quantities come from actual roll markers, not net pattern area.",
            "Binding and available plans are not a successful drape, empirical fit, thermal-comfort prediction, seam qualification or permission to operate machinery. Source-locked pattern_json can be passed to the existing drape/thermal/export consumers.",
        ];
        if self.assembly_mass.is_some() {
            notes.push("Declared assembly mass counts one pre-finishing cut inventory plus explicitly sourced retained extras. Cutting copies multiply purchased stock, not this one-assembly mass; finishing changes and unrepresented components are not inferred.");
        }
        if self.finite_stock.is_some() {
            notes.push("Finite-stock alternatives independently start with the same physical inventory. Used-item area/mass includes defects and prior occupied regions; newly allocated cut area is separate. Alternatives neither reserve inventory nor create hide purchase quotes.");
        }
        if self.cutting_job.is_some() {
            notes.push("The cutting job routes every original physical instance and copy exactly once across roll and finite stock. Its gross stock includes waste, defects and prior cuts; complete purchasing adds whole roll orders and used physical items only when all same-market prices are known. No stock is reserved and no machine is driven.");
        }
        if self.combinations.is_some() {
            notes.push("Every declared material combination is evaluated through the same production consumers. The frontier is conditional on this finite choice set and the declared comparison policy, not a global material/design optimum. Explicit interface pools select exact assigned snapshot pairs only; missing pair evidence remains a consumer refusal.");
        }
        if self.sewing_selection.is_some() {
            notes.push("Each material alternative reruns the declared finite qualified sewing recipe search. Sewing metrics describe its complete selected incumbent, including when the search budget stops before an optimum is proven. No selected plan keeps sewing unavailable; independent consumers are preserved. Recipe qualification and manufacturing hardware permission remain separate.");
        }
        if self
            .sewing_options()
            .is_some_and(|s| s.consumables.is_some())
        {
            notes.push("Thread demand uses the selected plan's actual traversals, independently qualified channel coefficients and once-per-operation allowances. Each exact yarn is aggregated before canonical whole-order pricing. Missing consumption or offers keep the complete metric unknown. Thread purchasing uses its own declared market and describes one sewing job, independently of cutting copies; it is not retained assembly mass or a combined cut-and-sew cost. Recipe selection follows its explicit declared objective; only thread-cost selection searches these order totals, and its separate proof and unresolved-trial status remain authoritative.");
        }
        if self.vapor.is_some() {
            notes.push("Potential vapor transport uses explicit common-area layer occurrences resolved from each final material assignment. It infers no overlap, body coverage, air-gap resistance, sweat supply, evaporation or coupled heat; canonical source qualifications and boundary assumptions remain in every available result.");
        }
        if self.is_design() {
            notes.push("Slash-spread and cap-ease preserve the canonical operator's mark behavior: grain/internal/decor/trace marks move, while notches, drills and placement anchors do not. Move/true-seam edit drafted vertices only. Complete flat validation includes seam walk, free-edge truing and cut/grain completeness; it does not establish wearer fit or empirical sewing quality. The finite declared design/material frontier is not a continuous or global optimum.");
        }
        if self.design_grid.is_some() {
            notes.push(if self.design_grid.as_ref().is_some_and(|grid| grid.fit_screening.is_some()) {
                "Grid variables generate linked canonical alterations in authored order, with the last variable changing fastest. The baseline is additional to the explicit design allowance. Optional fit screening inspects its declared larger prefix once before material expansion and selects its first passing candidates; failures and qualified-but-unselected points remain in the screening receipt. Unvisited points and downstream outcomes of unselected passing points remain unknown; the frontier is conditional on evaluated candidates, never a continuous or global optimum."
            } else {
                "Grid variables generate linked canonical alterations in authored order, with the last variable changing fastest. The baseline is additional to the explicit design allowance. Unevaluated grid points remain unknown; a budget-limited frontier is conditional on the evaluated prefix, never a continuous or global optimum."
            });
        }
        if self.design_fit.is_some() {
            notes.push("Explicit fit targets use the fixed caller-supplied body authority through canonical flat validation before binding or any production consumer. Model-derived measurements retain MODEL_FIT provenance; missing points or out-of-band raw values refuse the design even when a rounded residual is zero. Passing a declared ease band is not empirical garment fit or a safe-fit certification.");
        }
        if self.design_adaptive.is_some() {
            notes.push("Adaptive linked alterations use explicitly bounded coordinate polling and step refinement. Their declared fit-band objective compares unrounded canonical maximum violation, then total violation, solely to discover candidates. Every selected design still requires complete flat admission and independently reruns the requested material and production consumers. Stopping at the candidate allowance, evaluation budget or minimum steps leaves the continuous domain unresolved; no global optimum, infeasibility or empirical fit is inferred.");
        }
        let geometry_changed = alternatives.iter().any(|row| {
            row.design
                .as_ref()
                .is_some_and(|design| design.geometry_changed == Some(true))
        });
        Ok(Report {
            schema: if self.is_design() {
                "ftx-design-study-report/1"
            } else {
                "ftx-material-study-report/1"
            },
            source_pattern_sha256,
            request_sha256,
            screening,
            design_search,
            alternatives,
            comparison,
            geometry_changed,
            hardware_execution_authorized: false,
            physical_validation_claim: false,
            notes,
        })
    }

    fn evaluate_variant(
        &self,
        source: &Pattern,
        prepared: PreparedVariant,
        mass_work: &mut Work,
        design_work: &mut Work,
        fit: Option<&design::fit::Context<'_>>,
    ) -> Result<Alternative, Refusal> {
        let mut variant = prepared.variant;
        let (design, altered) = if let Some(design) = prepared.design {
            let seam_tolerance = self
                .sewing
                .as_ref()
                .or_else(|| self.sewing_selection.as_ref().map(|s| &s.options))
                .map_or(3.0, |s| s.seam_tolerance_mm);
            let (receipt, pattern) = design.apply(
                source,
                seam_tolerance,
                design_work,
                self.design_fit.as_ref(),
                fit,
            )?;
            (Some(receipt), Some(pattern))
        } else {
            (None, None)
        };
        let binding = (|| {
            let source = match &altered {
                Some(Ok(pattern)) => pattern,
                Some(Err(refusal)) => return Err(refusal.clone()),
                None => source,
            };
            if let Some(combinations) = &self.combinations
                && combinations.interfaces.is_some()
            {
                // Resolve actual effective assignments, including untouched pieces, before selecting
                // exact pair evidence. The ordinary binder validates the selected complete table.
                let bound = variant.changes.apply(source)?;
                variant.changes.interfaces = combinations.interfaces_for(&bound)?;
            }
            variant.changes.apply(source)
        })();
        let mut row = Alternative {
            key: variant.key,
            choices: prepared.choices,
            design,
            binding: Outcome::NotRequested,
            frozen_changes: None,
            cutting: Outcome::NotRequested,
            finite_stock: Outcome::NotRequested,
            cutting_job: Outcome::NotRequested,
            sewing: Outcome::NotRequested,
            sewing_selection: Outcome::NotRequested,
            purchasing: Outcome::NotRequested,
            assembly_mass: Outcome::NotRequested,
            vapor: Outcome::NotRequested,
            metrics: Metrics::default(),
        };
        let bound = match binding {
            Ok(result) => result,
            Err(refusal) => {
                if self.cutting.is_some() {
                    row.cutting = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.finite_stock.is_some() {
                    row.finite_stock = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.cutting_job.is_some() {
                    row.cutting_job = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.sewing.is_some() || self.sewing_selection.is_some() {
                    row.sewing = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.sewing_selection.is_some() {
                    row.sewing_selection = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.purchasing.is_some() {
                    row.purchasing = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.assembly_mass.is_some() {
                    row.assembly_mass = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                if self.vapor.is_some() {
                    row.vapor = Outcome::Refused {
                        refusal: refusal.clone(),
                    };
                }
                row.binding = Outcome::Refused { refusal };
                return Ok(row);
            }
        };
        let pattern = Pattern::new(bound["pattern"].clone());
        row.frozen_changes = Some(freeze_changes(
            pattern.raw(),
            variant.changes.interfaces,
            bound.get("frozen_elastic_casings"),
        )?);
        if let Some(request) = &self.vapor {
            row.vapor = Outcome::from_result(request.evaluate(&pattern));
        }
        if let Some(options) = &self.cutting {
            row.cutting = Outcome::from_result(CutPlan::build(&pattern, options));
        }
        if let Some(request) = &self.finite_stock {
            row.finite_stock = Outcome::from_result(request.build(&pattern));
        }
        if let Some(request) = &self.cutting_job {
            row.cutting_job = Outcome::from_result(request.build(&pattern));
        }
        if let Some(options) = &self.sewing {
            row.sewing = Outcome::from_result(SewPlan::build(&pattern, options));
        }
        if let Some(request) = &self.sewing_selection {
            (row.sewing, row.sewing_selection) = recipes::evaluate(&pattern, request);
        }
        if let Some(request) = &self.assembly_mass {
            let mass = (|| {
                // All cutting consumers must describe the same cut outline. Compare the effective
                // exporter defaults, allowing None versus an explicitly equal value.
                for cut_allowance in self
                    .cutting
                    .as_ref()
                    .map(|c| c.seam_allowance_mm)
                    .into_iter()
                    .chain(
                        self.finite_stock
                            .as_ref()
                            .map(|s| s.options.seam_allowance_mm),
                    )
                    .chain(
                        self.cutting_job
                            .as_ref()
                            .and_then(|j| j.roll.as_ref())
                            .map(|o| o.seam_allowance_mm),
                    )
                    .chain(
                        self.cutting_job
                            .as_ref()
                            .and_then(|j| j.finite_stock.as_ref())
                            .map(|r| r.options.seam_allowance_mm),
                    )
                {
                    let allowance = |sa| {
                        Ctx::new(
                            &pattern,
                            ExportOptions {
                                sa,
                                ..ExportOptions::default()
                            },
                        )
                        .map(|ctx| ctx.sa)
                    };
                    if allowance(request.seam_allowance_mm)? != allowance(cut_allowance)? {
                        return Err(invalid(
                            "assembly mass and cutting need the same effective seam allowance",
                        ));
                    }
                }
                request.evaluate_with_work(&pattern, mass_work)
            })();
            row.assembly_mass = Outcome::from_result(mass);
            row.metrics.assembly_mass_kg =
                row.assembly_mass.value().and_then(|r| r.assembly_mass.kg);
        }
        if let Some(purchasing) = &self.purchasing {
            row.purchasing = match &row.cutting {
                Outcome::Available { result } => Outcome::from_result(procurement::evaluate(
                    result,
                    &purchasing.quotes,
                    &purchasing.market,
                )),
                Outcome::Refused { refusal } => Outcome::Refused {
                    refusal: refusal.clone(),
                },
                Outcome::NotRequested => return Err(invalid("pricing requires cutting")),
            };
        }
        if let Some(plan) = row.cutting.value() {
            row.metrics.stock_area_m2 =
                Some(finite_sum(plan.rolls.iter().map(|r| r.stock_area_m2))?);
            row.metrics.external_cut_area_m2 = Some(finite_sum(
                plan.rolls.iter().map(|r| r.external_cut_area_m2),
            )?);
            if plan.rolls.iter().all(|r| r.stock_mass_kg.is_some()) {
                row.metrics.stock_mass_kg = Some(finite_sum(
                    plan.rolls.iter().filter_map(|r| r.stock_mass_kg),
                )?);
            }
        }
        if let Some(plan) = row.sewing.value() {
            row.metrics.stitch_intervals = Some(plan.stitch_intervals);
            row.metrics.feed_seconds = Some(plan.feed_seconds);
            if let Some(consumables) = plan
                .consumables
                .as_ref()
                .filter(|r| r.complete_consumption_coverage)
            {
                row.metrics.thread_required_length_m = consumables.total_required_length_m;
                row.metrics.thread_observed_order_cost = consumables
                    .purchasing
                    .as_ref()
                    .filter(|p| p.complete_offer_coverage)
                    .and_then(|p| p.least_observed_independent_order_total);
            }
            if let Some(timing) = plan
                .ordering
                .as_ref()
                .and_then(|order| order.timing.as_ref())
            {
                row.metrics.sewing_setup_seconds =
                    Some(timing.selected_setup_milliseconds as f64 / 1000.0);
                row.metrics.sewing_feed_and_setup_seconds = Some(timing.feed_and_setup_seconds);
            }
        }
        if let Some(plan) = row.finite_stock.value() {
            let used = || plan.markers().iter().filter(|m| !m.placements.is_empty());
            row.metrics.used_stock_count = Some(used().count());
            row.metrics.used_stock_gross_area_m2 =
                Some(finite_sum(used().map(|m| m.gross_area_m2))?);
            row.metrics.finite_stock_external_cut_area_m2 =
                Some(finite_sum(used().map(|m| m.external_cut_area_m2))?);
            if used().all(|m| m.stock_mass_kg.is_some()) {
                row.metrics.used_stock_mass_kg =
                    Some(finite_sum(used().filter_map(|m| m.stock_mass_kg))?);
            }
        }
        if let Some(plan) = row.cutting_job.value() {
            row.metrics.cutting_job_external_cut_area_m2 = Some(plan.external_cut_area_m2);
            row.metrics.cutting_job_stock_area_m2 = Some(plan.stock_area_m2);
            row.metrics.cutting_job_stock_mass_kg = plan.stock_mass_kg;
            row.metrics.cutting_job_observed_order_cost = plan
                .purchasing
                .as_ref()
                .filter(|p| p.complete_offer_coverage)
                .and_then(|p| p.least_observed_independent_order_total);
            row.metrics.cutting_job_selected_order_cost = plan
                .purchasing
                .as_ref()
                .filter(|p| p.complete_offer_coverage)
                .and_then(|p| p.selected_observed_order_total);
        }
        if let Some(priced) = row.purchasing.value() {
            row.metrics.least_observed_independent_order_total =
                priced["least_observed_independent_order_total"].as_f64();
            row.metrics.selected_observed_order_total =
                if priced["schema"] == "ftx-cut-procurement/3" {
                    priced["selected_observed_order_total"].as_f64()
                } else {
                    row.metrics.least_observed_independent_order_total
                };
        }
        row.binding = Outcome::Available { result: bound };
        Ok(row)
    }
}

fn finite_sum(values: impl Iterator<Item = f64>) -> Result<f64, Refusal> {
    let sum: f64 = values.sum();
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(invalid("manufacturing metric sum overflows"))
    }
}

fn freeze_changes(
    pattern: &Value,
    interfaces: Option<Vec<Value>>,
    elastic_casings: Option<&Value>,
) -> Result<Changes, Refusal> {
    let selection = |value: &Value| -> Result<Selection, Refusal> {
        serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))
    };
    let mut pieces = BTreeMap::new();
    for piece in pattern["pieces"]
        .as_array()
        .ok_or_else(|| invalid("bound pattern needs pieces"))?
    {
        if let Some(fabric) = piece.get("fabric") {
            pieces.insert(
                piece["id"]
                    .as_str()
                    .ok_or_else(|| invalid("piece ID must be text"))?
                    .into(),
                selection(&fabric["material"])?,
            );
        }
    }
    Ok(Changes {
        schema: "ftx-material-bindings/1".into(),
        default: Some(selection(&pattern["fabric"]["material"])?),
        pieces,
        // Only an explicit replacement needs replaying. Inherited raw evidence remains in the
        // same original source; omitting this field preserves existing request/report identities.
        interfaces,
        elastic_casings: elastic_casings
            .map(|value| serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string())))
            .transpose()?
            .unwrap_or_default(),
    })
}

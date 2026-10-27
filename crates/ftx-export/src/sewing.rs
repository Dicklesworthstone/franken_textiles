//! Material-following seam plans in the two original pattern-side frames (millimetres).
//! These are feed/stack/allowance checks, not robot trajectories or seam-quality certification.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::materials::{Evidence, Form, Source};
use ftx_pattern::textile_import::{piece_fabric, resolve_fabric};
use ftx_pattern::{Pattern, SeamPairs};
use serde::{Deserialize, Serialize};

use crate::{Ctx, ExportOptions, edge_sa};

pub mod access;
pub mod assembly;
pub mod consumables;
pub mod ordering;
pub mod pressure;
pub mod preview;
pub mod production;
pub mod program;
use assembly::{AssemblyReceipt, FlatAssembly};
use program::{Backtack, FeedDirection, ReverseFeed, StitchRun};
pub mod path;
use path::{OffsetPath, PathReceipt};
pub mod span;
use span::SourceSpan;

const MAX_OPERATIONS: usize = 4096;
const MAX_STATIONS: usize = 250_000;

/// User-declared machine envelope. Admission is not proof of machine calibration or safe execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewingMachine {
    /// Specific machine/configuration identifier, retained verbatim.
    pub id: String,
    /// Permitted advance per stitch on each side, [minimum, maximum] in mm.
    pub stitch_mm: [f64; 2],
    /// Permitted side-b / side-a advance ratio, not its reciprocal.
    pub feed_b_over_a: [f64; 2],
    /// Maximum uncompressed thickness of the declared ply stack, in mm.
    pub max_stack_mm: f64,
    /// Maximum continuous stitch rate; handling and acceleration are not modeled.
    pub max_stitches_per_minute: f64,
    /// Explicit reverse-feed envelope; omission refuses every reverse movement, including locks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<ReverseFeed>,
    /// Separate loaded-stack and prescribed-pressure envelope; never replaces unloaded clearance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presser_foot: Option<pressure::FootLimits>,
}

/// One declared pass over an existing seam. The passes must cover both complete source sides.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewingPass {
    /// Unique operation ID. Omission uses the seam ID for legacy one-pass jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Exact pattern seam ID.
    pub seam: String,
    /// Optional [start, end] in the original side-A arclength fractions, before any path offset.
    /// B is paired by the existing notch correspondence; omission covers the whole seam.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<[f64; 2]>,
    /// Upper bound on each side's stitch advance, not a guarantee of uniform pitch.
    pub max_stitch_mm: f64,
    /// Legacy declared ply counts. Supply this OR an explicit flat-fold assembly, never both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plies: Option<[u32; 2]>,
    /// Placed/folded source pieces from which the actual needle stack is derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<FlatAssembly>,
    /// Required allowance on both sides, in mm; no fold or seam recipe is guessed.
    pub minimum_allowance_mm: f64,
    /// Requested continuous stitch rate.
    pub stitches_per_minute: f64,
    /// Operator-selected needle identifier; suitability remains unvalidated.
    pub needle: String,
    /// Operator-selected thread identifier; consumption/strength are not inferred.
    pub thread: String,
    /// Traversal through the canonical paired station table, not the pattern seam's b-side reversal.
    #[serde(default, skip_serializing_if = "FeedDirection::is_forward")]
    pub direction: FeedDirection,
    /// Explicit start/end lock lengths in existing station intervals; never inferred from material.
    #[serde(default, skip_serializing_if = "Backtack::is_empty")]
    pub backtack: Backtack,
    /// Optional paired offset stitch lines. Omission preserves the original seam station table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stitch_path: Option<OffsetPath>,
    /// Optional measured compression evaluation of every ply at the same prescribed foot pressure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presser_foot: Option<pressure::Request>,
    /// Optional source-qualified planar tool/fixture clearance in the explicit assembly frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<access::Policy>,
    /// Operation IDs which must finish first. Omission adds no physical-accessibility claim.
    #[serde(default)]
    pub after: Vec<String>,
}

/// Complete sewing job policy, independent of simulation parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewOptions {
    /// Declared machine configuration.
    pub machine: SewingMachine,
    /// Explicit passes covering every seam, including seams omitted from a simulation.
    pub passes: Vec<SewingPass>,
    /// Optional bounded ordering by setup-change counts or source-qualified directed setup time.
    /// Omission preserves the existing lexicographic topological order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordering: Option<ordering::Policy>,
    /// Optional cross-garment schedule on this machine. Exclusive with one-garment `ordering`.
    /// The qualified geometry remains once per pass; production operations reference copy and pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production: Option<production::Policy>,
    /// Source-bearing process envelopes tied to exact resolved material stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualifications: Vec<SewingQualification>,
    /// Require every pass to match exactly one qualification; false preserves legacy planning.
    #[serde(default, skip_serializing_if = "is_false")]
    pub require_qualification: bool,
    /// Optional source-qualified thread inventory and dated purchasing from actual traversals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumables: Option<consumables::Request>,
    /// Default flat seam-walk tolerance in mm; existing per-seam tolerances take precedence.
    pub seam_tolerance_mm: f64,
    /// Default export allowance override; edge and piece allowances still take precedence.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
}

impl SewOptions {
    /// Conservative configured scheduling-work reservation for one complete recipe evaluation.
    /// Includes the serial reference and the parallel policy's entire shared inner/allocation cap.
    /// It stays conservative when a later manufacturing consumer inherits a larger cutting count.
    ///
    /// # Errors
    /// Conflicting quantities, invalid schedule bounds or more than 200000 total work units.
    pub fn scheduling_work_bound(&self) -> Result<usize, Refusal> {
        let resolved = production::resolve_options(self)?;
        if let Some(production) = &resolved.production {
            return production.parallel.as_ref().map_or_else(
                || Ok(production.ordering.max_search_nodes),
                |parallel| {
                    parallel.work_bound(
                        production.production_copies.unwrap_or(1),
                        production.ordering.max_search_nodes,
                    )
                },
            );
        }
        Ok(resolved
            .ordering
            .as_ref()
            .map_or(0, |ordering| ordering.max_search_nodes))
    }

    /// Active scheduling policy. Production and one-garment ordering are exclusive at admission.
    #[must_use]
    pub fn ordering_policy(&self) -> Option<&ordering::Policy> {
        self.production
            .as_ref()
            .map(|production| &production.ordering)
            .or(self.ordering.as_ref())
    }
}

/// Source-locked process envelope from an external seam/coupon qualification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewingQualification {
    /// Stable qualification identity retained in each admitted planned operation.
    pub id: String,
    /// Exact bottom-to-top material snapshot sequence; legacy ply jobs use A plies then B plies.
    pub materials: Vec<String>,
    /// Exact machine/configuration identifier.
    pub machine: String,
    /// Exact needle identifier qualified by the source.
    pub needle: String,
    /// Exact thread identifier qualified by the source.
    pub thread: String,
    /// Qualified actual stitch-advance interval, millimetres, applied independently to both sides.
    pub stitch_mm: [f64; 2],
    /// Qualified local differential-feed interval, side-b / side-a.
    pub feed_b_over_a: [f64; 2],
    /// Qualified uncompressed material-stack interval, millimetres.
    pub stack_mm: [f64; 2],
    /// Qualified continuous stitch-rate interval.
    pub stitches_per_minute: [f64; 2],
    /// Whether reverse travel, including backtacks, was included in this qualification.
    #[serde(default)]
    pub allow_reverse: bool,
    /// A pressure-bearing operation needs a pressure-bearing qualification; omission is not consent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presser_foot: Option<pressure::Qualification>,
    /// Independently sourced consumption applicable only within this exact admitted envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_consumption: Option<consumables::Consumption>,
    /// Evidence class of this qualification record.
    pub evidence: Evidence,
    /// Exact external source or retained local measurement identity.
    pub source: Source,
    /// Coupon/test protocol; an ID alone is not process evidence.
    pub method: String,
    /// Conditioning and applicability metadata.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Explicit permission to use an estimated qualification.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Source material and minimum edge allowance for one side of a seam.
#[derive(Clone, Debug, Serialize)]
pub struct SewSide {
    /// Existing INSTANCE/EDGE+EDGE reference.
    pub reference: String,
    /// Frozen material key in [`SewPlan::materials`].
    pub material_sha256: String,
    /// Thickness used per ply (mm): reported upper bound when present, otherwise central value.
    pub thickness_mm: f64,
    /// True when thickness uses the reported upper bound; its statistical meaning is not changed.
    pub uses_thickness_upper_bound: bool,
    /// Smallest authored/export allowance over the side's edges (mm).
    pub allowance_mm: f64,
}

/// One scheduled seam, preserving the existing curve, mirror, reverse and notch correspondence.
#[derive(Clone, Debug, Serialize)]
pub struct PlannedSeam {
    /// Original user-declared recipe, not inferred from the fiber name.
    pub pass: SewingPass,
    /// Side a and side b, in that order.
    pub sides: [SewSide; 2],
    /// Corresponding needle stations in each original side's local mm frame, not a robot frame.
    /// Fractions and lengths retain their full-path reference, including on partial operations.
    pub stations: SeamPairs,
    /// Selected source coverage and traveled lengths; absent on legacy whole-seam operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceSpan>,
    /// Offset polylines and source correspondence; absent on legacy centerline operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathReceipt>,
    /// Shared needle coordinates and material-following plies in a declared flat-fold assembly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assembly: Option<AssemblyReceipt>,
    /// Side-b / side-a feed ratio for each interval between consecutive stations.
    pub feed_b_over_a: Vec<f64>,
    /// Declared or geometrically derived uncompressed stack thickness (mm).
    pub stack_mm: f64,
    /// Loaded ply stack and source receipts, separate from the original stack/assembly geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presser_foot: Option<pressure::Receipt>,
    /// Full circular tool sweep against declared static fixtures; no machine-frame inference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<access::Receipt>,
    /// Ordered directed runs indexing `stations`; present for version-2 jobs, including lock travel.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<StitchRun>,
    /// All traversed stitch intervals, counting each lock/reinforcement traversal separately.
    pub stitch_intervals: usize,
    /// Continuous feed time at the requested rate (seconds), excluding handling/acceleration.
    pub feed_seconds: f64,
    /// Exact source qualification admitted for this operation, absent on unqualified legacy plans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualification_id: Option<String>,
}

/// Deterministic, source-locked manufacturing review plan. Never hardware execution permission.
#[derive(Clone, Debug, Serialize)]
pub struct SewPlan {
    /// `/1` legacy, `/2` directed, `/3` offsets, `/4` assemblies, `/5` qualifications, `/6` ordering,
    /// `/7` pressure, `/8` clearance, `/9` timed ordering, `/10` partial source spans,
    /// `/11` source-qualified thread consumption and purchasing, `/12` cross-copy production schedule,
    /// `/13` source-qualified finite workholding across production copies,
    /// `/14` source-qualified planar tool/fixture clearance in explicit assemblies.
    /// Capabilities compose rather than replace.
    pub schema: &'static str,
    /// Digest of the complete original pattern JSON, including material assignments.
    pub pattern_sha256: String,
    /// Policy and declared machine limits used for this plan.
    pub options: SewOptions,
    /// Portable selections, indexed by resolved digest; no dependence on a future live catalog.
    pub materials: BTreeMap<String, Selection>,
    /// Admitted operation order; optional optimization preserves every declared predecessor.
    pub seams: Vec<PlannedSeam>,
    /// Setup counts, optional times and bounded-search result; absent without an ordering policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordering: Option<ordering::Report>,
    /// Complete repeated operation graph and selected batch order, with independently scoped totals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production: Option<production::Report>,
    /// Optional complete thread BOM; missing consumption never becomes a zero requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumables: Option<consumables::Report>,
    /// Total traversed stitch intervals, including locks; not thread consumption.
    pub stitch_intervals: usize,
    /// Sum of the continuous feed times, never a complete manufacturing cycle-time estimate.
    pub feed_seconds: f64,
    /// Always false: no robot coordinates, sensing, interlocks or calibration are supplied.
    pub hardware_execution_authorized: bool,
    /// Always false: no pucker, seam strength, needle penetration or wearer-fit validation.
    pub physical_seam_quality_validated: bool,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn qualified_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

fn sha256_text(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn range(x: [f64; 2]) -> bool {
    positive(x[0]) && positive(x[1]) && x[0] <= x[1]
}

fn within(x: f64, limits: [f64; 2]) -> bool {
    // Arithmetic tolerance only: not a concealed machine/process allowance.
    let eps = |limit: f64| 1e-10 * x.abs().max(limit.abs());
    positive(x) && x >= limits[0] - eps(limits[0]) && x <= limits[1] + eps(limits[1])
}

impl SewingQualification {
    fn validate(&self) -> Result<(), Refusal> {
        if let Some(foot) = &self.presser_foot {
            foot.validate()?;
        }
        if let Some(consumption) = &self.thread_consumption {
            consumption.validate()?;
        }
        if !qualified_text(&self.id)
            || self.materials.is_empty()
            || self.materials.len() > 128
            || self.materials.iter().any(|h| !sha256_text(h))
            || !qualified_text(&self.machine)
            || !qualified_text(&self.needle)
            || !qualified_text(&self.thread)
            || !range(self.stitch_mm)
            || !range(self.feed_b_over_a)
            || !range(self.stack_mm)
            || !range(self.stitches_per_minute)
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(k, v)| !qualified_text(k) || !qualified_text(v))
        {
            return Err(bad(format!("{}: invalid sewing qualification", self.id)));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(format!(
                "{}: estimated sewing qualification requires explicit consent",
                self.id
            )));
        }
        Ok(())
    }

    fn identity_matches(
        &self,
        materials: &[String],
        machine: &SewingMachine,
        pass: &SewingPass,
    ) -> bool {
        self.materials == materials
            && self.machine == machine.id
            && self.needle == pass.needle
            && self.thread == pass.thread
    }

    fn envelope_matches(
        &self,
        pass: &SewingPass,
        stack_mm: f64,
        advances: &[[f64; 2]],
        feeds: &[f64],
        has_reverse: bool,
        foot: Option<&pressure::Receipt>,
    ) -> bool {
        within(stack_mm, self.stack_mm)
            && within(pass.stitches_per_minute, self.stitches_per_minute)
            && (!has_reverse || self.allow_reverse)
            && advances
                .iter()
                .flatten()
                .all(|&advance| within(advance, self.stitch_mm))
            && feeds.iter().all(|&feed| within(feed, self.feed_b_over_a))
            && match (&self.presser_foot, foot) {
                (None, None) => true,
                (Some(envelope), Some(receipt)) => envelope.admits(receipt),
                _ => false,
            }
    }
}

impl SewingPass {
    /// Stable dependency identity; two passes on one seam must have distinct operation IDs.
    #[must_use]
    pub fn operation_id(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.seam)
    }
}

impl SewPlan {
    /// Active admitted order, including the complete batch when production scheduling was requested.
    #[must_use]
    pub fn ordering_report(&self) -> Option<&ordering::Report> {
        if self
            .production
            .as_ref()
            .is_some_and(|production| production.parallel.is_some())
        {
            return None;
        }
        self.production
            .as_ref()
            .map(|production| &production.ordering)
            .or(self.ordering.as_ref())
    }

    /// Actual feed scope: the complete explicitly scheduled batch, otherwise one garment.
    /// A thread-purchasing quantity alone never implies a production schedule.
    #[must_use]
    pub fn total_feed_seconds(&self) -> f64 {
        self.production
            .as_ref()
            .map_or(self.feed_seconds, |production| production.feed_seconds)
    }

    /// Complete serial feed/setup/handling model, available only with admitted handling evidence.
    /// This is conditional modeled time, not an elapsed factory throughput or hardware claim.
    #[must_use]
    pub fn modeled_production_seconds(&self) -> Option<f64> {
        self.ordering_report()?
            .handling_timing
            .as_ref()
            .map(|timing| timing.selected_modeled_production_seconds)
    }

    /// Selected whole-job elapsed time under the explicit concurrent-cell millisecond model.
    /// Aggregate feed/setup/handling work remains separately scoped in the parallel report.
    #[must_use]
    pub fn parallel_makespan_seconds(&self) -> Option<f64> {
        self.production
            .as_ref()?
            .parallel
            .as_ref()
            .map(|parallel| parallel.selected.makespan_milliseconds as f64 / 1000.0)
    }

    /// Proof status for the qualified parallel-cell assignment and its declared time resolution.
    #[must_use]
    pub fn parallel_optimality_proven(&self) -> Option<bool> {
        self.production
            .as_ref()?
            .parallel
            .as_ref()
            .map(|parallel| parallel.optimality_proven)
    }

    /// Actual traversal scope, matching [`Self::total_feed_seconds`].
    #[must_use]
    pub fn total_stitch_intervals(&self) -> usize {
        self.production
            .as_ref()
            .map_or(self.stitch_intervals, |production| {
                production.stitch_intervals
            })
    }

    /// Plan every seam using the shared material resolver and the pattern's actual seam geometry.
    /// Changes to thickness, per-piece material, grain/mirror geometry and notches are read afresh.
    /// `sim:false` and simulation scaling do not omit or deform manufacturing pieces or seams.
    ///
    /// # Errors
    /// Missing/invalid material or geometry; unmatched notches; incomplete, duplicate or cyclic
    /// passes; failed flat seam checks; machine/allowance limits; more than 4096 passes or 250000
    /// conservative station slots. Refusal publishes no partial plan.
    pub fn build(pattern: &Pattern, options: &SewOptions) -> Result<Self, Refusal> {
        // Reserve finite quote/alias work before generating any geometry or pricing a prefix.
        consumables::purchasing_work_bound(options)?;
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        if pattern.raw()["schema"] != "fabric-pattern/1" || pattern.raw()["units"] != "mm" {
            return Err(bad("sewing requires fabric-pattern/1 in millimetres"));
        }
        if pattern.raw()["fabric"].get("layers").is_some() {
            return Err(bad(
                "implicit fabric layers need explicit sewing sides; ply counts cannot invent a layer stack",
            ));
        }
        let resolved_options = production::resolve_options(options)?;
        let options = resolved_options.as_ref();
        let machine = &options.machine;
        if machine.id.trim().is_empty()
            || !range(machine.stitch_mm)
            || !range(machine.feed_b_over_a)
            || !positive(machine.max_stack_mm)
            || !positive(machine.max_stitches_per_minute)
            || !options.seam_tolerance_mm.is_finite()
            || options.seam_tolerance_mm < 0.0
        {
            return Err(bad("invalid sewing machine limits or seam tolerance"));
        }
        if let Some(reverse) = &machine.reverse {
            reverse.validate()?;
        }
        if let Some(foot) = &machine.presser_foot {
            foot.validate()?;
        }
        if options.qualifications.len() > MAX_OPERATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "at most 4096 sewing qualifications are supported",
            ));
        }
        let mut qualification_ids = BTreeSet::new();
        for q in &options.qualifications {
            q.validate()?;
            if !qualification_ids.insert(&q.id) {
                return Err(bad("sewing qualification IDs must be unique"));
            }
        }
        if options.require_qualification && options.qualifications.is_empty() {
            return Err(bad(
                "qualification is required but no sewing qualifications were supplied",
            ));
        }
        if options.passes.is_empty() || options.passes.len() > MAX_OPERATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing needs 1..4096 passes",
            ));
        }
        if let Some(ordering) = &options.ordering {
            ordering.validate(options.passes.len(), &machine.id)?;
        }
        let ctx = Ctx::new(
            pattern,
            ExportOptions {
                sa: options.seam_allowance_mm,
                ..ExportOptions::default()
            },
        )?;
        if !ctx.sa.is_finite() || ctx.sa < 0.0 {
            return Err(bad("invalid export seam allowance"));
        }
        let mut remaining = BTreeMap::new();
        let mut access_work = access::Budget::default();
        for pass in &options.passes {
            if pass.seam.trim().is_empty()
                || pass.operation_id().trim().is_empty()
                || remaining
                    .insert(pass.operation_id().to_string(), pass)
                    .is_some()
            {
                return Err(bad("sewing pass IDs must be nonempty and unique"));
            }
            if !within(pass.max_stitch_mm, [0.01, 1000.0])
                || pass.plies.is_some() == pass.assembly.is_some()
                || pass
                    .plies
                    .is_some_and(|p| p.iter().any(|&n| n == 0 || n > 64))
                || !pass.minimum_allowance_mm.is_finite()
                || pass.minimum_allowance_mm < 0.0
                || !positive(pass.stitches_per_minute)
                || pass.needle.trim().is_empty()
                || pass.thread.trim().is_empty()
            {
                return Err(bad(format!(
                    "{}: invalid or unsupported sewing recipe",
                    pass.seam
                )));
            }
            if let Some(range) = pass.source_range {
                span::validate(range)?;
            }
            program::validate_primary_feed(pass, machine)?;
            if let Some(path) = &pass.stitch_path {
                path.validate()?;
            }
            if let Some(foot) = &pass.presser_foot {
                foot.validate(machine.presser_foot.as_ref())?;
            }
            if let Some(access) = &pass.access {
                if pass.assembly.is_none() {
                    return Err(bad("planar access requires an explicit flat assembly"));
                }
                access.validate(machine, &mut access_work)?;
            }
        }
        let mut source_seams = BTreeMap::new();
        for seam in pattern.seams() {
            let id = seam
                .id()
                .ok_or_else(|| bad("every manufacturing seam needs a string ID"))?;
            if source_seams.insert(id, seam).is_some() {
                return Err(bad(format!("duplicate pattern seam {id}")));
            }
        }
        let covered: BTreeSet<_> = remaining.values().map(|p| p.seam.as_str()).collect();
        if covered.len() != source_seams.len()
            || covered.iter().any(|id| !source_seams.contains_key(id))
        {
            return Err(bad(
                "provide at least one sewing pass for every pattern seam, and no unknown seams",
            ));
        }
        for id in source_seams.keys() {
            span::require_coverage(
                id,
                "a",
                remaining
                    .values()
                    .filter(|p| p.seam == *id)
                    .map(|p| p.source_range.unwrap_or([0.0, 1.0]))
                    .collect(),
            )?;
        }
        for pass in remaining.values() {
            let deps: BTreeSet<_> = pass.after.iter().collect();
            if deps.len() != pass.after.len() || deps.iter().any(|id| !remaining.contains_key(*id))
            {
                return Err(bad(format!(
                    "{}: duplicate or unknown predecessor",
                    pass.seam
                )));
            }
        }
        let advanced = machine.reverse.is_some()
            || options
                .passes
                .iter()
                .any(|p| p.id.is_some() || !p.direction.is_forward() || !p.backtack.is_empty());
        let mut out = Self {
            schema: if options
                .production
                .as_ref()
                .is_some_and(|production| production.parallel.is_some())
            {
                "ftx-sewing-plan/16"
            } else if options
                .production
                .as_ref()
                .is_some_and(|production| production.handling.is_some())
            {
                "ftx-sewing-plan/15"
            } else if options.passes.iter().any(|pass| pass.access.is_some()) {
                "ftx-sewing-plan/14"
            } else if options
                .production
                .as_ref()
                .is_some_and(|production| production.workholding.is_some())
            {
                "ftx-sewing-plan/13"
            } else if options.production.is_some() {
                "ftx-sewing-plan/12"
            } else if options.consumables.is_some()
                || options
                    .qualifications
                    .iter()
                    .any(|q| q.thread_consumption.is_some())
            {
                "ftx-sewing-plan/11"
            } else if options.passes.iter().any(|p| p.source_range.is_some()) {
                "ftx-sewing-plan/10"
            } else if options
                .ordering
                .as_ref()
                .is_some_and(|o| o.setup_times.is_some())
            {
                "ftx-sewing-plan/9"
            } else if machine
                .presser_foot
                .as_ref()
                .is_some_and(|f| f.normal_force_n.is_some())
                || options.passes.iter().any(|p| {
                    p.presser_foot
                        .as_ref()
                        .is_some_and(|f| f.clearance.is_some())
                })
                || options.qualifications.iter().any(|q| {
                    q.presser_foot
                        .as_ref()
                        .is_some_and(|f| f.clearance.is_some())
                })
            {
                "ftx-sewing-plan/8"
            } else if machine.presser_foot.is_some()
                || options.passes.iter().any(|p| p.presser_foot.is_some())
                || options
                    .qualifications
                    .iter()
                    .any(|q| q.presser_foot.is_some())
            {
                "ftx-sewing-plan/7"
            } else if options.ordering.is_some() {
                "ftx-sewing-plan/6"
            } else if !options.qualifications.is_empty() || options.require_qualification {
                "ftx-sewing-plan/5"
            } else if options.passes.iter().any(|p| p.assembly.is_some()) {
                "ftx-sewing-plan/4"
            } else if options.passes.iter().any(|p| p.stitch_path.is_some()) {
                "ftx-sewing-plan/3"
            } else if advanced {
                "ftx-sewing-plan/2"
            } else {
                "ftx-sewing-plan/1"
            },
            pattern_sha256: sha256_hex(
                &serde_json::to_vec(pattern.raw()).map_err(|e| bad(e.to_string()))?,
            ),
            options: options.clone(),
            materials: BTreeMap::new(),
            seams: Vec::new(),
            ordering: None,
            production: None,
            consumables: None,
            stitch_intervals: 0,
            feed_seconds: 0.0,
            hardware_execution_authorized: false,
            physical_seam_quality_validated: false,
        };
        let mut done = BTreeSet::new();
        let mut budget = 0usize;
        let mut geometry_work = 0usize;
        let mut assembly_slots = 0usize;
        let mut pressure_work = pressure::Budget::default();
        while !remaining.is_empty() {
            let id = remaining
                .iter()
                .find(|(_, p)| p.after.iter().all(|d| done.contains(d)))
                .map(|(id, _)| id.clone())
                .ok_or_else(|| bad("cyclic sewing order"))?;
            let pass = remaining.remove(&id).expect("selected remaining pass");
            let seam = source_seams[pass.seam.as_str()];
            let ra = seam
                .side_ref("a")
                .ok_or_else(|| bad("missing seam side a"))?;
            let rb = seam
                .side_ref("b")
                .ok_or_else(|| bad("missing seam side b"))?;
            let sides = [
                out.side(pattern, ra, ctx.sa)?,
                out.side(pattern, rb, ctx.sa)?,
            ];
            let la = pattern.side_length(ra).map_err(|e| bad(e.to_string()))?;
            let lb = pattern.side_length(rb).map_err(|e| bad(e.to_string()))?;
            let mut breaks = 2usize;
            for r in [ra, rb] {
                let (_, edges) = pattern.side(r).map_err(|e| bad(e.to_string()))?;
                let notches = pattern.notch_fractions(r).map_err(|e| bad(e.to_string()))?;
                if notches.windows(2).any(|w| w[0] >= w[1]) {
                    return Err(bad(format!("{id}: duplicate notch positions")));
                }
                breaks += edges.len() + notches.len();
            }
            // Bound legacy seam_pairs before allocation. Partial/offset samplers independently
            // bound their actual span before allocation; unused source length is not sewn work.
            let slots = if pass.source_range.is_some() {
                breaks as f64
            } else {
                ((la + lb) / pass.max_stitch_mm).ceil() + breaks as f64
            };
            let extra = pass.backtack.extra_intervals()?;
            if !positive(la)
                || !positive(lb)
                || !slots.is_finite()
                || slots > MAX_STATIONS as f64
                || budget.saturating_add(slots as usize).saturating_add(extra) > MAX_STATIONS
            {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "sewing station budget exceeded",
                ));
            }
            budget += slots as usize + extra;
            let (stations, path) = if let Some(path) = &pass.stitch_path {
                path.build(
                    pattern,
                    seam,
                    pass.max_stitch_mm,
                    pass.source_range,
                    &ctx,
                    &mut geometry_work,
                )?
            } else if let Some(range) = pass.source_range {
                (span::build(pattern, seam, range, pass.max_stitch_mm)?, None)
            } else {
                (
                    pattern
                        .seam_pairs(seam, pass.max_stitch_mm)
                        .map_err(|e| bad(e.to_string()))?,
                    None,
                )
            };
            // Charge the larger actual table, including lock travel and offset/span sampling.
            let actual_slots = stations.fa.len();
            if actual_slots > slots as usize {
                budget = budget.saturating_add(actual_slots - slots as usize);
                if budget > MAX_STATIONS {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "sewing station budget exceeded after path sampling",
                    ));
                }
            }
            let (la, lb) = (stations.len_a, stations.len_b);
            if stations.notch_match.starts_with("count-mismatch") {
                return Err(bad(format!("{id}: notches must pair before sewing")));
            }
            let source_span = pass
                .source_range
                .map(|range| SourceSpan::new(range, &stations, path.as_ref()));
            let assembly = pass
                .assembly
                .as_ref()
                .map(|a| {
                    a.compile(
                        pattern,
                        &ctx,
                        [ra, rb],
                        &stations,
                        &mut out.materials,
                        &mut geometry_work,
                    )
                })
                .transpose()?;
            let stack = if let Some(a) = &assembly {
                assembly_slots =
                    assembly_slots.saturating_add(a.layers.len().saturating_mul(a.needle_mm.len()));
                if assembly_slots > MAX_STATIONS {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "sewing job material preimage budget exceeded",
                    ));
                }
                a.stack_mm
            } else {
                let plies = pass.plies.expect("validated exclusive ply policy");
                sides[0].thickness_mm * f64::from(plies[0])
                    + sides[1].thickness_mm * f64::from(plies[1])
            };
            if !positive(stack)
                || stack > machine.max_stack_mm
                || sides
                    .iter()
                    .any(|s| s.allowance_mm < pass.minimum_allowance_mm)
            {
                return Err(bad(format!(
                    "{id}: stack thickness or seam allowance exceeds recipe/machine limits"
                )));
            }
            let mut feeds = Vec::with_capacity(stations.fa.len() - 1);
            let mut advances = Vec::with_capacity(stations.fa.len() - 1);
            for (a, b) in stations.fa.windows(2).zip(stations.fb.windows(2)) {
                let da = (a[1] - a[0]) * la;
                let db = (b[1] - b[0]) * lb;
                if !positive(da) || !positive(db) {
                    return Err(bad(format!(
                        "{id}: local stitch advance or differential feed outside machine limits"
                    )));
                }
                feeds.push(db / da);
                advances.push([da, db]);
            }
            let runs = program::compile(&stations, [la, lb], pass, machine)?;
            let has_reverse = runs.iter().any(|run| !run.direction.is_forward());
            let stack_materials: Vec<String> = if let Some(receipt) = &assembly {
                receipt
                    .layers
                    .iter()
                    .map(|layer| layer.material_sha256.clone())
                    .collect()
            } else {
                let plies = pass.plies.expect("validated exclusive ply policy");
                std::iter::repeat_n(
                    sides[0].material_sha256.clone(),
                    usize::try_from(plies[0]).expect("validated ply count"),
                )
                .chain(std::iter::repeat_n(
                    sides[1].material_sha256.clone(),
                    usize::try_from(plies[1]).expect("validated ply count"),
                ))
                .collect()
            };
            let presser_foot = pass
                .presser_foot
                .as_ref()
                .map(|request| {
                    request.evaluate(
                        machine.presser_foot.as_ref(),
                        &stack_materials,
                        &out.materials,
                        &mut pressure_work,
                    )
                })
                .transpose()?;
            let identity: Vec<_> = options
                .qualifications
                .iter()
                .filter(|q| q.identity_matches(&stack_materials, machine, pass))
                .collect();
            let qualified: Vec<_> = identity
                .iter()
                .copied()
                .filter(|q| {
                    q.envelope_matches(
                        pass,
                        stack,
                        &advances,
                        &feeds,
                        has_reverse,
                        presser_foot.as_ref(),
                    )
                })
                .collect();
            let qualification_id = match qualified.as_slice() {
                [q] => Some(q.id.clone()),
                [] if options.require_qualification => {
                    let reason = if identity.is_empty() {
                        "no qualification matches the exact stack, machine, needle and thread"
                    } else {
                        "operation falls outside its qualified stitch/feed/stack/rate/reverse envelope"
                    };
                    return Err(bad(format!("{id}: {reason}")));
                }
                [] => None,
                _ => {
                    return Err(bad(format!(
                        "{id}: multiple sewing qualifications admit the same operation"
                    )));
                }
            };
            let access = pass
                .access
                .as_ref()
                .map(|policy| {
                    let qualification = qualified.first().copied().ok_or_else(|| {
                        bad(format!(
                            "{id}: planar access needs an admitted source process qualification"
                        ))
                    })?;
                    policy.evaluate(
                        assembly.as_ref().expect("validated access assembly"),
                        qualification,
                        &mut access_work,
                    )
                })
                .transpose()?;
            let stitch_intervals = runs.iter().map(|r| r.stitch_intervals).sum();
            let feed_seconds = stitch_intervals as f64 * 60.0 / pass.stitches_per_minute;
            out.feed_seconds += feed_seconds;
            if !out.feed_seconds.is_finite() {
                return Err(bad("non-finite sewing feed time"));
            }
            out.stitch_intervals += stitch_intervals;
            out.seams.push(PlannedSeam {
                pass: pass.clone(),
                sides,
                stations,
                source_span,
                path,
                assembly,
                feed_b_over_a: feeds,
                stack_mm: stack,
                presser_foot,
                access,
                runs: if out.schema != "ftx-sewing-plan/1" {
                    runs
                } else {
                    Vec::new()
                },
                stitch_intervals,
                feed_seconds,
                qualification_id,
            });
            done.insert(id);
        }
        // Different offset recipes can pair the same A split with different B source positions.
        // Coverage must therefore hold in both original source frames, with no gap tolerance.
        for id in source_seams.keys() {
            span::require_coverage(
                id,
                "b",
                out.seams
                    .iter()
                    .filter(|p| p.pass.seam == *id)
                    .map(|p| {
                        p.source_span
                            .as_ref()
                            .map_or([0.0, 1.0], |s| s.source_ranges[1])
                    })
                    .collect(),
            )?;
        }
        for row in ftx_pattern::validate::seam_walk(pattern, options.seam_tolerance_mm) {
            if row.status != "PASS" {
                return Err(bad(format!(
                    "sewing requires passing flat seams: {}: {}",
                    row.seam,
                    row.message.as_deref().unwrap_or(&row.status)
                )));
            }
        }
        // All original geometric, material and recipe gates apply before a schedule is considered.
        // Reordering moves complete admitted operations; no physical assembly state is inferred.
        if let Some(policy) = &options.ordering {
            let report = ordering::optimize(policy, &out.seams)?;
            let ranks: BTreeMap<_, _> = report
                .selected_order
                .iter()
                .enumerate()
                .map(|(rank, id)| (id.as_str(), rank))
                .collect();
            out.seams
                .sort_by_key(|seam| ranks[seam.pass.operation_id()]);
            out.ordering = Some(report);
        }
        if let Some(request) = &options.consumables {
            out.consumables = Some(consumables::evaluate(&out, request)?);
        }
        if let Some(policy) = &options.production {
            out.production = Some(production::evaluate(&out, policy)?);
        }
        Ok(out)
    }

    fn side(
        &mut self,
        pattern: &Pattern,
        reference: &str,
        default_sa: f64,
    ) -> Result<SewSide, Refusal> {
        let (instance, edges) = pattern.side(reference).map_err(|e| bad(e.to_string()))?;
        if edges.len() > MAX_OPERATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "too many seam-side edges",
            ));
        }
        let (selected, thickness_mm, uses_thickness_upper_bound) =
            sheet(pattern, instance.piece_index)?;
        let piece = &pattern.doc.as_ref().expect("validated pattern").pieces[instance.piece_index];
        let mut allowance_mm = f64::INFINITY;
        for id in edges {
            let source = instance.edge(&id).map_err(|e| bad(e.to_string()))?.source;
            let sa = edge_sa(piece, source, default_sa);
            if !sa.is_finite() || sa < 0.0 {
                return Err(bad(format!("{reference}: invalid seam allowance")));
            }
            allowance_mm = allowance_mm.min(sa);
        }
        let material_sha256 = selected.sha256().to_string();
        self.materials
            .entry(material_sha256.clone())
            .or_insert_with(|| selected.frozen_selection());
        Ok(SewSide {
            reference: reference.into(),
            material_sha256,
            thickness_mm,
            uses_thickness_upper_bound,
            allowance_mm,
        })
    }
}

// One thickness authority for legacy seam sides AND every additional folded/lining ply.
fn sheet(pattern: &Pattern, piece_index: usize) -> Result<(ResolvedMaterial, f64, bool), Refusal> {
    let selected = resolve_fabric(piece_fabric(pattern, piece_index)?)?
        .ok_or_else(|| bad("sewing needs a source-bearing material selection"))?;
    let material = selected.material();
    if material.structure.contains_key("compression_model") {
        return Err(bad(
            "sewing requires qualified uncompressed sheet thickness; a pressure-conditioned selection cannot establish unloaded stack clearance or presser-foot feasibility",
        ));
    }
    if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
        return Err(bad(
            "sewing requires an identified sheet/hide, not loose fill or yarn",
        ));
    }
    let thickness = material.require("thickness", "m", selected.allows_estimates())?;
    let bounds = material.properties["thickness"].bounds;
    let thickness_mm = bounds.map_or(thickness, |v| v[1]) * 1000.0;
    if !positive(thickness) || !positive(thickness_mm) || bounds.is_some_and(|v| !positive(v[0])) {
        return Err(bad("thickness must be finite and positive"));
    }
    Ok((selected, thickness_mm, bounds.is_some()))
}

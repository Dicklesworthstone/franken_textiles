//! Material-following seam plans in the two original pattern-side frames (millimetres).
//! These are feed/stack/allowance checks, not robot trajectories or seam-quality certification.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Form;
use ftx_pattern::textile_import::{piece_fabric, resolve_fabric};
use ftx_pattern::{Pattern, SeamPairs};
use serde::{Deserialize, Serialize};

use crate::{Ctx, ExportOptions, edge_sa};

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
}

/// One declared pass over an existing seam. All pattern seams must have exactly one pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewingPass {
    /// Exact pattern seam ID.
    pub seam: String,
    /// Upper bound on each side's stitch advance, not a guarantee of uniform pitch.
    pub max_stitch_mm: f64,
    /// Number of plies of each selected side material at the needle, including declared folds.
    pub plies: [u32; 2],
    /// Required allowance on both sides, in mm; no fold or seam recipe is guessed.
    pub minimum_allowance_mm: f64,
    /// Requested continuous stitch rate.
    pub stitches_per_minute: f64,
    /// Operator-selected needle identifier; suitability remains unvalidated.
    pub needle: String,
    /// Operator-selected thread identifier; consumption/strength are not inferred.
    pub thread: String,
    /// Seam IDs which must finish first. Omission adds no physical-accessibility claim.
    #[serde(default)]
    pub after: Vec<String>,
}

/// Complete sewing job policy, independent of simulation parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SewOptions {
    /// Declared machine configuration.
    pub machine: SewingMachine,
    /// One explicit pass for every seam, including seams omitted from a simulation.
    pub passes: Vec<SewingPass>,
    /// Default flat seam-walk tolerance in mm; existing per-seam tolerances take precedence.
    pub seam_tolerance_mm: f64,
    /// Default export allowance override; edge and piece allowances still take precedence.
    #[serde(default)]
    pub seam_allowance_mm: Option<f64>,
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
    pub stations: SeamPairs,
    /// Side-b / side-a feed ratio for each interval between consecutive stations.
    pub feed_b_over_a: Vec<f64>,
    /// Declared ply counts times sourced, uncompressed thicknesses (mm).
    pub stack_mm: f64,
    /// One forward stitch interval per pair of consecutive stations; no locking/backtack stitches.
    pub stitch_intervals: usize,
    /// Continuous feed time at the requested rate (seconds), excluding handling/acceleration.
    pub feed_seconds: f64,
}

/// Deterministic, source-locked manufacturing review plan. Never hardware execution permission.
#[derive(Clone, Debug, Serialize)]
pub struct SewPlan {
    /// `ftx-sewing-plan/1`.
    pub schema: &'static str,
    /// Digest of the complete original pattern JSON, including material assignments.
    pub pattern_sha256: String,
    /// Policy and declared machine limits used for this plan.
    pub options: SewOptions,
    /// Portable selections, indexed by resolved digest; no dependence on a future live catalog.
    pub materials: BTreeMap<String, Selection>,
    /// Stable topological order, seam-ID order to break ties; not an inferred assembly strategy.
    pub seams: Vec<PlannedSeam>,
    /// Total forward stitch intervals, not thread consumption.
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

fn range(x: [f64; 2]) -> bool {
    positive(x[0]) && positive(x[1]) && x[0] <= x[1]
}

fn within(x: f64, limits: [f64; 2]) -> bool {
    // Arithmetic tolerance only: not a concealed machine/process allowance.
    let eps = |limit: f64| 1e-10 * x.abs().max(limit.abs());
    positive(x) && x >= limits[0] - eps(limits[0]) && x <= limits[1] + eps(limits[1])
}

impl SewPlan {
    /// Plan every seam using the shared material resolver and the pattern's actual seam geometry.
    /// Changes to thickness, per-piece material, grain/mirror geometry and notches are read afresh.
    /// `sim:false` and simulation scaling do not omit or deform manufacturing pieces or seams.
    ///
    /// # Errors
    /// Missing/invalid material or geometry; unmatched notches; incomplete, duplicate or cyclic
    /// passes; failed flat seam checks; machine/allowance limits; more than 4096 passes or 250000
    /// conservative station slots. Refusal publishes no partial plan.
    pub fn build(pattern: &Pattern, options: &SewOptions) -> Result<Self, Refusal> {
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(bad(errors.join("; ")));
        }
        if pattern.raw()["schema"] != "fabric-pattern/1" || pattern.raw()["units"] != "mm" {
            return Err(bad("sewing requires fabric-pattern/1 in millimetres"));
        }
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
        if options.passes.is_empty() || options.passes.len() > MAX_OPERATIONS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing needs 1..4096 passes",
            ));
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
        for pass in &options.passes {
            if pass.seam.trim().is_empty() || remaining.insert(pass.seam.clone(), pass).is_some() {
                return Err(bad("sewing pass IDs must be nonempty and unique"));
            }
            if !within(pass.max_stitch_mm, [0.01, 1000.0])
                || !within(pass.max_stitch_mm, machine.stitch_mm)
                || pass.plies.iter().any(|&n| n == 0 || n > 64)
                || !pass.minimum_allowance_mm.is_finite()
                || pass.minimum_allowance_mm < 0.0
                || !positive(pass.stitches_per_minute)
                || pass.stitches_per_minute > machine.max_stitches_per_minute
                || pass.needle.trim().is_empty()
                || pass.thread.trim().is_empty()
            {
                return Err(bad(format!(
                    "{}: invalid or unsupported sewing recipe",
                    pass.seam
                )));
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
        if source_seams.len() != remaining.len()
            || remaining
                .keys()
                .any(|id| !source_seams.contains_key(id.as_str()))
        {
            return Err(bad(
                "provide exactly one sewing pass for every pattern seam",
            ));
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
        let mut out = Self {
            schema: "ftx-sewing-plan/1",
            pattern_sha256: sha256_hex(
                &serde_json::to_vec(pattern.raw()).map_err(|e| bad(e.to_string()))?,
            ),
            options: options.clone(),
            materials: BTreeMap::new(),
            seams: Vec::new(),
            stitch_intervals: 0,
            feed_seconds: 0.0,
            hardware_execution_authorized: false,
            physical_seam_quality_validated: false,
        };
        let mut done = BTreeSet::new();
        let mut budget = 0usize;
        while !remaining.is_empty() {
            let id = remaining
                .iter()
                .find(|(_, p)| p.after.iter().all(|d| done.contains(d)))
                .map(|(id, _)| id.clone())
                .ok_or_else(|| bad("cyclic sewing order"))?;
            let pass = remaining.remove(&id).expect("selected remaining pass");
            let seam = source_seams[id.as_str()];
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
            // Bound allocation BEFORE seam_pairs computes ceil(span / pitch) samples per segment.
            let slots = ((la + lb) / pass.max_stitch_mm).ceil() + breaks as f64;
            if !positive(la)
                || !positive(lb)
                || !slots.is_finite()
                || slots > MAX_STATIONS as f64
                || budget.saturating_add(slots as usize) > MAX_STATIONS
            {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "sewing station budget exceeded",
                ));
            }
            budget += slots as usize;
            let stations = pattern
                .seam_pairs(seam, pass.max_stitch_mm)
                .map_err(|e| bad(e.to_string()))?;
            if stations.notch_match.starts_with("count-mismatch") {
                return Err(bad(format!("{id}: notches must pair before sewing")));
            }
            let stack = sides[0].thickness_mm * f64::from(pass.plies[0])
                + sides[1].thickness_mm * f64::from(pass.plies[1]);
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
            for (a, b) in stations.fa.windows(2).zip(stations.fb.windows(2)) {
                let da = (a[1] - a[0]) * la;
                let db = (b[1] - b[0]) * lb;
                if !positive(da)
                    || !positive(db)
                    || !within(da, machine.stitch_mm)
                    || !within(db, machine.stitch_mm)
                    || !within(db / da, machine.feed_b_over_a)
                {
                    return Err(bad(format!(
                        "{id}: local stitch advance or differential feed outside machine limits"
                    )));
                }
                feeds.push(db / da);
            }
            let stitch_intervals = feeds.len();
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
                feed_b_over_a: feeds,
                stack_mm: stack,
                stitch_intervals,
                feed_seconds,
            });
            done.insert(id);
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
        let selected =
            resolve_fabric(piece_fabric(pattern, instance.piece_index)?)?.ok_or_else(|| {
                bad(format!(
                    "{reference}: sewing needs a source-bearing material selection"
                ))
            })?;
        let material = selected.material();
        if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(bad(format!(
                "{reference}: sewing requires an identified sheet/hide, not loose fill or yarn"
            )));
        }
        let thickness = material.require("thickness", "m", selected.allows_estimates())?;
        let bounds = material.properties["thickness"].bounds;
        let thickness_mm = bounds.map_or(thickness, |v| v[1]) * 1000.0;
        if !positive(thickness)
            || !positive(thickness_mm)
            || bounds.is_some_and(|v| !positive(v[0]))
        {
            return Err(bad(format!(
                "{reference}: thickness must be finite and positive"
            )));
        }
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
            uses_thickness_upper_bound: bounds.is_some(),
            allowance_mm,
        })
    }
}

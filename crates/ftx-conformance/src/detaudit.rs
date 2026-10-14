//! Staged determinism-audit harness and first-divergence locator across runs, worker counts and
//! targets (native vs wasm, KAT 9 debugging).
//!
//! Provenance: lifted and adapted from `frankensim` `crates/fs-detaudit/src/lib.rs` at commit
//! `bb4b89ead3db0c3e5d197dd7c74d3a8d9d6f2e2c`, with `fnv1a64` replaced by `ftx_core::receipt::sha256_hex`
//! (`FTX-25` / `ftx-kkq` item 5).
//!
//! No-claims: the harness audits the staged traces subjects hand it. It does not prove the absence
//! of nondeterminism beyond the exercised matrix and repeats, makes no throughput claim (mode deltas
//! are measured per call, never assumed), and the cross-ISA report is evidence of classified
//! divergence, not a cross-ISA equality certificate.

use core::fmt::{self, Write as _};
use std::collections::BTreeMap;
use std::time::Instant;

use ftx_core::receipt::sha256_hex;

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{0008}' => escaped.push_str("\\b"),
            '\u{000c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{0000}'..='\u{001f}' => {
                let _ = write!(&mut escaped, "\\u{:04x}", u32::from(ch));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Lowercase hex SHA-256 over raw stage bytes.
#[must_use]
pub fn sha256_stage(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

/// Lowercase hex SHA-256 over a slice of `f64` values in canonical little-endian IEEE-754 bit order.
#[must_use]
pub fn sha256_f64s(values: &[f64]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for &v in values {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    sha256_hex(&bytes)
}

/// One stage of a subject run: a stable stage label and the SHA-256 hex digest of its exact result bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageHash {
    /// Stable stage label (names the placement, Newton step, linear solve or report stage).
    pub label: String,
    /// Lowercase hex SHA-256 content hash over exact result bits at this stage.
    pub sha256: String,
}

impl StageHash {
    /// Hashes `bytes` with SHA-256 under `label`.
    #[must_use]
    pub fn of(label: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            label: label.into(),
            sha256: sha256_hex(bytes),
        }
    }

    /// Hashes `values` (`f64` bits in little-endian order) with SHA-256 under `label`.
    #[must_use]
    pub fn of_f64s(label: impl Into<String>, values: &[f64]) -> Self {
        Self {
            label: label.into(),
            sha256: sha256_f64s(values),
        }
    }
}

/// Ordered staged SHA-256 trace of one subject run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedTrace {
    /// Stages in execution order.
    pub stages: Vec<StageHash>,
}

impl StagedTrace {
    /// The final artifact SHA-256 (last stage), if any stage ran.
    #[must_use]
    pub fn final_sha256(&self) -> Option<&str> {
        self.stages.last().map(|s| s.sha256.as_str())
    }
}

/// A determinism-audit subject: runs the workload at a worker count and reports its staged trace.
pub struct Subject {
    /// Stable subject name.
    pub name: &'static str,
    /// Execute at the given worker count.
    pub run: Box<dyn Fn(usize) -> StagedTrace>,
}

/// The worker-count matrix an audit exercises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerMatrix {
    /// Distinct worker counts, ascending.
    pub counts: Vec<usize>,
}

impl WorkerMatrix {
    /// Canonical host matrix: `1, 2, P, P+2, 2P` (deduplicated, ascending), with `P` the available
    /// parallelism.
    #[must_use]
    pub fn host_default() -> Self {
        let p = std::thread::available_parallelism().map_or(4, usize::from);
        let mut counts = vec![1, 2, p, p + 2, 2 * p];
        counts.sort_unstable();
        counts.dedup();
        Self { counts }
    }

    /// An explicit matrix.
    ///
    /// # Panics
    /// If `counts` is empty or contains zero.
    #[must_use]
    pub fn explicit(mut counts: Vec<usize>) -> Self {
        assert!(
            !counts.is_empty() && counts.iter().all(|&c| c > 0),
            "worker matrix must be non-empty and positive"
        );
        counts.sort_unstable();
        counts.dedup();
        Self { counts }
    }
}

/// Audit configuration: the matrix and the repeats per worker count.
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Worker counts to exercise.
    pub matrix: WorkerMatrix,
    /// Runs per worker count (`>= 1`).
    pub repeats: usize,
}

impl AuditConfig {
    /// A config over the host-default matrix.
    #[must_use]
    pub fn host_default(repeats: usize) -> Self {
        Self {
            matrix: WorkerMatrix::host_default(),
            repeats: repeats.max(1),
        }
    }
}

/// Where a diverging run first left the baseline trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivergenceLocator {
    /// Worker count of the diverging run.
    pub workers: usize,
    /// Repeat index of the diverging run.
    pub repeat: usize,
    /// Index of the FIRST differing stage (or first missing stage on a length mismatch).
    pub first_stage: usize,
    /// The baseline's label at that stage (empty when the baseline is shorter than the diverging trace).
    pub stage_label: String,
    /// Baseline SHA-256 at that stage (empty when absent).
    pub baseline_sha256: String,
    /// Observed SHA-256 at that stage (empty when absent).
    pub observed_sha256: String,
}

/// One executed audit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// Worker count.
    pub workers: usize,
    /// Repeat index.
    pub repeat: usize,
    /// Final artifact SHA-256 of the run.
    pub final_sha256: String,
    /// Whether the full staged trace matched the baseline bit-for-bit.
    pub identical: bool,
}

/// The audit verdict and evidence.
#[derive(Debug, Clone)]
pub struct AuditReport {
    /// Subject name.
    pub subject: &'static str,
    /// Baseline (first matrix entry, repeat 0) staged trace.
    pub baseline: StagedTrace,
    /// Every executed row (`matrix × repeats`), in execution order.
    pub rows: Vec<AuditRow>,
    /// Every divergence, localized to its first differing stage.
    pub divergences: Vec<DivergenceLocator>,
}

impl AuditReport {
    /// Whether every run reproduced the baseline bit-for-bit.
    #[must_use]
    pub fn identical(&self) -> bool {
        self.divergences.is_empty() && self.rows.iter().all(|r| r.identical)
    }

    /// One JSON line per row plus one per divergence.
    #[must_use]
    pub fn json_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.rows.len() + self.divergences.len());
        let subject = escape_json_string(self.subject);
        for row in &self.rows {
            out.push(format!(
                "{{\"detaudit\":\"row\",\"subject\":\"{}\",\"workers\":{},\"repeat\":{},\"final_sha256\":\"{}\",\"identical\":{}}}",
                subject,
                row.workers,
                row.repeat,
                escape_json_string(&row.final_sha256),
                row.identical
            ));
        }
        for d in &self.divergences {
            out.push(format!(
                "{{\"detaudit\":\"divergence\",\"subject\":\"{}\",\"workers\":{},\"repeat\":{},\"first_stage\":{},\"stage_label\":\"{}\",\"baseline\":\"{}\",\"observed\":\"{}\"}}",
                subject,
                d.workers,
                d.repeat,
                d.first_stage,
                escape_json_string(&d.stage_label),
                escape_json_string(&d.baseline_sha256),
                escape_json_string(&d.observed_sha256)
            ));
        }
        out
    }
}

/// Localizes the first differing stage between two staged traces (e.g. native vs wasm on KAT 9).
#[must_use]
pub fn locate_divergence(
    baseline: &StagedTrace,
    observed: &StagedTrace,
    workers: usize,
    repeat: usize,
) -> Option<DivergenceLocator> {
    let n = baseline.stages.len().max(observed.stages.len());
    for i in 0..n {
        let b = baseline.stages.get(i);
        let o = observed.stages.get(i);
        let same = match (b, o) {
            (Some(b), Some(o)) => b == o,
            _ => false,
        };
        if !same {
            return Some(DivergenceLocator {
                workers,
                repeat,
                first_stage: i,
                stage_label: b.map_or_else(String::new, |s| s.label.clone()),
                baseline_sha256: b.map_or_else(String::new, |s| s.sha256.clone()),
                observed_sha256: o.map_or_else(String::new, |s| s.sha256.clone()),
            });
        }
    }
    None
}

/// Runs the determinism audit: every `(worker count, repeat)` run must reproduce the baseline
/// staged trace bit-for-bit; divergences are localized to their first differing stage and reported.
#[must_use]
pub fn audit(subject: &Subject, config: &AuditConfig) -> AuditReport {
    let baseline_workers = config.matrix.counts[0];
    let baseline = (subject.run)(baseline_workers);
    let mut rows = Vec::new();
    let mut divergences = Vec::new();
    for &workers in &config.matrix.counts {
        for repeat in 0..config.repeats {
            let trace = if workers == baseline_workers && repeat == 0 {
                baseline.clone()
            } else {
                (subject.run)(workers)
            };
            let identical = trace == baseline;
            rows.push(AuditRow {
                workers,
                repeat,
                final_sha256: trace.final_sha256().unwrap_or("").to_owned(),
                identical,
            });
            if !identical && let Some(loc) = locate_divergence(&baseline, &trace, workers, repeat) {
                divergences.push(loc);
            }
        }
    }
    AuditReport {
        subject: subject.name,
        baseline,
        rows,
        divergences,
    }
}

/// Documented cross-ISA divergence categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DivergenceClass {
    /// FMA contraction differences (declared and accepted).
    FmaContraction,
    /// Elementary function ULP differences bounded by a declared envelope; verified against value
    /// bits when both ledgers supply them.
    LibmUlp {
        /// Maximum admitted ULP distance.
        max_ulps: u32,
    },
}

impl fmt::Display for DivergenceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FmaContraction => write!(f, "fma-contraction"),
            Self::LibmUlp { max_ulps } => write!(f, "libm-ulp<={max_ulps}"),
        }
    }
}

/// One artifact row in a per-ISA ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    /// SHA-256 hex digest over the artifact's exact bits.
    pub sha256: String,
    /// Scalar value bits when the artifact is a single `f64` (enables ULP verification).
    pub value_bits: Option<u64>,
}

/// A per-ISA artifact ledger: artifact name -> row.
#[derive(Debug, Clone)]
pub struct IsaLedger {
    /// ISA label (e.g. `"aarch64"`, `"x86-64"`, `"wasm32"`).
    pub isa: String,
    /// Artifact rows.
    pub rows: BTreeMap<String, LedgerRow>,
}

/// Declared admissible divergence per artifact. Artifacts absent from the policy must match
/// bit-for-bit; reduction-shape divergence has no admissible declaration in deterministic mode.
#[derive(Debug, Clone, Default)]
pub struct DivergencePolicy {
    /// Artifact name -> declared class.
    pub declared: BTreeMap<String, DivergenceClass>,
}

/// How one artifact classified across two ISAs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Classification {
    /// Bit-identical across ISAs.
    Identical,
    /// Divergent inside its declared category.
    Classified {
        /// Declared class.
        class: DivergenceClass,
        /// Verification detail.
        detail: String,
    },
    /// Divergent with no admissible declaration, a violated envelope, or a ledger-shape mismatch.
    Unclassified {
        /// Reason.
        reason: String,
    },
}

/// One cross-ISA report row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossIsaRow {
    /// Artifact name.
    pub artifact: String,
    /// SHA-256 on the first ledger's ISA (empty when absent).
    pub sha256_a: String,
    /// SHA-256 on the second ledger's ISA (empty when absent).
    pub sha256_b: String,
    /// Classification.
    pub classification: Classification,
}

/// Cross-ISA divergence report.
#[derive(Debug, Clone)]
pub struct CrossIsaReport {
    /// First ISA label.
    pub isa_a: String,
    /// Second ISA label.
    pub isa_b: String,
    /// One row per artifact across both ledgers.
    pub rows: Vec<CrossIsaRow>,
}

fn ulp_distance(a_bits: u64, b_bits: u64) -> Option<u64> {
    let a = f64::from_bits(a_bits);
    let b = f64::from_bits(b_bits);
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    if a.is_sign_negative() != b.is_sign_negative() && a != 0.0 && b != 0.0 {
        return None;
    }
    let key = |bits: u64| -> i128 {
        let v = f64::from_bits(bits);
        if v.is_sign_negative() {
            -i128::from(bits & 0x7fff_ffff_ffff_ffff)
        } else {
            i128::from(bits)
        }
    };
    Some(
        key(a_bits)
            .abs_diff(key(b_bits))
            .try_into()
            .unwrap_or(u64::MAX),
    )
}

/// Classifies every artifact difference between two per-ISA ledgers under the declared policy.
#[must_use]
pub fn classify_cross_isa(
    a: &IsaLedger,
    b: &IsaLedger,
    policy: &DivergencePolicy,
) -> CrossIsaReport {
    let mut names: Vec<&String> = a.rows.keys().chain(b.rows.keys()).collect();
    names.sort();
    names.dedup();
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        let ra = a.rows.get(name);
        let rb = b.rows.get(name);
        let classification = match (ra, rb) {
            (None, _) | (_, None) => Classification::Unclassified {
                reason: "artifact missing from one ISA ledger".to_owned(),
            },
            (Some(ra), Some(rb)) if ra.sha256 == rb.sha256 => Classification::Identical,
            (Some(ra), Some(rb)) => match policy.declared.get(name) {
                None => Classification::Unclassified {
                    reason: "divergence with no declared category (reduction-shape \
                             divergence is forbidden in deterministic mode)"
                        .to_owned(),
                },
                Some(class @ DivergenceClass::FmaContraction) => Classification::Classified {
                    class: *class,
                    detail: "declared FMA-contraction divergence accepted".to_owned(),
                },
                Some(class @ DivergenceClass::LibmUlp { max_ulps }) => {
                    match (ra.value_bits, rb.value_bits) {
                        (Some(va), Some(vb)) => match ulp_distance(va, vb) {
                            Some(d) if d <= u64::from(*max_ulps) => Classification::Classified {
                                class: *class,
                                detail: format!("measured {d} ulps within envelope"),
                            },
                            Some(d) => Classification::Unclassified {
                                reason: format!("libm envelope violated: {d} ulps > {max_ulps}"),
                            },
                            None => Classification::Unclassified {
                                reason: "libm class needs comparable finite same-sign values"
                                    .to_owned(),
                            },
                        },
                        _ => Classification::Unclassified {
                            reason: "libm class declared but value bits missing from a ledger"
                                .to_owned(),
                        },
                    }
                }
            },
        };
        rows.push(CrossIsaRow {
            artifact: name.clone(),
            sha256_a: ra.map_or_else(String::new, |r| r.sha256.clone()),
            sha256_b: rb.map_or_else(String::new, |r| r.sha256.clone()),
            classification,
        });
    }
    CrossIsaReport {
        isa_a: a.isa.clone(),
        isa_b: b.isa.clone(),
        rows,
    }
}

impl CrossIsaReport {
    /// Whether every row is identical or classified (no unclassified rows).
    #[must_use]
    pub fn clean(&self) -> bool {
        self.rows
            .iter()
            .all(|r| !matches!(r.classification, Classification::Unclassified { .. }))
    }

    /// Renders a markdown table of the cross-ISA comparison.
    #[must_use]
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Cross-ISA divergence report: {} vs {}\n\n\
             | artifact | {} | {} | classification |\n|---|---|---|---|\n",
            self.isa_a, self.isa_b, self.isa_a, self.isa_b
        ));
        for row in &self.rows {
            let class = match &row.classification {
                Classification::Identical => "identical".to_owned(),
                Classification::Classified { class, detail } => format!("{class}: {detail}"),
                Classification::Unclassified { reason } => format!("UNCLASSIFIED: {reason}"),
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                row.artifact, row.sha256_a, row.sha256_b, class
            ));
        }
        out.push_str(&format!(
            "\nverdict: {}\n",
            if self.clean() {
                "clean (every divergence classified)"
            } else {
                "NOT CLEAN (unclassified rows are build failures)"
            }
        ));
        out
    }
}

/// Measured fast-versus-deterministic execution tradeoff for one workload.
#[derive(Debug, Clone)]
pub struct ModeDeltaReport {
    /// Workload name.
    pub name: &'static str,
    /// Total deterministic-mode wall time over the repeats.
    pub deterministic_ns: u128,
    /// Total fast-mode wall time over the repeats.
    pub fast_ns: u128,
    /// `deterministic_ns / fast_ns` (`> 1` means fast mode is faster).
    pub gain_ratio: f64,
    /// Whether deterministic-mode SHA-256 digests were identical across repeats.
    pub deterministic_reproducible: bool,
    /// Whether fast-mode SHA-256 digests were identical across repeats.
    pub fast_reproducible: bool,
}

/// Measures the fast-versus-deterministic tradeoff over `repeats` runs.
#[must_use]
pub fn measure_mode_delta(
    name: &'static str,
    repeats: usize,
    deterministic: impl Fn() -> String,
    fast: impl Fn() -> String,
) -> ModeDeltaReport {
    let repeats = repeats.max(2);
    let mut det_hashes = Vec::with_capacity(repeats);
    let det_start = Instant::now();
    for _ in 0..repeats {
        det_hashes.push(deterministic());
    }
    let deterministic_ns = det_start.elapsed().as_nanos();
    let mut fast_hashes = Vec::with_capacity(repeats);
    let fast_start = Instant::now();
    for _ in 0..repeats {
        fast_hashes.push(fast());
    }
    let fast_ns = fast_start.elapsed().as_nanos();
    let gain_ratio = if fast_ns == 0 {
        f64::INFINITY
    } else {
        deterministic_ns as f64 / fast_ns as f64
    };
    ModeDeltaReport {
        name,
        deterministic_ns,
        fast_ns,
        gain_ratio,
        deterministic_reproducible: det_hashes.windows(2).all(|w| w[0] == w[1]),
        fast_reproducible: fast_hashes.windows(2).all(|w| w[0] == w[1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::{Vec2, Vec3};
    use ftx_drape::{ClothModel, Membrane, Solver};

    #[test]
    fn staged_drape_solver_audits_bit_identical_and_localizes_seeded_stage_divergence() {
        let subject = Subject {
            name: "kat9-staged-cantilever",
            run: Box::new(|_workers| {
                let rest = vec![
                    Vec2::new(0.0, 0.0),
                    Vec2::new(0.1, 0.0),
                    Vec2::new(0.0, 0.1),
                    Vec2::new(0.1, 0.1),
                ];
                let tris = [[0, 1, 2], [1, 3, 2]];
                let start: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
                let pinned = [[true; 3], [true; 3], [false; 3], [false; 3]];
                let model = ClothModel::new(
                    rest,
                    &tris,
                    Membrane::orthotropic(120.0, 120.0, 12.0, 45.0, 0.05),
                    1e-4,
                    0.18,
                )
                .unwrap();
                let mut solver = Solver::new(model, start, &pinned, Vec3::new(0.0, 0.0, -9.81));
                let flat_coords =
                    |s: &Solver| -> Vec<f64> { s.x.iter().flat_map(|v| [v.x, v.y, v.z]).collect() };
                let s0 = StageHash::of_f64s("start", &flat_coords(&solver));
                solver.step(0.01).unwrap();
                let s1 = StageHash::of_f64s("step-1", &flat_coords(&solver));
                solver.step(0.01).unwrap();
                let s2 = StageHash::of_f64s("step-2", &flat_coords(&solver));
                StagedTrace {
                    stages: vec![s0, s1, s2],
                }
            }),
        };
        let config = AuditConfig {
            matrix: WorkerMatrix::explicit(vec![1, 2, 4]),
            repeats: 2,
        };
        let report = audit(&subject, &config);
        assert!(
            report.identical(),
            "staged drape trace must be bit-identical"
        );
        assert_eq!(report.rows.len(), 6);
        assert_eq!(report.baseline.stages[0].sha256.len(), 64);

        // Seed a divergence at stage 1 ("step-1") on workers > 1 and verify first-stage localization.
        let mut mutated = report.baseline.clone();
        mutated.stages[1].sha256 = sha256_stage(b"perturbed-step-1");
        let loc = locate_divergence(&report.baseline, &mutated, 2, 0).expect("must locate");
        assert_eq!(loc.first_stage, 1);
        assert_eq!(loc.stage_label, "step-1");
        assert_ne!(loc.baseline_sha256, loc.observed_sha256);
    }

    #[test]
    fn arrival_order_violation_and_cross_isa_classification_work_with_sha256() {
        let arrival = Subject {
            name: "selftest-arrival-order",
            run: Box::new(|workers| {
                let inputs: Vec<f64> = (1..=64).map(|i| 1.0 / f64::from(i)).collect();
                let mut sum = 0.0f64;
                let stride = workers.max(1);
                for lane in 0..stride {
                    let mut i = lane;
                    while i < inputs.len() {
                        sum += inputs[i];
                        i += stride;
                    }
                }
                StagedTrace {
                    stages: vec![
                        StageHash::of_f64s("load", &inputs),
                        StageHash::of_f64s("reduce", &[sum]),
                        StageHash::of("publish", b"published"),
                    ],
                }
            }),
        };
        let report = audit(
            &arrival,
            &AuditConfig {
                matrix: WorkerMatrix::explicit(vec![1, 2, 4]),
                repeats: 2,
            },
        );
        assert!(!report.identical());
        for d in &report.divergences {
            assert_eq!(d.first_stage, 1);
            assert_eq!(d.stage_label, "reduce");
        }

        let base = 1.000_000_000_000_1_f64;
        let one_ulp = f64::from_bits(base.to_bits() + 1);
        let mut rows_a = BTreeMap::new();
        rows_a.insert(
            "det/atan2".to_owned(),
            LedgerRow {
                sha256: sha256_f64s(&[base]),
                value_bits: Some(base.to_bits()),
            },
        );
        let mut rows_b = BTreeMap::new();
        rows_b.insert(
            "det/atan2".to_owned(),
            LedgerRow {
                sha256: sha256_f64s(&[one_ulp]),
                value_bits: Some(one_ulp.to_bits()),
            },
        );
        let a = IsaLedger {
            isa: "aarch64".to_owned(),
            rows: rows_a,
        };
        let b = IsaLedger {
            isa: "wasm32".to_owned(),
            rows: rows_b,
        };
        let mut policy = DivergencePolicy::default();
        policy.declared.insert(
            "det/atan2".to_owned(),
            DivergenceClass::LibmUlp { max_ulps: 1 },
        );
        let classified = classify_cross_isa(&a, &b, &policy);
        assert!(classified.clean(), "{}", classified.render_markdown());
        assert!(!classify_cross_isa(&a, &b, &DivergencePolicy::default()).clean());
    }
}

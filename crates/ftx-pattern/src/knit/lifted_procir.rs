#![forbid(unsafe_code)]

//! Pure-Kernel Leaf lifted from `/dp/frankensim/crates/fs-procir/src/kernel/` (`FS-K03`).
//!
//! Provenance:
//! - Source files: `/dp/frankensim/crates/fs-procir/src/kernel/{mod.rs,automaton.rs,vm.rs,golden_vectors.rs}`
//! - Upstream bead: `FS-K03` (`/dp/frankensim` workspace snapshot `2026-11-27`; note `/dp/frankensim`
//!   has no `.git` directory, post-`frankensim-source-manifest.json` `source_root`
//!   `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`)
//! - Pinned golden digests:
//!   - `PROCIR_KERNEL_GOLDEN_FNV64 = 0xf600_d8c9_7e7d_3854`
//!   - `PROCIR_KERNEL_GOLDEN_SHA256 = 0644c259c5c0be0222d926027419a2980a2bfa92367336f38aab82ad0ed3ad35`
//! - Local adaptations:
//!   - Packaged as a single leaf file with inline `pub mod` submodules (`automaton`, `vm`,
//!     `golden_vectors`) to preserve upstream paths with zero new crate dependencies.
//!   - Crate root `crates/ftx-pattern/src/lib.rs` enforces `#![forbid(unsafe_code)]` alongside this
//!     leaf module's `#![forbid(unsafe_code)]`.
//!
//! Invariants of this leaf module:
//! - `#![forbid(unsafe_code)]`
//! - Zero `HashMap` / `HashSet` iteration (uses `BTreeMap` / `BTreeSet` exclusively)
//! - Zero `mul_add` or platform `libm` calls

pub use automaton::{
    ActuatorState, AutomatonSemantics, SlotRailSemantics, SlotRailState, StructuredViolation,
    ViolationKind,
};
pub use vm::{DiscreteVmReceipt, run_slot_rail_vm, run_vm};

/// Current canonical schema version for [`ProcessProgram`].
pub const PROCESS_PROGRAM_SCHEMA_VERSION: u32 = 1;

/// Discrete coordinate identifying one resource slot inside a named bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotRef {
    /// Owning slot-bank identifier (`SlotBankSpec::bank_id`).
    pub bank_id: u16,
    /// Signed integer slot index within `[min_slot, max_slot]`.
    pub slot: i32,
}

/// Specification of one discrete resource slot bank (e.g., creel supply bank,
/// mandrel deposition zones, holding buffer, or discrete station array).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotBankSpec {
    /// Unique identifier for this bank.
    pub bank_id: u16,
    /// Human-readable domain-neutral bank label.
    pub name: String,
    /// Inclusive minimum valid slot index.
    pub min_slot: i32,
    /// Inclusive maximum valid slot index.
    pub max_slot: i32,
    /// Maximum number of material tokens allowed simultaneously in one slot.
    pub max_occupancy: u32,
    /// Whether active material-forming operations (`ProcessOp::ApplyStep`) are
    /// allowed on this bank (false for transfer/holding-only auxiliary banks).
    pub allows_apply_step: bool,
}

/// Lifecycle state of a mobile actuator on a kinematic rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActuatorLifecycle {
    /// Not mounted or disabled; cannot execute `MoveActuator` or `ApplyStep`
    /// until activated via `InsertToken`.
    Inactive,
    /// Parked at a safe home/clear position; may transition to `Active` via
    /// `MoveActuator` or `InsertToken`, but cannot directly `ApplyStep`.
    Parked,
    /// Engaged in the active work envelope; may execute `MoveActuator`,
    /// `ApplyStep`, `InsertToken`, or `ParkActuator`.
    Active,
}

/// Initial state descriptor for one actuator mounted on a [`RailSpec`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActuatorSpec {
    /// Unique actuator identifier across the entire machine topology.
    pub actuator_id: u16,
    /// Initial discrete position along the owning rail.
    pub initial_position: i32,
    /// Initial lifecycle state at step 0.
    pub initial_lifecycle: ActuatorLifecycle,
}

/// Specification of one kinematic actuator rail enforcing exclusion envelopes
/// and discrete rail-offset alignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailSpec {
    /// Unique rail identifier.
    pub rail_id: u16,
    /// Human-readable rail label.
    pub name: String,
    /// Actuators sharing this physical rail axis.
    pub actuators: Vec<ActuatorSpec>,
    /// Inclusive minimum position along this rail.
    pub min_position: i32,
    /// Inclusive maximum position along this rail.
    pub max_position: i32,
    /// Minimum required slot separation `|pos_a - pos_b| >= min_clearance_slots`
    /// between any two non-`Inactive` actuators sharing this rail.
    pub min_clearance_slots: u32,
    /// Minimum permitted discrete offset in ticks for `SetRailOffset`.
    pub min_offset_ticks: i32,
    /// Maximum permitted discrete offset in ticks for `SetRailOffset`.
    pub max_offset_ticks: i32,
    /// Required tick alignment step (`offset_ticks % offset_step_ticks == 0`, strictly `> 0`).
    pub offset_step_ticks: i32,
}

/// Classification of a symbolic scalar bound enforced by the L2 discrete VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScalarBoundKind {
    /// Maximum absolute discrete rail offset in ticks (`|offset_ticks| <= limit`).
    MaxRailOffsetTicks,
    /// Maximum discrete span between consecutive engaged/transferred slots (`span <= limit`).
    MaxSpanSlots,
    /// Minimum strictly positive feed rate (`v_min > 0`).
    MinFeedRate,
    /// Minimum strictly positive dwell/pause duration (`tau_min > 0`).
    MinDwellTime,
    /// Minimum value for a named scalar process parameter.
    MinNamedParam,
    /// Maximum value for a named scalar process parameter.
    MaxNamedParam,
}

/// Symbolic scalar constraint enforced at L2 before L3/L4 physical solvers run.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarBoundSpec {
    /// Parameter name matched by `SetScalarParam` (or label for structural limits).
    pub param_name: String,
    /// Semantic class of the scalar bound.
    pub kind: ScalarBoundKind,
    /// Numeric threshold value (must be finite; strictly positive for `MinFeedRate`/`MinDwellTime`).
    pub limit_value: f64,
    /// When `true`, enforces strict inequality (`>` / `<`); when `false`, inclusive (`>=` / `<=`).
    pub strict: bool,
}

/// Content-addressed machine topology specification consumed by the L2 VM.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineTopologySpec {
    /// Canonical SHA-256 digest of the topology specification fields.
    pub spec_sha256: [u8; 32],
    /// Discrete resource slot banks.
    pub slot_banks: Vec<SlotBankSpec>,
    /// Kinematic actuator rails with collision envelopes.
    pub actuator_rails: Vec<RailSpec>,
    /// Symbolic scalar limits enforced during execution.
    pub scalar_limits: Vec<ScalarBoundSpec>,
}

/// Domain-agnostic discrete fabrication operation executed by the L2 VM.
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessOp {
    /// Insert material token `token_id` into `slot` using `actuator_id`
    /// (activating the actuator at `slot.slot` with collision checking).
    InsertToken {
        /// Actuator performing the insertion.
        actuator_id: u16,
        /// Target resource slot.
        slot: SlotRef,
        /// Caller-assigned material token identifier.
        token_id: u64,
    },
    /// Remove the top material token from `slot` (optionally asserting its ID).
    RemoveToken {
        /// Target resource slot.
        slot: SlotRef,
        /// If `Some(id)`, requires the removed top token to equal `id`.
        token_id: Option<u64>,
    },
    /// Transfer `count` tokens from the top of `from` to `to`, enforcing
    /// capacity and `MaxSpanSlots`.
    TransferToken {
        /// Source slot.
        from: SlotRef,
        /// Destination slot.
        to: SlotRef,
        /// Number of tokens to transfer (must be `>= 1`).
        count: u32,
    },
    /// Move `actuator_id` along its rail to `target_position`, checking bounds,
    /// lifecycle, exclusion clearance, and pass-through collisions.
    MoveActuator {
        /// Actuator to move.
        actuator_id: u16,
        /// Destination discrete rail coordinate.
        target_position: i32,
    },
    /// Park `actuator_id` at `park_position`, transitioning it to `Parked`.
    ParkActuator {
        /// Actuator to park.
        actuator_id: u16,
        /// Safe parking coordinate on the actuator's rail.
        park_position: i32,
    },
    /// Set discrete offset `offset_ticks` on `rail_id`.
    SetRailOffset {
        /// Target rail identifier.
        rail_id: u16,
        /// New discrete offset in ticks.
        offset_ticks: i32,
    },
    /// Execute a material-forming step at `slot` using active `actuator_id`,
    /// popping `pop_tokens` and pushing `push_tokens` onto `slot`'s token stack.
    ApplyStep {
        /// Active actuator executing the step.
        actuator_id: u16,
        /// Target resource slot (must permit `ApplyStep`).
        slot: SlotRef,
        /// Number of new tokens pushed onto the slot stack.
        push_tokens: u32,
        /// Number of existing tokens consumed from the slot stack.
        pop_tokens: u32,
    },
    /// Execute a controlled dwell or pause of `duration_s` seconds.
    DwellOrPause {
        /// Duration in seconds (checked against `MinDwellTime`).
        duration_s: f64,
        /// Structured domain-neutral reason code.
        reason_code: u32,
    },
    /// Set a named scalar process parameter, validated against `scalar_limits`.
    SetScalarParam {
        /// Parameter identifier (e.g., `"feed_rate_m_s"`, `"cut_length_m"`).
        name: String,
        /// Numeric value (must be finite).
        value: f64,
    },
}

/// Sequential fabrication process program bound to a [`MachineTopologySpec`].
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessProgram {
    /// Schema version (must match [`PROCESS_PROGRAM_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Canonical SHA-256 digest of the target [`MachineTopologySpec`].
    pub machine_spec_sha256: [u8; 32],
    /// Ordered sequence of discrete fabrication operations.
    pub ops: Vec<ProcessOp>,
}

impl MachineTopologySpec {
    /// Compute the deterministic canonical SHA-256 digest of this spec's
    /// structural fields (ignoring the stored `spec_sha256` field itself).
    #[must_use]
    pub fn canonical_sha256(&self) -> [u8; 32] {
        let mut sha = Sha256::new();
        sha.update(b"fs-procir:machine-topology-spec:v1");
        sha.feed_u64(self.slot_banks.len() as u64);
        for bank in &self.slot_banks {
            sha.feed_u16(bank.bank_id);
            sha.feed_str(&bank.name);
            sha.feed_i32(bank.min_slot);
            sha.feed_i32(bank.max_slot);
            sha.feed_u32(bank.max_occupancy);
            sha.update(&[u8::from(bank.allows_apply_step)]);
        }
        sha.feed_u64(self.actuator_rails.len() as u64);
        for rail in &self.actuator_rails {
            sha.feed_u16(rail.rail_id);
            sha.feed_str(&rail.name);
            sha.feed_u64(rail.actuators.len() as u64);
            for act in &rail.actuators {
                sha.feed_u16(act.actuator_id);
                sha.feed_i32(act.initial_position);
                sha.update(&[lifecycle_tag(act.initial_lifecycle)]);
            }
            sha.feed_i32(rail.min_position);
            sha.feed_i32(rail.max_position);
            sha.feed_u32(rail.min_clearance_slots);
            sha.feed_i32(rail.min_offset_ticks);
            sha.feed_i32(rail.max_offset_ticks);
            sha.feed_i32(rail.offset_step_ticks);
        }
        sha.feed_u64(self.scalar_limits.len() as u64);
        for lim in &self.scalar_limits {
            sha.feed_str(&lim.param_name);
            sha.update(&[scalar_bound_tag(lim.kind)]);
            sha.feed_f64(lim.limit_value);
            sha.update(&[u8::from(lim.strict)]);
        }
        sha.finalize()
    }

    /// Return a copy of `self` with `spec_sha256` sealed to [`Self::canonical_sha256`].
    #[must_use]
    pub fn sealed(mut self) -> Self {
        self.spec_sha256 = self.canonical_sha256();
        self
    }
}

impl ProcessProgram {
    /// Compute the deterministic canonical SHA-256 digest of this program.
    #[must_use]
    pub fn canonical_sha256(&self) -> [u8; 32] {
        let mut sha = Sha256::new();
        sha.update(b"fs-procir:process-program:v1");
        sha.feed_u32(self.schema_version);
        sha.update(&self.machine_spec_sha256);
        sha.feed_u64(self.ops.len() as u64);
        for op in &self.ops {
            feed_op(&mut sha, op);
        }
        sha.finalize()
    }
}

fn lifecycle_tag(lc: ActuatorLifecycle) -> u8 {
    match lc {
        ActuatorLifecycle::Inactive => 0,
        ActuatorLifecycle::Parked => 1,
        ActuatorLifecycle::Active => 2,
    }
}

fn scalar_bound_tag(kind: ScalarBoundKind) -> u8 {
    match kind {
        ScalarBoundKind::MaxRailOffsetTicks => 1,
        ScalarBoundKind::MaxSpanSlots => 2,
        ScalarBoundKind::MinFeedRate => 3,
        ScalarBoundKind::MinDwellTime => 4,
        ScalarBoundKind::MinNamedParam => 5,
        ScalarBoundKind::MaxNamedParam => 6,
    }
}

fn feed_op(sha: &mut Sha256, op: &ProcessOp) {
    match op {
        ProcessOp::InsertToken {
            actuator_id,
            slot,
            token_id,
        } => {
            sha.update(&[1]);
            sha.feed_u16(*actuator_id);
            sha.feed_u16(slot.bank_id);
            sha.feed_i32(slot.slot);
            sha.feed_u64(*token_id);
        }
        ProcessOp::RemoveToken { slot, token_id } => {
            sha.update(&[2]);
            sha.feed_u16(slot.bank_id);
            sha.feed_i32(slot.slot);
            match token_id {
                Some(id) => {
                    sha.update(&[1]);
                    sha.feed_u64(*id);
                }
                None => sha.update(&[0]),
            }
        }
        ProcessOp::TransferToken { from, to, count } => {
            sha.update(&[3]);
            sha.feed_u16(from.bank_id);
            sha.feed_i32(from.slot);
            sha.feed_u16(to.bank_id);
            sha.feed_i32(to.slot);
            sha.feed_u32(*count);
        }
        ProcessOp::MoveActuator {
            actuator_id,
            target_position,
        } => {
            sha.update(&[4]);
            sha.feed_u16(*actuator_id);
            sha.feed_i32(*target_position);
        }
        ProcessOp::ParkActuator {
            actuator_id,
            park_position,
        } => {
            sha.update(&[5]);
            sha.feed_u16(*actuator_id);
            sha.feed_i32(*park_position);
        }
        ProcessOp::SetRailOffset {
            rail_id,
            offset_ticks,
        } => {
            sha.update(&[6]);
            sha.feed_u16(*rail_id);
            sha.feed_i32(*offset_ticks);
        }
        ProcessOp::ApplyStep {
            actuator_id,
            slot,
            push_tokens,
            pop_tokens,
        } => {
            sha.update(&[7]);
            sha.feed_u16(*actuator_id);
            sha.feed_u16(slot.bank_id);
            sha.feed_i32(slot.slot);
            sha.feed_u32(*push_tokens);
            sha.feed_u32(*pop_tokens);
        }
        ProcessOp::DwellOrPause {
            duration_s,
            reason_code,
        } => {
            sha.update(&[8]);
            sha.feed_f64(*duration_s);
            sha.feed_u32(*reason_code);
        }
        ProcessOp::SetScalarParam { name, value } => {
            sha.update(&[9]);
            sha.feed_str(name);
            sha.feed_f64(*value);
        }
    }
}

// ---------------------------------------------------------------------------
// Self-contained deterministic hashing (FNV-1a 64 + FIPS 180-4 SHA-256).
// ---------------------------------------------------------------------------

/// Streaming FNV-1a 64-bit hasher for kernel golden vectors.
#[derive(Clone, Debug)]
pub(crate) struct Fnv1a64(u64);

impl Fnv1a64 {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub(crate) fn finish(self) -> u64 {
        self.0
    }
}

/// Pure safe-Rust FIPS 180-4 SHA-256 hasher (zero cross-crate dependencies).
#[derive(Clone, Debug)]
pub(crate) struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bits: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    pub(crate) fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bits: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let j = 4 * i;
            *word = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for (&k_i, &w_i) in Self::K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k_i)
                .wrapping_add(w_i);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((bytes.len() as u64) * 8);
        let mut idx = 0;
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(bytes.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&bytes[..take]);
            self.buf_len += take;
            idx += take;
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while idx + 64 <= bytes.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&bytes[idx..idx + 64]);
            Self::compress(&mut self.state, &block);
            idx += 64;
        }
        let rem = bytes.len() - idx;
        if rem > 0 {
            self.buf[..rem].copy_from_slice(&bytes[idx..]);
            self.buf_len = rem;
        }
    }

    pub(crate) fn feed_u16(&mut self, v: u16) {
        self.update(&v.to_le_bytes());
    }

    pub(crate) fn feed_u32(&mut self, v: u32) {
        self.update(&v.to_le_bytes());
    }

    pub(crate) fn feed_i32(&mut self, v: i32) {
        self.update(&v.to_le_bytes());
    }

    pub(crate) fn feed_u64(&mut self, v: u64) {
        self.update(&v.to_le_bytes());
    }

    pub(crate) fn feed_f64(&mut self, x: f64) {
        let norm = if x == 0.0 { 0.0 } else { x };
        self.update(&norm.to_bits().to_le_bytes());
    }

    pub(crate) fn feed_str(&mut self, s: &str) {
        self.feed_u64(s.len() as u64);
        self.update(s.as_bytes());
    }

    pub(crate) fn finalize(mut self) -> [u8; 32] {
        let total_bits = self.total_bits;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            for b in &mut self.buf[self.buf_len..64] {
                *b = 0;
            }
            let block = self.buf;
            Self::compress(&mut self.state, &block);
            self.buf = [0u8; 64];
            self.buf_len = 0;
        } else {
            for b in &mut self.buf[self.buf_len..56] {
                *b = 0;
            }
        }
        self.buf[56..64].copy_from_slice(&total_bits.to_be_bytes());
        let block = self.buf;
        Self::compress(&mut self.state, &block);
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

pub mod automaton {
    //! Pluggable [`AutomatonSemantics`] trait and generic built-in
    //! [`SlotRailSemantics`] discrete fabrication state machine.

    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        ActuatorLifecycle, MachineTopologySpec, ProcessOp, RailSpec, ScalarBoundKind,
        ScalarBoundSpec, SlotBankSpec, SlotRef,
    };

    /// Enumerated fail-closed violation classes detected by the L2 discrete VM.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum ViolationKind {
        /// Program or spec SHA-256 digest does not match canonical topology digest.
        SpecHashMismatch,
        /// Schema version is unsupported.
        UnsupportedSchemaVersion,
        /// `MachineTopologySpec` itself is structurally invalid.
        InvalidSpec,
        /// Operation references an undeclared slot bank.
        UnknownBank,
        /// Operation references an undeclared actuator rail.
        UnknownRail,
        /// Operation references an undeclared actuator.
        UnknownActuator,
        /// Target slot lies outside `[bank.min_slot, bank.max_slot]`.
        SlotOutOfRange,
        /// Slot token stack would exceed `bank.max_occupancy`.
        SlotOccupancyOverflow,
        /// Slot lacks enough tokens (or requested token ID mismatch) for pop/transfer.
        SlotUnderflow,
        /// `ApplyStep` attempted on a bank with `allows_apply_step == false`.
        ApplyStepForbiddenOnBank,
        /// Actuator destination lies outside `[rail.min_position, rail.max_position]`.
        ActuatorOutOfRange,
        /// Actuator lifecycle state prohibits the requested operation.
        ActuatorNotActive,
        /// Two actuators on the same rail violate `min_clearance_slots` or cross paths.
        RailCollision,
        /// `SetRailOffset` value lies outside `[min_offset_ticks, max_offset_ticks]`.
        RailOffsetOutOfRange,
        /// `SetRailOffset` value is not aligned to `offset_step_ticks`.
        RailOffsetMisaligned,
        /// Engagement or transfer span exceeds `MaxSpanSlots`.
        SpanLimitExceeded,
        /// Scalar process parameter, dwell, or offset exceeds a `ScalarBoundSpec`.
        ScalarBoundViolated,
        /// A floating-point parameter or duration is NaN or infinite.
        NonFiniteScalar,
        /// Actuator remained `Active` at program termination when parking is required.
        UnparkedActuatorAtFinalize,
    }

    impl ViolationKind {
        /// Stable numeric discriminant used in canonical receipt hashing.
        #[must_use]
        pub const fn code(self) -> u16 {
            match self {
                Self::SpecHashMismatch => 1,
                Self::UnsupportedSchemaVersion => 2,
                Self::InvalidSpec => 3,
                Self::UnknownBank => 4,
                Self::UnknownRail => 5,
                Self::UnknownActuator => 6,
                Self::SlotOutOfRange => 7,
                Self::SlotOccupancyOverflow => 8,
                Self::SlotUnderflow => 9,
                Self::ApplyStepForbiddenOnBank => 10,
                Self::ActuatorOutOfRange => 11,
                Self::ActuatorNotActive => 12,
                Self::RailCollision => 13,
                Self::RailOffsetOutOfRange => 14,
                Self::RailOffsetMisaligned => 15,
                Self::SpanLimitExceeded => 16,
                Self::ScalarBoundViolated => 17,
                Self::NonFiniteScalar => 18,
                Self::UnparkedActuatorAtFinalize => 19,
            }
        }
    }

    /// Deterministic structured violation emitted by [`AutomatonSemantics`].
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StructuredViolation {
        /// Operation index where the violation occurred (`None` for init/finalize).
        pub step_idx: Option<usize>,
        /// Typed violation classification.
        pub kind: ViolationKind,
        /// Deterministic diagnostic explanation.
        pub detail: String,
    }

    /// Pluggable discrete state-transition semantics executed by [`super::run_vm`].
    pub trait AutomatonSemantics {
        /// Concrete deterministic machine state.
        type State: Clone;

        /// Construct and validate initial state from `spec`.
        fn init_state(
            &self,
            spec: &MachineTopologySpec,
        ) -> Result<Self::State, Vec<StructuredViolation>>;

        /// Validate and execute one [`ProcessOp`] at index `step_idx`.
        fn step(
            &self,
            spec: &MachineTopologySpec,
            state: &mut Self::State,
            op: &ProcessOp,
            step_idx: usize,
        ) -> Result<(), StructuredViolation>;

        /// Run end-of-program invariants after all operations complete.
        fn finalize(
            &self,
            spec: &MachineTopologySpec,
            state: &Self::State,
        ) -> Vec<StructuredViolation>;

        /// Peak single-slot token occupancy observed during execution.
        fn peak_slot_occupancy(&self, state: &Self::State) -> u32;

        /// Maximum discrete slot span observed across transfers/steps.
        fn max_span_slots(&self, state: &Self::State) -> u32;
    }

    /// Runtime state of a single mobile actuator inside [`SlotRailState`].
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ActuatorState {
        /// Owning rail identifier.
        pub rail_id: u16,
        /// Current discrete rail coordinate.
        pub position: i32,
        /// Current lifecycle state.
        pub lifecycle: ActuatorLifecycle,
        /// Most recent engaged resource slot (used to bound contiguous spans).
        pub last_engaged_slot: Option<SlotRef>,
    }

    /// Complete deterministic runtime state of [`SlotRailSemantics`].
    #[derive(Clone, Debug, PartialEq)]
    pub struct SlotRailState {
        /// Token stacks keyed in sorted `BTreeMap` order by [`SlotRef`].
        pub slot_stacks: BTreeMap<SlotRef, Vec<u64>>,
        /// Actuator states keyed by `actuator_id`.
        pub actuators: BTreeMap<u16, ActuatorState>,
        /// Current discrete rail offsets (ticks) keyed by `rail_id`.
        pub rail_offsets: BTreeMap<u16, i32>,
        /// Latest scalar parameter values keyed by parameter name.
        pub scalar_params: BTreeMap<String, f64>,
        /// Monotonic token counter for `ApplyStep` pushed tokens.
        pub next_synthetic_token_id: u64,
        /// Peak slot occupancy observed across all steps.
        pub peak_occupancy: u32,
        /// Maximum discrete span observed across all steps.
        pub max_span_slots: u32,
    }

    /// Generic built-in discrete fabrication automaton enforcing bounded slot
    /// occupancy stacks, kinematic rail collision envelopes, actuator lifecycle
    /// state machines, rail offset alignment, and symbolic scalar bounds.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SlotRailSemantics {
        /// When `true`, `finalize` requires every actuator to end `Parked` or `Inactive`.
        pub require_parked_at_finalize: bool,
        /// When `true`, `ApplyStep` requires the actuator's rail coordinate to match
        /// `slot.slot` (enforcing alignment before forming).
        pub require_actuator_aligned_for_apply: bool,
    }

    impl Default for SlotRailSemantics {
        fn default() -> Self {
            Self {
                require_parked_at_finalize: true,
                require_actuator_aligned_for_apply: true,
            }
        }
    }

    fn find_bank(spec: &MachineTopologySpec, bank_id: u16) -> Option<&SlotBankSpec> {
        spec.slot_banks.iter().find(|b| b.bank_id == bank_id)
    }

    fn find_rail(spec: &MachineTopologySpec, rail_id: u16) -> Option<&RailSpec> {
        spec.actuator_rails.iter().find(|r| r.rail_id == rail_id)
    }

    fn check_scalar_limit(val: f64, lim: &ScalarBoundSpec) -> bool {
        match lim.kind {
            ScalarBoundKind::MinFeedRate
            | ScalarBoundKind::MinDwellTime
            | ScalarBoundKind::MinNamedParam => {
                if lim.strict {
                    val > lim.limit_value
                } else {
                    val >= lim.limit_value
                }
            }
            ScalarBoundKind::MaxRailOffsetTicks
            | ScalarBoundKind::MaxSpanSlots
            | ScalarBoundKind::MaxNamedParam => {
                if lim.strict {
                    val < lim.limit_value
                } else {
                    val <= lim.limit_value
                }
            }
        }
    }

    fn check_rail_move_clearance(
        rail: &RailSpec,
        actuators: &BTreeMap<u16, ActuatorState>,
        moving_id: u16,
        prev_pos: i32,
        prev_active_or_parked: bool,
        target_pos: i32,
        step_idx: usize,
    ) -> Result<(), StructuredViolation> {
        if target_pos < rail.min_position || target_pos > rail.max_position {
            return Err(StructuredViolation {
                step_idx: Some(step_idx),
                kind: ViolationKind::ActuatorOutOfRange,
                detail: format!(
                    "actuator {moving_id} target {target_pos} outside rail {} bounds [{}..={}]",
                    rail.rail_id, rail.min_position, rail.max_position
                ),
            });
        }
        let min_clear = i64::from(rail.min_clearance_slots);
        let tgt = i64::from(target_pos);
        let prev = i64::from(prev_pos);

        for (&other_id, other) in actuators {
            if other_id == moving_id || other.rail_id != rail.rail_id {
                continue;
            }
            if other.lifecycle == ActuatorLifecycle::Inactive {
                continue;
            }
            let oth = i64::from(other.position);
            // 1. Endpoint exclusion zone check.
            if (tgt - oth).abs() < min_clear {
                return Err(StructuredViolation {
                    step_idx: Some(step_idx),
                    kind: ViolationKind::RailCollision,
                    detail: format!(
                        "actuator {moving_id} at {target_pos} violates rail {} clearance {} against actuator {other_id} at {}",
                        rail.rail_id, rail.min_clearance_slots, other.position
                    ),
                });
            }
            // 2. Swept path-crossing check (cannot pass through another actuator on the same rail).
            if prev_active_or_parked {
                if prev < oth && tgt + min_clear > oth {
                    return Err(StructuredViolation {
                        step_idx: Some(step_idx),
                        kind: ViolationKind::RailCollision,
                        detail: format!(
                            "actuator {moving_id} move {prev_pos}->{target_pos} crosses actuator {other_id} at {} on rail {}",
                            other.position, rail.rail_id
                        ),
                    });
                }
                if prev > oth && tgt - min_clear < oth {
                    return Err(StructuredViolation {
                        step_idx: Some(step_idx),
                        kind: ViolationKind::RailCollision,
                        detail: format!(
                            "actuator {moving_id} move {prev_pos}->{target_pos} crosses actuator {other_id} at {} on rail {}",
                            other.position, rail.rail_id
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    fn check_span_bounds(
        spec: &MachineTopologySpec,
        span: u32,
        step_idx: usize,
    ) -> Result<(), StructuredViolation> {
        for lim in &spec.scalar_limits {
            if lim.kind == ScalarBoundKind::MaxSpanSlots
                && !check_scalar_limit(f64::from(span), lim)
            {
                return Err(StructuredViolation {
                    step_idx: Some(step_idx),
                    kind: ViolationKind::SpanLimitExceeded,
                    detail: format!(
                        "slot span {span} exceeds {} limit {}",
                        lim.param_name, lim.limit_value
                    ),
                });
            }
        }
        Ok(())
    }

    impl AutomatonSemantics for SlotRailSemantics {
        type State = SlotRailState;

        #[allow(clippy::too_many_lines)]
        fn init_state(
            &self,
            spec: &MachineTopologySpec,
        ) -> Result<Self::State, Vec<StructuredViolation>> {
            let mut errs = Vec::new();
            let mut seen_banks = BTreeSet::new();
            for bank in &spec.slot_banks {
                if !seen_banks.insert(bank.bank_id) {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!("duplicate bank_id {}", bank.bank_id),
                    });
                }
                if bank.min_slot > bank.max_slot || bank.max_occupancy == 0 {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!(
                            "invalid slot range or zero capacity on bank {}",
                            bank.bank_id
                        ),
                    });
                }
            }

            let mut seen_rails = BTreeSet::new();
            let mut actuators = BTreeMap::new();
            let mut rail_offsets = BTreeMap::new();
            for rail in &spec.actuator_rails {
                if !seen_rails.insert(rail.rail_id) {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!("duplicate rail_id {}", rail.rail_id),
                    });
                }
                if rail.min_position > rail.max_position
                    || rail.min_offset_ticks > rail.max_offset_ticks
                    || rail.offset_step_ticks <= 0
                {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!("invalid bounds or offset step on rail {}", rail.rail_id),
                    });
                }
                rail_offsets.insert(rail.rail_id, 0);
                for act in &rail.actuators {
                    if act.initial_position < rail.min_position
                        || act.initial_position > rail.max_position
                    {
                        errs.push(StructuredViolation {
                            step_idx: None,
                            kind: ViolationKind::InvalidSpec,
                            detail: format!(
                                "actuator {} initial position {} outside rail {} bounds",
                                act.actuator_id, act.initial_position, rail.rail_id
                            ),
                        });
                    }
                    let prev = actuators.insert(
                        act.actuator_id,
                        ActuatorState {
                            rail_id: rail.rail_id,
                            position: act.initial_position,
                            lifecycle: act.initial_lifecycle,
                            last_engaged_slot: None,
                        },
                    );
                    if prev.is_some() {
                        errs.push(StructuredViolation {
                            step_idx: None,
                            kind: ViolationKind::InvalidSpec,
                            detail: format!("duplicate actuator_id {}", act.actuator_id),
                        });
                    }
                }
                // Verify initial pairwise actuator separation on this rail.
                let min_clear = i64::from(rail.min_clearance_slots);
                for i in 0..rail.actuators.len() {
                    if rail.actuators[i].initial_lifecycle == ActuatorLifecycle::Inactive {
                        continue;
                    }
                    for j in (i + 1)..rail.actuators.len() {
                        if rail.actuators[j].initial_lifecycle == ActuatorLifecycle::Inactive {
                            continue;
                        }
                        let d = (i64::from(rail.actuators[i].initial_position)
                            - i64::from(rail.actuators[j].initial_position))
                        .abs();
                        if d < min_clear {
                            errs.push(StructuredViolation {
                                step_idx: None,
                                kind: ViolationKind::RailCollision,
                                detail: format!(
                                    "initial actuators {} and {} violate clearance {} on rail {}",
                                    rail.actuators[i].actuator_id,
                                    rail.actuators[j].actuator_id,
                                    rail.min_clearance_slots,
                                    rail.rail_id
                                ),
                            });
                        }
                    }
                }
            }

            for lim in &spec.scalar_limits {
                if !lim.limit_value.is_finite() {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::NonFiniteScalar,
                        detail: format!("non-finite limit on scalar bound {}", lim.param_name),
                    });
                }
                if matches!(
                    lim.kind,
                    ScalarBoundKind::MinFeedRate | ScalarBoundKind::MinDwellTime
                ) && lim.limit_value <= 0.0
                {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!(
                            "scalar bound {} requires strictly positive threshold (> 0)",
                            lim.param_name
                        ),
                    });
                }
                if matches!(
                    lim.kind,
                    ScalarBoundKind::MaxRailOffsetTicks | ScalarBoundKind::MaxSpanSlots
                ) && lim.limit_value < 0.0
                {
                    errs.push(StructuredViolation {
                        step_idx: None,
                        kind: ViolationKind::InvalidSpec,
                        detail: format!(
                            "scalar bound {} cannot have negative threshold",
                            lim.param_name
                        ),
                    });
                }
            }

            if !errs.is_empty() {
                return Err(errs);
            }

            Ok(SlotRailState {
                slot_stacks: BTreeMap::new(),
                actuators,
                rail_offsets,
                scalar_params: BTreeMap::new(),
                next_synthetic_token_id: 1_000_000,
                peak_occupancy: 0,
                max_span_slots: 0,
            })
        }

        #[allow(clippy::too_many_lines)]
        fn step(
            &self,
            spec: &MachineTopologySpec,
            state: &mut Self::State,
            op: &ProcessOp,
            step_idx: usize,
        ) -> Result<(), StructuredViolation> {
            match op {
                ProcessOp::InsertToken {
                    actuator_id,
                    slot,
                    token_id,
                } => {
                    let Some(bank) = find_bank(spec, slot.bank_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownBank,
                            detail: format!("unknown bank_id {}", slot.bank_id),
                        });
                    };
                    if slot.slot < bank.min_slot || slot.slot > bank.max_slot {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOutOfRange,
                            detail: format!(
                                "slot {} outside bank {} range [{}..={}]",
                                slot.slot, bank.bank_id, bank.min_slot, bank.max_slot
                            ),
                        });
                    }
                    let Some(act_snapshot) = state.actuators.get(actuator_id).cloned() else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownActuator,
                            detail: format!("unknown actuator_id {actuator_id}"),
                        });
                    };
                    let Some(rail) = find_rail(spec, act_snapshot.rail_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownRail,
                            detail: format!("unknown rail_id {}", act_snapshot.rail_id),
                        });
                    };
                    check_rail_move_clearance(
                        rail,
                        &state.actuators,
                        *actuator_id,
                        act_snapshot.position,
                        act_snapshot.lifecycle != ActuatorLifecycle::Inactive,
                        slot.slot,
                        step_idx,
                    )?;
                    if let Some(prev_slot) = act_snapshot.last_engaged_slot
                        && prev_slot.bank_id == slot.bank_id
                    {
                        let span = slot.slot.abs_diff(prev_slot.slot);
                        check_span_bounds(spec, span, step_idx)?;
                        if span > state.max_span_slots {
                            state.max_span_slots = span;
                        }
                    }
                    let current_len = state.slot_stacks.get(slot).map_or(0, Vec::len) as u32;
                    if current_len + 1 > bank.max_occupancy {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOccupancyOverflow,
                            detail: format!(
                                "bank {} slot {} occupancy {} exceeds cap {}",
                                bank.bank_id,
                                slot.slot,
                                current_len + 1,
                                bank.max_occupancy
                            ),
                        });
                    }
                    let stack = state.slot_stacks.entry(*slot).or_default();
                    stack.push(*token_id);
                    let occ = stack.len() as u32;
                    if occ > state.peak_occupancy {
                        state.peak_occupancy = occ;
                    }
                    if let Some(act) = state.actuators.get_mut(actuator_id) {
                        act.position = slot.slot;
                        act.lifecycle = ActuatorLifecycle::Active;
                        act.last_engaged_slot = Some(*slot);
                    }
                    Ok(())
                }

                ProcessOp::RemoveToken { slot, token_id } => {
                    let Some(bank) = find_bank(spec, slot.bank_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownBank,
                            detail: format!("unknown bank_id {}", slot.bank_id),
                        });
                    };
                    if slot.slot < bank.min_slot || slot.slot > bank.max_slot {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOutOfRange,
                            detail: format!("slot {} outside bank {}", slot.slot, bank.bank_id),
                        });
                    }
                    let Some(stack) = state.slot_stacks.get_mut(slot) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: format!("slot {slot:?} is empty"),
                        });
                    };
                    let Some(&top) = stack.last() else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: format!("slot {slot:?} is empty"),
                        });
                    };
                    if let Some(expected) = token_id
                        && top != *expected
                    {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: format!(
                                "slot {slot:?} top token {top} does not match expected {expected}"
                            ),
                        });
                    }
                    stack.pop();
                    Ok(())
                }

                ProcessOp::TransferToken { from, to, count } => {
                    if *count == 0 {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: "TransferToken count must be >= 1".to_string(),
                        });
                    }
                    let Some(from_bank) = find_bank(spec, from.bank_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownBank,
                            detail: format!("unknown source bank_id {}", from.bank_id),
                        });
                    };
                    let Some(to_bank) = find_bank(spec, to.bank_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownBank,
                            detail: format!("unknown destination bank_id {}", to.bank_id),
                        });
                    };
                    if from.slot < from_bank.min_slot || from.slot > from_bank.max_slot {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOutOfRange,
                            detail: format!("source slot {from:?} out of range"),
                        });
                    }
                    if to.slot < to_bank.min_slot || to.slot > to_bank.max_slot {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOutOfRange,
                            detail: format!("destination slot {to:?} out of range"),
                        });
                    }
                    let span = to.slot.abs_diff(from.slot);
                    check_span_bounds(spec, span, step_idx)?;

                    let from_len = state.slot_stacks.get(from).map_or(0, Vec::len) as u32;
                    if from_len < *count {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: format!(
                                "source slot {from:?} has {from_len} tokens, cannot transfer {count}"
                            ),
                        });
                    }
                    let to_len = if from == to {
                        from_len - *count
                    } else {
                        state.slot_stacks.get(to).map_or(0, Vec::len) as u32
                    };
                    if to_len + *count > to_bank.max_occupancy {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOccupancyOverflow,
                            detail: format!(
                                "destination slot {to:?} occupancy {} exceeds cap {}",
                                to_len + *count,
                                to_bank.max_occupancy
                            ),
                        });
                    }

                    let mut moved = Vec::with_capacity(*count as usize);
                    if let Some(src) = state.slot_stacks.get_mut(from) {
                        for _ in 0..*count {
                            if let Some(tok) = src.pop() {
                                moved.push(tok);
                            }
                        }
                    }
                    moved.reverse();
                    let dst = state.slot_stacks.entry(*to).or_default();
                    dst.extend(moved);
                    let occ = dst.len() as u32;
                    if occ > state.peak_occupancy {
                        state.peak_occupancy = occ;
                    }
                    if span > state.max_span_slots {
                        state.max_span_slots = span;
                    }
                    Ok(())
                }

                ProcessOp::MoveActuator {
                    actuator_id,
                    target_position,
                } => {
                    let Some(act_snapshot) = state.actuators.get(actuator_id).cloned() else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownActuator,
                            detail: format!("unknown actuator_id {actuator_id}"),
                        });
                    };
                    if act_snapshot.lifecycle == ActuatorLifecycle::Inactive {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::ActuatorNotActive,
                            detail: format!(
                                "actuator {actuator_id} is Inactive and cannot move without activation"
                            ),
                        });
                    }
                    let Some(rail) = find_rail(spec, act_snapshot.rail_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownRail,
                            detail: format!("unknown rail_id {}", act_snapshot.rail_id),
                        });
                    };
                    check_rail_move_clearance(
                        rail,
                        &state.actuators,
                        *actuator_id,
                        act_snapshot.position,
                        true,
                        *target_position,
                        step_idx,
                    )?;
                    if let Some(act) = state.actuators.get_mut(actuator_id) {
                        act.position = *target_position;
                        act.lifecycle = ActuatorLifecycle::Active;
                    }
                    Ok(())
                }

                ProcessOp::ParkActuator {
                    actuator_id,
                    park_position,
                } => {
                    let Some(act_snapshot) = state.actuators.get(actuator_id).cloned() else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownActuator,
                            detail: format!("unknown actuator_id {actuator_id}"),
                        });
                    };
                    let Some(rail) = find_rail(spec, act_snapshot.rail_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownRail,
                            detail: format!("unknown rail_id {}", act_snapshot.rail_id),
                        });
                    };
                    check_rail_move_clearance(
                        rail,
                        &state.actuators,
                        *actuator_id,
                        act_snapshot.position,
                        act_snapshot.lifecycle != ActuatorLifecycle::Inactive,
                        *park_position,
                        step_idx,
                    )?;
                    if let Some(act) = state.actuators.get_mut(actuator_id) {
                        act.position = *park_position;
                        act.lifecycle = ActuatorLifecycle::Parked;
                        act.last_engaged_slot = None;
                    }
                    Ok(())
                }

                ProcessOp::SetRailOffset {
                    rail_id,
                    offset_ticks,
                } => {
                    let Some(rail) = find_rail(spec, *rail_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownRail,
                            detail: format!("unknown rail_id {rail_id}"),
                        });
                    };
                    if *offset_ticks < rail.min_offset_ticks
                        || *offset_ticks > rail.max_offset_ticks
                    {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::RailOffsetOutOfRange,
                            detail: format!(
                                "rail {rail_id} offset {offset_ticks} outside [{}..={}]",
                                rail.min_offset_ticks, rail.max_offset_ticks
                            ),
                        });
                    }
                    if offset_ticks % rail.offset_step_ticks != 0 {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::RailOffsetMisaligned,
                            detail: format!(
                                "rail {rail_id} offset {offset_ticks} not a multiple of step {}",
                                rail.offset_step_ticks
                            ),
                        });
                    }
                    let abs_ticks = f64::from(offset_ticks.unsigned_abs());
                    for lim in &spec.scalar_limits {
                        if lim.kind == ScalarBoundKind::MaxRailOffsetTicks
                            && !check_scalar_limit(abs_ticks, lim)
                        {
                            return Err(StructuredViolation {
                                step_idx: Some(step_idx),
                                kind: ViolationKind::ScalarBoundViolated,
                                detail: format!(
                                    "rail {rail_id} offset magnitude {} violates {} limit {}",
                                    abs_ticks, lim.param_name, lim.limit_value
                                ),
                            });
                        }
                    }
                    state.rail_offsets.insert(*rail_id, *offset_ticks);
                    Ok(())
                }

                ProcessOp::ApplyStep {
                    actuator_id,
                    slot,
                    push_tokens,
                    pop_tokens,
                } => {
                    let Some(bank) = find_bank(spec, slot.bank_id) else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownBank,
                            detail: format!("unknown bank_id {}", slot.bank_id),
                        });
                    };
                    if !bank.allows_apply_step {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::ApplyStepForbiddenOnBank,
                            detail: format!(
                                "bank {} ({}) forbids direct ApplyStep operations",
                                bank.bank_id, bank.name
                            ),
                        });
                    }
                    if slot.slot < bank.min_slot || slot.slot > bank.max_slot {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOutOfRange,
                            detail: format!("slot {slot:?} outside bank range"),
                        });
                    }
                    let Some(act_snapshot) = state.actuators.get(actuator_id).cloned() else {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::UnknownActuator,
                            detail: format!("unknown actuator_id {actuator_id}"),
                        });
                    };
                    if act_snapshot.lifecycle != ActuatorLifecycle::Active {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::ActuatorNotActive,
                            detail: format!(
                                "actuator {actuator_id} lifecycle {:?} must be Active for ApplyStep",
                                act_snapshot.lifecycle
                            ),
                        });
                    }
                    if self.require_actuator_aligned_for_apply && act_snapshot.position != slot.slot
                    {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::ActuatorOutOfRange,
                            detail: format!(
                                "actuator {actuator_id} at {} is not aligned with target slot {}",
                                act_snapshot.position, slot.slot
                            ),
                        });
                    }
                    if let Some(prev_slot) = act_snapshot.last_engaged_slot
                        && prev_slot.bank_id == slot.bank_id
                    {
                        let span = slot.slot.abs_diff(prev_slot.slot);
                        check_span_bounds(spec, span, step_idx)?;
                        if span > state.max_span_slots {
                            state.max_span_slots = span;
                        }
                    }
                    let cur_len = state.slot_stacks.get(slot).map_or(0, Vec::len) as u32;
                    if cur_len < *pop_tokens {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotUnderflow,
                            detail: format!(
                                "slot {slot:?} has {cur_len} tokens, cannot pop {pop_tokens}"
                            ),
                        });
                    }
                    let next_len = cur_len - *pop_tokens + *push_tokens;
                    if next_len > bank.max_occupancy {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::SlotOccupancyOverflow,
                            detail: format!(
                                "slot {slot:?} occupancy {next_len} exceeds cap {}",
                                bank.max_occupancy
                            ),
                        });
                    }
                    let stack = state.slot_stacks.entry(*slot).or_default();
                    for _ in 0..*pop_tokens {
                        let _ = stack.pop();
                    }
                    for _ in 0..*push_tokens {
                        stack.push(state.next_synthetic_token_id);
                        state.next_synthetic_token_id += 1;
                    }
                    let occ = stack.len() as u32;
                    if occ > state.peak_occupancy {
                        state.peak_occupancy = occ;
                    }
                    if let Some(act) = state.actuators.get_mut(actuator_id) {
                        act.last_engaged_slot = Some(*slot);
                    }
                    Ok(())
                }

                ProcessOp::DwellOrPause { duration_s, .. } => {
                    if !duration_s.is_finite() {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::NonFiniteScalar,
                            detail: "DwellOrPause duration_s must be finite".to_string(),
                        });
                    }
                    if *duration_s < 0.0 {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::ScalarBoundViolated,
                            detail: format!("negative dwell duration {duration_s}"),
                        });
                    }
                    for lim in &spec.scalar_limits {
                        if lim.kind == ScalarBoundKind::MinDwellTime
                            && !check_scalar_limit(*duration_s, lim)
                        {
                            return Err(StructuredViolation {
                                step_idx: Some(step_idx),
                                kind: ViolationKind::ScalarBoundViolated,
                                detail: format!(
                                    "dwell {duration_s} violates {} limit {}",
                                    lim.param_name, lim.limit_value
                                ),
                            });
                        }
                    }
                    Ok(())
                }

                ProcessOp::SetScalarParam { name, value } => {
                    if !value.is_finite() {
                        return Err(StructuredViolation {
                            step_idx: Some(step_idx),
                            kind: ViolationKind::NonFiniteScalar,
                            detail: format!("scalar parameter {name} value must be finite"),
                        });
                    }
                    for lim in &spec.scalar_limits {
                        let applies = match lim.kind {
                            ScalarBoundKind::MinFeedRate
                            | ScalarBoundKind::MinDwellTime
                            | ScalarBoundKind::MinNamedParam
                            | ScalarBoundKind::MaxNamedParam => lim.param_name == *name,
                            ScalarBoundKind::MaxRailOffsetTicks | ScalarBoundKind::MaxSpanSlots => {
                                false
                            }
                        };
                        if applies && !check_scalar_limit(*value, lim) {
                            return Err(StructuredViolation {
                                step_idx: Some(step_idx),
                                kind: ViolationKind::ScalarBoundViolated,
                                detail: format!(
                                    "scalar parameter {name}={value} violates {:?} limit {}",
                                    lim.kind, lim.limit_value
                                ),
                            });
                        }
                    }
                    state.scalar_params.insert(name.clone(), *value);
                    Ok(())
                }
            }
        }

        fn finalize(
            &self,
            _spec: &MachineTopologySpec,
            state: &Self::State,
        ) -> Vec<StructuredViolation> {
            let mut violations = Vec::new();
            if self.require_parked_at_finalize {
                for (&act_id, act) in &state.actuators {
                    if act.lifecycle == ActuatorLifecycle::Active {
                        violations.push(StructuredViolation {
                            step_idx: None,
                            kind: ViolationKind::UnparkedActuatorAtFinalize,
                            detail: format!("actuator {act_id} remained Active at program end"),
                        });
                    }
                }
            }
            violations
        }

        fn peak_slot_occupancy(&self, state: &Self::State) -> u32 {
            state.peak_occupancy
        }

        fn max_span_slots(&self, state: &Self::State) -> u32 {
            state.max_span_slots
        }
    }
}

pub mod vm {
    //! Deterministic read-back VM runner and canonical [`DiscreteVmReceipt`].

    use super::{
        AutomatonSemantics, MachineTopologySpec, PROCESS_PROGRAM_SCHEMA_VERSION, ProcessProgram,
        Sha256, SlotRailSemantics, StructuredViolation, ViolationKind,
    };

    /// Canonical read-back verification verdict emitted by the L2 discrete VM
    /// before any L3/L4 continuum physical checks execute.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct DiscreteVmReceipt {
        /// Canonical SHA-256 digest of the verified [`ProcessProgram`].
        pub program_sha256: [u8; 32],
        /// Canonical SHA-256 digest of the [`MachineTopologySpec`].
        pub machine_spec_sha256: [u8; 32],
        /// Canonical SHA-256 digest of this receipt's verdict fields.
        pub receipt_sha256: [u8; 32],
        /// Number of operations attempted/executed before completion.
        pub steps_executed: usize,
        /// Peak single-slot token occupancy observed during execution.
        pub peak_slot_occupancy: u32,
        /// Maximum discrete slot span observed across transfers/steps.
        pub max_span_slots: u32,
        /// All deterministic violations recorded during spec validation, stepping, and finalization.
        pub violations: Vec<StructuredViolation>,
        /// True iff `violations.is_empty()` (discrete structural feasibility only —
        /// never a continuum physics or hardware execution authorization claim).
        pub feasible: bool,
    }

    fn seal_receipt_sha256(
        program_sha256: &[u8; 32],
        machine_spec_sha256: &[u8; 32],
        steps_executed: usize,
        peak_slot_occupancy: u32,
        max_span_slots: u32,
        violations: &[StructuredViolation],
        feasible: bool,
    ) -> [u8; 32] {
        let mut sha = Sha256::new();
        sha.update(b"fs-procir:discrete-vm-receipt:v1");
        sha.update(program_sha256);
        sha.update(machine_spec_sha256);
        sha.feed_u64(steps_executed as u64);
        sha.feed_u32(peak_slot_occupancy);
        sha.feed_u32(max_span_slots);
        sha.update(&[u8::from(feasible)]);
        sha.feed_u64(violations.len() as u64);
        for v in violations {
            match v.step_idx {
                Some(idx) => {
                    sha.update(&[1]);
                    sha.feed_u64(idx as u64);
                }
                None => sha.update(&[0]),
            }
            sha.feed_u16(v.kind.code());
            sha.feed_str(&v.detail);
        }
        sha.finalize()
    }

    /// Execute `program` against `spec` using pluggable [`AutomatonSemantics`] and
    /// emit a deterministic [`DiscreteVmReceipt`].
    #[must_use]
    pub fn run_vm<S: AutomatonSemantics>(
        semantics: &S,
        spec: &MachineTopologySpec,
        program: &ProcessProgram,
    ) -> DiscreteVmReceipt {
        let canonical_spec_hash = spec.canonical_sha256();
        let program_sha256 = program.canonical_sha256();
        let mut violations = Vec::new();

        if program.schema_version != PROCESS_PROGRAM_SCHEMA_VERSION {
            violations.push(StructuredViolation {
                step_idx: None,
                kind: ViolationKind::UnsupportedSchemaVersion,
                detail: format!(
                    "unsupported schema_version {} (expected {PROCESS_PROGRAM_SCHEMA_VERSION})",
                    program.schema_version
                ),
            });
        }
        if spec.spec_sha256 != canonical_spec_hash {
            violations.push(StructuredViolation {
                step_idx: None,
                kind: ViolationKind::SpecHashMismatch,
                detail: "MachineTopologySpec::spec_sha256 does not match canonical digest"
                    .to_string(),
            });
        }
        if program.machine_spec_sha256 != canonical_spec_hash {
            violations.push(StructuredViolation {
                step_idx: None,
                kind: ViolationKind::SpecHashMismatch,
                detail: "ProcessProgram::machine_spec_sha256 does not match target spec digest"
                    .to_string(),
            });
        }

        let mut state = match semantics.init_state(spec) {
            Ok(st) => st,
            Err(init_errs) => {
                violations.extend(init_errs);
                let feasible = violations.is_empty();
                let receipt_sha256 = seal_receipt_sha256(
                    &program_sha256,
                    &canonical_spec_hash,
                    0,
                    0,
                    0,
                    &violations,
                    feasible,
                );
                return DiscreteVmReceipt {
                    program_sha256,
                    machine_spec_sha256: canonical_spec_hash,
                    receipt_sha256,
                    steps_executed: 0,
                    peak_slot_occupancy: 0,
                    max_span_slots: 0,
                    violations,
                    feasible,
                };
            }
        };

        let mut steps_executed = 0usize;
        for (idx, op) in program.ops.iter().enumerate() {
            steps_executed += 1;
            if let Err(v) = semantics.step(spec, &mut state, op, idx) {
                violations.push(v);
            }
        }
        violations.extend(semantics.finalize(spec, &state));

        let peak_slot_occupancy = semantics.peak_slot_occupancy(&state);
        let max_span_slots = semantics.max_span_slots(&state);
        let feasible = violations.is_empty();
        let receipt_sha256 = seal_receipt_sha256(
            &program_sha256,
            &canonical_spec_hash,
            steps_executed,
            peak_slot_occupancy,
            max_span_slots,
            &violations,
            feasible,
        );

        DiscreteVmReceipt {
            program_sha256,
            machine_spec_sha256: canonical_spec_hash,
            receipt_sha256,
            steps_executed,
            peak_slot_occupancy,
            max_span_slots,
            violations,
            feasible,
        }
    }

    /// Convenience entry point running [`run_vm`] with default [`SlotRailSemantics`].
    #[must_use]
    pub fn run_slot_rail_vm(
        spec: &MachineTopologySpec,
        program: &ProcessProgram,
    ) -> DiscreteVmReceipt {
        let sem = SlotRailSemantics::default();
        run_vm(&sem, spec, program)
    }
}

pub mod golden_vectors {
    //! Deterministic golden test vector for `lifted_procir` (`fs-procir::kernel`) using a
    //! 2-head Automated Fiber Placement (AFP) composite layup machine scenario.

    use super::{
        ActuatorLifecycle, ActuatorSpec, Fnv1a64, MachineTopologySpec,
        PROCESS_PROGRAM_SCHEMA_VERSION, ProcessOp, ProcessProgram, RailSpec, ScalarBoundKind,
        ScalarBoundSpec, Sha256, SlotBankSpec, SlotRef, run_slot_rail_vm,
    };

    /// Pinned FNV-1a 64-bit golden digest for [`compute_golden_digests`].
    pub const PROCIR_KERNEL_GOLDEN_FNV64: u64 = 0xf600_d8c9_7e7d_3854;

    /// Pinned SHA-256 golden digest for [`compute_golden_digests`].
    pub const PROCIR_KERNEL_GOLDEN_SHA256: [u8; 32] = [
        0x06, 0x44, 0xc2, 0x59, 0xc5, 0xc0, 0xbe, 0x02, 0x22, 0xd9, 0x26, 0x02, 0x74, 0x19, 0xa2,
        0x98, 0x0a, 0x2b, 0xfa, 0x92, 0x36, 0x73, 0x36, 0xf3, 0x8a, 0xab, 0x82, 0xad, 0x0e, 0xd3,
        0xad, 0x35,
    ];

    /// Build the canonical 2-head Automated Fiber Placement (AFP) machine topology:
    /// - Bank 0 (`creel_supply`): holding/feed bank (slots `0..=100`, cap 2 tows/slot, no `ApplyStep`)
    /// - Bank 1 (`mandrel_layup`): deposition zones (slots `0..=100`, cap 4 plies/slot, `ApplyStep` enabled)
    /// - Rail 0 (`gantry_x`): Head 1 (`id=1`, parked at 0) and Head 2 (`id=2`, parked at 100),
    ///   exclusion clearance `10` slots, discrete indexing offset `[-8..=8]` step `2`
    /// - Scalar limits: `feed_rate_m_s > 0.05`, `dwell_s >= 0.01`, `cut_length_m >= 0.15`, `max_span_slots <= 25`,
    ///   `max_rail_offset_ticks <= 8`.
    #[must_use]
    pub fn build_afp_topology_spec() -> MachineTopologySpec {
        MachineTopologySpec {
            spec_sha256: [0u8; 32],
            slot_banks: vec![
                SlotBankSpec {
                    bank_id: 0,
                    name: "creel_supply".to_string(),
                    min_slot: 0,
                    max_slot: 100,
                    max_occupancy: 2,
                    allows_apply_step: false,
                },
                SlotBankSpec {
                    bank_id: 1,
                    name: "mandrel_layup".to_string(),
                    min_slot: 0,
                    max_slot: 100,
                    max_occupancy: 4,
                    allows_apply_step: true,
                },
            ],
            actuator_rails: vec![RailSpec {
                rail_id: 0,
                name: "gantry_x".to_string(),
                actuators: vec![
                    ActuatorSpec {
                        actuator_id: 1,
                        initial_position: 0,
                        initial_lifecycle: ActuatorLifecycle::Parked,
                    },
                    ActuatorSpec {
                        actuator_id: 2,
                        initial_position: 100,
                        initial_lifecycle: ActuatorLifecycle::Parked,
                    },
                ],
                min_position: 0,
                max_position: 100,
                min_clearance_slots: 10,
                min_offset_ticks: -8,
                max_offset_ticks: 8,
                offset_step_ticks: 2,
            }],
            scalar_limits: vec![
                ScalarBoundSpec {
                    param_name: "feed_rate_m_s".to_string(),
                    kind: ScalarBoundKind::MinFeedRate,
                    limit_value: 0.05,
                    strict: true,
                },
                ScalarBoundSpec {
                    param_name: "dwell_s".to_string(),
                    kind: ScalarBoundKind::MinDwellTime,
                    limit_value: 0.01,
                    strict: false,
                },
                ScalarBoundSpec {
                    param_name: "cut_length_m".to_string(),
                    kind: ScalarBoundKind::MinNamedParam,
                    limit_value: 0.15,
                    strict: false,
                },
                ScalarBoundSpec {
                    param_name: "max_span_slots".to_string(),
                    kind: ScalarBoundKind::MaxSpanSlots,
                    limit_value: 25.0,
                    strict: false,
                },
                ScalarBoundSpec {
                    param_name: "max_rail_offset_ticks".to_string(),
                    kind: ScalarBoundKind::MaxRailOffsetTicks,
                    limit_value: 8.0,
                    strict: false,
                },
            ],
        }
        .sealed()
    }

    /// Build a valid interleaved 2-head AFP composite layup program on `spec`.
    #[must_use]
    pub fn build_valid_afp_program(spec: &MachineTopologySpec) -> ProcessProgram {
        let slot = |bank_id, s| SlotRef { bank_id, slot: s };
        ProcessProgram {
            schema_version: PROCESS_PROGRAM_SCHEMA_VERSION,
            machine_spec_sha256: spec.spec_sha256,
            ops: vec![
                ProcessOp::SetScalarParam {
                    name: "feed_rate_m_s".to_string(),
                    value: 0.45,
                },
                ProcessOp::SetScalarParam {
                    name: "cut_length_m".to_string(),
                    value: 0.25,
                },
                ProcessOp::SetRailOffset {
                    rail_id: 0,
                    offset_ticks: 2,
                },
                ProcessOp::InsertToken {
                    actuator_id: 1,
                    slot: slot(0, 10),
                    token_id: 101,
                },
                ProcessOp::InsertToken {
                    actuator_id: 2,
                    slot: slot(0, 80),
                    token_id: 201,
                },
                ProcessOp::TransferToken {
                    from: slot(0, 10),
                    to: slot(1, 20),
                    count: 1,
                },
                ProcessOp::TransferToken {
                    from: slot(0, 80),
                    to: slot(1, 70),
                    count: 1,
                },
                ProcessOp::MoveActuator {
                    actuator_id: 1,
                    target_position: 20,
                },
                ProcessOp::ApplyStep {
                    actuator_id: 1,
                    slot: slot(1, 20),
                    push_tokens: 1,
                    pop_tokens: 0,
                },
                ProcessOp::MoveActuator {
                    actuator_id: 2,
                    target_position: 70,
                },
                ProcessOp::ApplyStep {
                    actuator_id: 2,
                    slot: slot(1, 70),
                    push_tokens: 1,
                    pop_tokens: 0,
                },
                ProcessOp::DwellOrPause {
                    duration_s: 0.05,
                    reason_code: 1,
                },
                ProcessOp::MoveActuator {
                    actuator_id: 1,
                    target_position: 40,
                },
                ProcessOp::ApplyStep {
                    actuator_id: 1,
                    slot: slot(1, 40),
                    push_tokens: 2,
                    pop_tokens: 0,
                },
                ProcessOp::ParkActuator {
                    actuator_id: 1,
                    park_position: 0,
                },
                ProcessOp::ParkActuator {
                    actuator_id: 2,
                    park_position: 100,
                },
            ],
        }
    }

    /// Evaluate canonical valid and adversarial AFP programs and return `(fnv64, sha256)`.
    #[must_use]
    pub fn compute_golden_digests() -> (u64, [u8; 32]) {
        let spec = build_afp_topology_spec();
        let valid_prog = build_valid_afp_program(&spec);
        let valid_receipt = run_slot_rail_vm(&spec, &valid_prog);

        // Adversarial colliding program.
        let mut bad_prog = valid_prog.clone();
        bad_prog.ops.insert(
            10,
            ProcessOp::MoveActuator {
                actuator_id: 2,
                target_position: 25,
            },
        );
        let bad_receipt = run_slot_rail_vm(&spec, &bad_prog);

        let mut fnv = Fnv1a64::new();
        let mut sha = Sha256::new();
        for receipt in [&valid_receipt, &bad_receipt] {
            fnv.update(&receipt.receipt_sha256);
            sha.update(&receipt.receipt_sha256);
            fnv.update(&[u8::from(receipt.feasible)]);
            sha.update(&[u8::from(receipt.feasible)]);
            fnv.update(&(receipt.steps_executed as u64).to_le_bytes());
            sha.feed_u64(receipt.steps_executed as u64);
            fnv.update(&receipt.peak_slot_occupancy.to_le_bytes());
            sha.feed_u32(receipt.peak_slot_occupancy);
            fnv.update(&receipt.max_span_slots.to_le_bytes());
            sha.feed_u32(receipt.max_span_slots);
        }
        (fnv.finish(), sha.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::golden_vectors::{
        PROCIR_KERNEL_GOLDEN_FNV64, PROCIR_KERNEL_GOLDEN_SHA256, compute_golden_digests,
    };

    #[test]
    fn lifted_procir_golden_digests_match_frankensim() {
        let (fnv64, sha256) = compute_golden_digests();
        assert_eq!(fnv64, PROCIR_KERNEL_GOLDEN_FNV64);
        assert_eq!(sha256, PROCIR_KERNEL_GOLDEN_SHA256);
    }
}

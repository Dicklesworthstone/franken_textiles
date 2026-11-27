//! Versioned knitting stitch-graph IR (`ftx-knit/1`) and lossless CMU Knitout v2 program AST.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::profile::WearerAgeClass;

/// Schema version for knitting pattern documents.
pub const KNIT_SCHEMA: &str = "ftx-knit/1";
/// Schema version for lossless CMU Knitout v2 AST documents.
pub const KNITOUT_SCHEMA: &str = "knitout/2";

/// Canonical yarn carrier identifier (arbitrary non-whitespace ASCII token, e.g. `"1"` or `"A"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CarrierId(pub String);

impl CarrierId {
    /// Validates and constructs a [`CarrierId`].
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if empty or containing whitespace/non-ASCII characters.
    pub fn new(id: impl Into<String>) -> Result<Self, Refusal> {
        let s = id.into();
        if s.is_empty()
            || !s
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b';' && b != b',')
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("invalid Knitout carrier identifier '{s}'"),
            ));
        }
        Ok(Self(s))
    }

    /// Borrow as string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Needle bed selector in CMU Knitout v2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bed {
    /// Front main needle bed (`f`).
    Front,
    /// Back main needle bed (`b`).
    Back,
    /// Front transfer slider bed (`fs`).
    FrontSlider,
    /// Back transfer slider bed (`bs`).
    BackSlider,
}

impl Bed {
    /// Knitout bed prefix (`f`, `b`, `fs`, `bs`).
    #[must_use]
    pub const fn knitout_prefix(self) -> &'static str {
        match self {
            Self::Front => "f",
            Self::Back => "b",
            Self::FrontSlider => "fs",
            Self::BackSlider => "bs",
        }
    }

    /// True if this is a transfer slider bed (`fs` or `bs`).
    #[must_use]
    pub const fn is_slider(self) -> bool {
        matches!(self, Self::FrontSlider | Self::BackSlider)
    }

    /// Opposite main bed (`Front <-> Back`, `FrontSlider <-> BackSlider`).
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Front => Self::Back,
            Self::Back => Self::Front,
            Self::FrontSlider => Self::BackSlider,
            Self::BackSlider => Self::FrontSlider,
        }
    }
}

/// Exact needle address on a V-bed machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Needle {
    /// Target needle bed.
    pub bed: Bed,
    /// Signed integer needle index (`0`-based on standard beds).
    pub index: i32,
}

impl Needle {
    /// Constructs a needle address.
    #[must_use]
    pub const fn new(bed: Bed, index: i32) -> Self {
        Self { bed, index }
    }

    /// Formats as Knitout needle token (e.g. `f12`, `bs-3`).
    #[must_use]
    pub fn knitout_token(self) -> String {
        format!("{}{}", self.bed.knitout_prefix(), self.index)
    }
}

/// Carriage travel direction along the needle bed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CourseDir {
    /// Increasing needle index (`+`).
    Plus,
    /// Decreasing needle index (`-`).
    Minus,
}

impl CourseDir {
    /// Knitout direction symbol (`+` or `-`).
    #[must_use]
    pub const fn knitout_symbol(self) -> &'static str {
        match self {
            Self::Plus => "+",
            Self::Minus => "-",
        }
    }

    /// Reverses carriage direction.
    #[must_use]
    pub const fn reversed(self) -> Self {
        match self {
            Self::Plus => Self::Minus,
            Self::Minus => Self::Plus,
        }
    }
}

/// Topological stitch class in a stitch graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    /// Front-bed knit stitch (v-face toward front).
    Knit,
    /// Back-bed / purl stitch (bump toward front).
    Purl,
    /// Tuck loop held with existing loop(s).
    Tuck,
    /// Float/slip without forming a new loop.
    Slip,
    /// Initial cast-on loop.
    CastOn,
    /// Final bind-off loop.
    BindOff,
    /// Stitch increase (e.g. split / make-one).
    Increase,
    /// Stitch decrease (2+ parent loops merged in hook-stack order).
    Decrease,
    /// Split loop forming a new loop while transferring the old loop.
    Split,
    /// Crossed cable stitch.
    Cable,
}

/// Ordered parent reference preserving hook-stack depth (`0` = bottom/oldest loop on hook).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentRef {
    /// Parent [`StitchNode::id`].
    pub stitch_id: u32,
    /// Hook-stack position (`0` = bottom/oldest, increasing toward top/newest).
    pub stack_pos: u8,
}

/// Dimensionless machine cam / dial stitch setting scoped to a machine family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CamStitchSetting {
    /// Machine-specific dimensionless stitch setting (e.g. `0.0..=15.0` on Kniterate).
    pub setting: f64,
    /// Machine family owning this scale (e.g. `"kniterate_7e"`, `"brother_standard"`, `"hand_mm"`).
    pub machine_family: String,
}

/// One node in a topological knit stitch graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchNode {
    /// Unique stitch node identifier (`0..N`).
    pub id: u32,
    /// Course index (row number from cast-on `0`).
    pub course: u32,
    /// Wale column index (signed horizontal coordinate).
    pub wale: i32,
    /// Primary knitting bed.
    pub bed: Bed,
    /// Loop formation type.
    pub kind: LoopKind,
    /// Ordered parent loops consumed by this stitch (bottom-to-top hook stack).
    #[serde(default)]
    pub parents: Vec<ParentRef>,
    /// Active yarn carrier set knitting this stitch (supports plating/multiple carriers).
    pub carrier_ids: Vec<CarrierId>,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Dimensionless stitch-cam setting for this stitch.
    pub cam_stitch_setting: CamStitchSetting,
    /// Optional explicit yarn arc length consumed by this stitch, in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yarn_length_mm: Option<f64>,
}

/// Explicit short-row turnaround metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortRowTurn {
    /// Course at which carriage reverses before reaching panel edge.
    pub course: u32,
    /// Turnaround wale index.
    pub turn_wale: i32,
    /// Carriage direction immediately after the turn.
    pub dir_after: CourseDir,
    /// Whether an explicit wrap/tuck/German-short-row anchor prevents an eyelet hole.
    pub anchored: bool,
}

/// Explicit cable crossing group (left/right wales exchanged with over/under chirality).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CableGroup {
    /// Course where the cable transfer occurs.
    pub course: u32,
    /// Left wale indices moving right.
    pub left_wales: Vec<i32>,
    /// Right wale indices moving left.
    pub right_wales: Vec<i32>,
    /// True if `left_wales` cross in front of `right_wales`.
    pub left_in_front: bool,
}

/// Contiguous yarn run fed by one carrier (supports multi-piece & intarsia dyeing).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YarnRun {
    /// Unique run identifier.
    pub run_id: u32,
    /// Carrier delivering this yarn run.
    pub carrier_id: CarrierId,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// First course index inclusive.
    pub start_course: u32,
    /// Last course index inclusive.
    pub end_course: u32,
    /// Estimated or calibrated yarn length consumed in this run, in metres.
    pub total_yarn_length_m: f64,
    /// Explicit cut-tail length left on garment interior, in mm (checked for infant safety).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_tail_mm: Option<f64>,
}

/// Measured or target knit gauge specification (units strictly in mm and per-100mm counts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaugeSpec {
    /// Stitches (wales) per 100 mm horizontally.
    pub wales_per_100mm: f64,
    /// Rows (courses) per 100 mm vertically.
    pub courses_per_100mm: f64,
    /// Machine needle pitch in mm (or hand needle spacing reference).
    pub needle_pitch_mm: f64,
    /// Optional yarn linear density in tex (g/km).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yarn_tex: Option<f64>,
    /// Optional relaxed knitted fabric thickness in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
    /// True only if the gauge was measured after washing and blocking relaxation.
    pub relaxed_after_wash: bool,
}

impl GaugeSpec {
    /// Blocked wale pitch (`100.0 / wales_per_100mm`), in mm.
    #[must_use]
    pub fn blocked_wale_pitch_mm(&self) -> f64 {
        100.0 / self.wales_per_100mm
    }

    /// Blocked course height (`100.0 / courses_per_100mm`), in mm.
    #[must_use]
    pub fn blocked_course_height_mm(&self) -> f64 {
        100.0 / self.courses_per_100mm
    }

    /// Validates positivity and finiteness of all gauge dimensions.
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] or [`RefusalCode::OutOfRange`].
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, val) in [
            ("wales_per_100mm", self.wales_per_100mm),
            ("courses_per_100mm", self.courses_per_100mm),
            ("needle_pitch_mm", self.needle_pitch_mm),
        ] {
            Refusal::finite(name, val)?;
            if val <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{name} must be strictly positive (got {val})"),
                ));
            }
        }
        if let Some(tex) = self.yarn_tex {
            Refusal::finite("yarn_tex", tex)?;
            if tex <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("yarn_tex must be strictly positive (got {tex})"),
                ));
            }
        }
        if let Some(t) = self.thickness_mm {
            Refusal::finite("thickness_mm", t)?;
            if t <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("thickness_mm must be strictly positive (got {t})"),
                ));
            }
        }
        Ok(())
    }
}

/// Complete `ftx-knit/1` knitting pattern document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitDoc {
    /// Must equal [`KNIT_SCHEMA`] (`"ftx-knit/1"`).
    pub schema: String,
    /// Pattern/garment name.
    pub name: String,
    /// Required, non-defaulting wearer age classification (governs child-safety gates).
    pub wearer_age_class: WearerAgeClass,
    /// Gauge specification for this document.
    pub gauge: GaugeSpec,
    /// Topological stitch nodes.
    pub stitches: Vec<StitchNode>,
    /// Explicit short-row turnaround records.
    #[serde(default)]
    pub short_row_turns: Vec<ShortRowTurn>,
    /// Explicit cable crossing groups.
    #[serde(default)]
    pub cable_groups: Vec<CableGroup>,
    /// Contiguous yarn feed runs.
    #[serde(default)]
    pub yarn_runs: Vec<YarnRun>,
    /// True if garment design includes any cord/drawstring (refused for infant/child neck/hood).
    #[serde(default)]
    pub has_hood_or_neck_drawstring: bool,
    /// Structured metadata (deterministic `BTreeMap` order).
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl KnitDoc {
    /// Structural validation of schema tag, gauge, unique stitch IDs, DAG parent ordering, and carrier IDs.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`], [`RefusalCode::OutOfRange`], or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != KNIT_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("expected schema '{KNIT_SCHEMA}', got '{}'", self.schema),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "knit document name must be non-empty",
            ));
        }
        self.gauge.validate()?;
        if self.stitches.is_empty() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "knit document contains zero stitches",
            ));
        }

        let mut seen_ids = BTreeSet::new();
        let mut course_of = BTreeMap::new();
        for node in &self.stitches {
            if !seen_ids.insert(node.id) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("duplicate stitch id {}", node.id),
                ));
            }
            if node.bed.is_slider()
                && matches!(
                    node.kind,
                    LoopKind::Knit | LoopKind::Purl | LoopKind::Tuck | LoopKind::Split
                )
            {
                return Err(Refusal::new(
                    RefusalCode::MachineStateInfeasible,
                    format!(
                        "stitch {} attempts {:?} on slider bed {:?}",
                        node.id, node.kind, node.bed
                    ),
                ));
            }
            if node.carrier_ids.is_empty() && !matches!(node.kind, LoopKind::Slip) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("stitch {} must declare at least one carrier", node.id),
                ));
            }
            for cid in &node.carrier_ids {
                CarrierId::new(cid.as_str())?;
            }
            Refusal::finite("cam_stitch_setting", node.cam_stitch_setting.setting)?;
            if let Some(len) = node.yarn_length_mm {
                Refusal::finite("yarn_length_mm", len)?;
                if len < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("stitch {} yarn_length_mm cannot be negative", node.id),
                    ));
                }
            }
            course_of.insert(node.id, node.course);
        }

        // Verify all parent references exist and come from strictly earlier courses (acyclic DAG)
        for node in &self.stitches {
            let mut stack_positions = BTreeSet::new();
            for p in &node.parents {
                let Some(&parent_course) = course_of.get(&p.stitch_id) else {
                    return Err(Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!(
                            "stitch {} references nonexistent parent stitch {}",
                            node.id, p.stitch_id
                        ),
                    ));
                };
                if p.stitch_id == node.id
                    || parent_course > node.course
                    || (parent_course == node.course && !matches!(node.kind, LoopKind::BindOff))
                {
                    return Err(Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!(
                            "stitch {} (course {}) has non-causal parent {} (course {})",
                            node.id, node.course, p.stitch_id, parent_course
                        ),
                    ));
                }
                if !stack_positions.insert(p.stack_pos) {
                    return Err(Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!(
                            "stitch {} has duplicate parent stack_pos {}",
                            node.id, p.stack_pos
                        ),
                    ));
                }
            }
        }
        let mut seen_runs = BTreeSet::new();
        for run in &self.yarn_runs {
            if !seen_runs.insert(run.run_id) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("duplicate yarn run_id {}", run.run_id),
                ));
            }
            CarrierId::new(run.carrier_id.as_str())?;
            if run.start_course > run.end_course {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "yarn run {} start_course {} exceeds end_course {}",
                        run.run_id, run.start_course, run.end_course
                    ),
                ));
            }
            Refusal::finite("total_yarn_length_m", run.total_yarn_length_m)?;
            if run.total_yarn_length_m < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "yarn run {} total_yarn_length_m cannot be negative",
                        run.run_id
                    ),
                ));
            }
            if let Some(tail) = run.cut_tail_mm {
                Refusal::finite("cut_tail_mm", tail)?;
                if tail < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("yarn run {} cut_tail_mm cannot be negative", run.run_id),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Canonical SHA-256 hash of this document.
    ///
    /// # Errors
    /// Same as [`canonical_json_sha256`].
    pub fn canonical_sha256(&self) -> Result<String, Refusal> {
        canonical_json_sha256(self)
    }
}

/// Standard CMU Knitout v2 header metadata (`;;Key: Value`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitoutHeader {
    /// Target machine identifier (`;;Machine:`).
    pub machine: String,
    /// Machine gauge string (`;;Gauge:`), e.g. `"7"`.
    pub gauge: String,
    /// Machine needle bed width (`;;Width:`), e.g. `252`.
    pub width: u32,
    /// Positioning convention (`;;Position:`), e.g. `"Center"`, `"Left"`, `"Keep"`.
    pub position: String,
    /// Ordered carrier list (`;;Carriers:`).
    pub carriers: Vec<CarrierId>,
    /// Optional additional `;;Key: Value` headers in sorted order.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

/// Lossless CMU Knitout v2 operation AST.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KnitoutOp {
    /// Bring carrier(s) into action directly (`in cs`).
    In {
        /// Carrier set brought into action.
        carriers: Vec<CarrierId>,
    },
    /// Take carrier(s) out of action directly (`out cs`).
    Out {
        /// Carrier set parked out of action.
        carriers: Vec<CarrierId>,
    },
    /// Bring carrier(s) in via yarn-inserting hook (`inhook cs`).
    InHook {
        /// Carrier set brought in on the yarn-inserting hook.
        carriers: Vec<CarrierId>,
    },
    /// Release yarn-inserting hook (`releasehook cs`).
    ReleaseHook {
        /// Carrier set released from the yarn-inserting hook.
        carriers: Vec<CarrierId>,
    },
    /// Take carrier(s) out via yarn-inserting hook (`outhook cs`).
    OutHook {
        /// Carrier set taken out via the yarn-inserting hook.
        carriers: Vec<CarrierId>,
    },
    /// Set leading/trailing stitch numbers (`stitch L T`).
    Stitch {
        /// Leading stitch-length dial value.
        leading: i32,
        /// Trailing stitch-length dial value.
        trailing: i32,
    },
    /// Rack back bed relative to front bed in exact integer quarter-pitches (`rack R`).
    /// For example, `+4` emits `rack 1`, `-2` emits `rack -0.5`, `+1` emits `rack 0.25`.
    Rack {
        /// Signed bed racking offset in exact quarter-pitches.
        quarter_pitches: i32,
    },
    /// Knit stitch (`knit D N cs`).
    Knit {
        /// Carriage travel direction.
        dir: CourseDir,
        /// Target needle address.
        needle: Needle,
        /// Active carrier set feeding the stitch.
        carriers: Vec<CarrierId>,
    },
    /// Tuck loop (`tuck D N cs`).
    Tuck {
        /// Carriage travel direction.
        dir: CourseDir,
        /// Target needle address.
        needle: Needle,
        /// Active carrier set feeding the tuck.
        carriers: Vec<CarrierId>,
    },
    /// Split stitch transferring old loop to `to` while forming new loop on `from` (`split D N1 N2 cs`).
    Split {
        /// Carriage travel direction.
        dir: CourseDir,
        /// Source needle forming the new loop.
        from: Needle,
        /// Destination needle receiving the old loop.
        to: Needle,
        /// Active carrier set feeding the split.
        carriers: Vec<CarrierId>,
    },
    /// Transfer loop(s) (`xfer N1 N2`).
    Xfer {
        /// Source needle losing its loop stack.
        from: Needle,
        /// Destination needle receiving the loop stack.
        to: Needle,
    },
    /// Move carrier(s) without knitting (`miss D N cs`).
    Miss {
        /// Carriage travel direction.
        dir: CourseDir,
        /// Target needle position.
        needle: Needle,
        /// Active carrier set carried past the needle.
        carriers: Vec<CarrierId>,
    },
    /// Empty-carrier needle pass / tuck-release (`amiss N`).
    Amiss {
        /// Target needle address.
        needle: Needle,
    },
    /// Drop loop(s) off needle (`drop N`).
    Drop {
        /// Target needle address to cast off.
        needle: Needle,
    },
    /// Standard operator pause (`pause ; <note>`).
    Pause {
        /// Optional operator note rendered after `; `.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Non-executable comment line (`; <text>`).
    Comment {
        /// Single-line comment text.
        text: String,
    },
    /// Extension: relative roller advance (`x-add-roller-advance N`).
    AddRollerAdvance {
        /// Signed roller-advance units.
        units: i32,
    },
    /// Extension: carriage speed setting (`x-speed-number N`).
    SpeedNumber {
        /// Machine speed index.
        speed: u32,
    },
    /// Extension: presser foot mode (`x-presser-mode M`).
    PresserMode {
        /// Presser mode keyword (`auto`, `on`, `off`).
        mode: String,
    },
    /// Extension: minimum carrier spacing (`x-carrier-spacing N`).
    CarrierSpacing {
        /// Minimum needle gap enforced between carriers.
        needles: u32,
    },
}

/// Complete lossless CMU Knitout v2 program.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitoutProgram {
    /// Must equal [`KNITOUT_SCHEMA`] (`"knitout/2"`).
    pub schema: String,
    /// Knitout v2 `;;` headers.
    pub header: KnitoutHeader,
    /// Ordered Knitout operations.
    pub ops: Vec<KnitoutOp>,
}

impl KnitoutProgram {
    /// Renders canonical CMU Knitout v2 text (`;!knitout-2` magic header + `;;` headers + ops).
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if a comment/pause note contains raw newlines.
    pub fn to_knitout_text(&self) -> Result<String, Refusal> {
        if self.schema != KNITOUT_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("expected schema '{KNITOUT_SCHEMA}', got '{}'", self.schema),
            ));
        }
        let check_line = |label: &str, value: &str| -> Result<(), Refusal> {
            if value.contains(['\n', '\r']) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("Knitout {label} cannot contain line breaks"),
                ));
            }
            Ok(())
        };
        check_line("header machine", &self.header.machine)?;
        check_line("header gauge", &self.header.gauge)?;
        check_line("header position", &self.header.position)?;
        let carriers_line = format_carriers(&self.header.carriers, false)?;

        let mut out = String::new();
        out.push_str(";!knitout-2\n");
        let _ = writeln!(out, ";;Machine: {}", self.header.machine);
        let _ = writeln!(out, ";;Gauge: {}", self.header.gauge);
        let _ = writeln!(out, ";;Width: {}", self.header.width);
        let _ = writeln!(out, ";;Position: {}", self.header.position);
        let _ = writeln!(out, ";;Carriers: {carriers_line}");
        for (k, v) in &self.header.extra {
            check_line("header key", k)?;
            check_line("header value", v)?;
            let _ = writeln!(out, ";;{k}: {v}");
        }
        for op in &self.ops {
            match op {
                KnitoutOp::In { carriers } => {
                    let cs = format_carriers(carriers, true)?;
                    let _ = writeln!(out, "in {cs}");
                }
                KnitoutOp::Out { carriers } => {
                    let cs = format_carriers(carriers, true)?;
                    let _ = writeln!(out, "out {cs}");
                }
                KnitoutOp::InHook { carriers } => {
                    let cs = format_carriers(carriers, true)?;
                    let _ = writeln!(out, "inhook {cs}");
                }
                KnitoutOp::ReleaseHook { carriers } => {
                    let cs = format_carriers(carriers, true)?;
                    let _ = writeln!(out, "releasehook {cs}");
                }
                KnitoutOp::OutHook { carriers } => {
                    let cs = format_carriers(carriers, true)?;
                    let _ = writeln!(out, "outhook {cs}");
                }
                KnitoutOp::Stitch { leading, trailing } => {
                    let _ = writeln!(out, "stitch {leading} {trailing}");
                }
                KnitoutOp::Rack { quarter_pitches } => {
                    let _ = writeln!(out, "rack {}", format_quarter_pitches(*quarter_pitches));
                }
                KnitoutOp::Knit {
                    dir,
                    needle,
                    carriers,
                } => {
                    let cs = format_carriers(carriers, false)?;
                    if cs.is_empty() {
                        let _ = writeln!(
                            out,
                            "knit {} {}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "knit {} {} {cs}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    }
                }
                KnitoutOp::Tuck {
                    dir,
                    needle,
                    carriers,
                } => {
                    let cs = format_carriers(carriers, false)?;
                    if cs.is_empty() {
                        let _ = writeln!(
                            out,
                            "tuck {} {}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "tuck {} {} {cs}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    }
                }
                KnitoutOp::Split {
                    dir,
                    from,
                    to,
                    carriers,
                } => {
                    let cs = format_carriers(carriers, false)?;
                    if cs.is_empty() {
                        let _ = writeln!(
                            out,
                            "split {} {} {}",
                            dir.knitout_symbol(),
                            from.knitout_token(),
                            to.knitout_token()
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "split {} {} {} {cs}",
                            dir.knitout_symbol(),
                            from.knitout_token(),
                            to.knitout_token()
                        );
                    }
                }
                KnitoutOp::Xfer { from, to } => {
                    let _ = writeln!(out, "xfer {} {}", from.knitout_token(), to.knitout_token());
                }
                KnitoutOp::Miss {
                    dir,
                    needle,
                    carriers,
                } => {
                    let cs = format_carriers(carriers, false)?;
                    if cs.is_empty() {
                        let _ = writeln!(
                            out,
                            "miss {} {}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    } else {
                        let _ = writeln!(
                            out,
                            "miss {} {} {cs}",
                            dir.knitout_symbol(),
                            needle.knitout_token()
                        );
                    }
                }
                KnitoutOp::Amiss { needle } => {
                    let _ = writeln!(out, "amiss {}", needle.knitout_token());
                }
                KnitoutOp::Drop { needle } => {
                    let _ = writeln!(out, "drop {}", needle.knitout_token());
                }
                KnitoutOp::Pause { note } => match note {
                    Some(n) if !n.is_empty() => {
                        check_line("pause note", n)?;
                        let _ = writeln!(out, "pause ; {n}");
                    }
                    _ => out.push_str("pause\n"),
                },
                KnitoutOp::Comment { text } => {
                    check_line("comment", text)?;
                    let _ = writeln!(out, "; {text}");
                }
                KnitoutOp::AddRollerAdvance { units } => {
                    let _ = writeln!(out, "x-add-roller-advance {units}");
                }
                KnitoutOp::SpeedNumber { speed } => {
                    let _ = writeln!(out, "x-speed-number {speed}");
                }
                KnitoutOp::PresserMode { mode } => {
                    if mode.is_empty() || mode.bytes().any(|b| !b.is_ascii_graphic() || b == b';') {
                        return Err(Refusal::new(
                            RefusalCode::InvalidDocument,
                            format!("invalid Knitout presser mode '{mode}'"),
                        ));
                    }
                    let _ = writeln!(out, "x-presser-mode {mode}");
                }
                KnitoutOp::CarrierSpacing { needles } => {
                    let _ = writeln!(out, "x-carrier-spacing {needles}");
                }
            }
        }
        Ok(out)
    }

    /// Canonical SHA-256 of the rendered Knitout text.
    ///
    /// # Errors
    /// Same as [`Self::to_knitout_text`].
    pub fn canonical_text_sha256(&self) -> Result<String, Refusal> {
        let txt = self.to_knitout_text()?;
        Ok(ftx_core::receipt::sha256_hex(txt.as_bytes()))
    }
}

fn format_carriers(carriers: &[CarrierId], require_non_empty: bool) -> Result<String, Refusal> {
    if require_non_empty && carriers.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "Knitout carrier operation requires at least one carrier",
        ));
    }
    for cid in carriers {
        CarrierId::new(cid.as_str())?;
    }
    Ok(carriers
        .iter()
        .map(CarrierId::as_str)
        .collect::<Vec<_>>()
        .join(" "))
}

/// Formats integer quarter-pitches cleanly as Knitout decimal rack tokens (`0`, `1`, `-0.5`, `0.25`).
#[must_use]
pub fn format_quarter_pitches(quarter_pitches: i32) -> String {
    let sign = if quarter_pitches < 0 { "-" } else { "" };
    let abs = quarter_pitches.unsigned_abs();
    let whole = abs / 4;
    match abs % 4 {
        0 => format!("{sign}{whole}"),
        1 => format!("{sign}{whole}.25"),
        2 => format!("{sign}{whole}.5"),
        3 => format!("{sign}{whole}.75"),
        _ => unreachable!(),
    }
}

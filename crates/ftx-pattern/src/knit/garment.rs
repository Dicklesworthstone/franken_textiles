//! Four separately knitted pieces for a manually seamed drop-shoulder pullover.
//!
//! Each piece remains an ordinary ftx-knit/1 document. This module describes the
//! actual joining edges, including stitch-to-row joins, without claiming a seamless
//! machine program, a fitted garment, or a physically qualified neck finish.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, det};
use serde::{Deserialize, Serialize};

use super::draft::{ParametricPanelSpec, draft_shaped_panel};
use super::ir::{Bed, CarrierId, GaugeSpec, KnitDoc, LoopKind, StitchNode};
use super::profile::{MachineProfile, WearerAgeClass};

/// Version of the complete, manually assembled pullover document.
pub const SEAMED_PULLOVER_SCHEMA: &str = "ftx-knit-seamed-pullover/1";
const MAX_GARMENT_STITCHES: u64 = 500_000;
const MAX_COURSES: u32 = 5_000;

/// Explicit finished dimensions, including the caller's chosen wearing ease.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamedPulloverSpec {
    /// Garment name; the four piece names are derived from this name.
    pub name: String,
    /// Required wearer classification; a pullover cannot be a calibration swatch.
    pub wearer_age_class: WearerAgeClass,
    /// Finished chest circumference, including ease.
    pub chest_girth_mm: f64,
    /// Finished hem circumference, including ease.
    pub hem_girth_mm: f64,
    /// Hem-to-shoulder length of both body panels.
    pub body_length_mm: f64,
    /// Unsewn vertical opening on each front/back side, measured from the shoulder.
    pub armhole_depth_mm: f64,
    /// Cuff-to-sleeve-head length, excluding any additional neckband or trim.
    pub sleeve_length_mm: f64,
    /// Finished cuff circumference, including ease.
    pub cuff_girth_mm: f64,
    /// Width left open between the two shoulder seams.
    pub neck_width_mm: f64,
    /// Front neck drop; zero leaves a straight, bound center edge unsewn.
    pub front_neck_depth_mm: f64,
    /// Back neck drop; zero leaves a straight, bound center edge unsewn.
    pub back_neck_depth_mm: f64,
    /// Number of initial rib courses on each body panel.
    pub body_rib_courses: u32,
    /// Number of initial rib courses on each sleeve.
    pub cuff_rib_courses: u32,
    /// Dimensionless cam setting for the stated yarn and machine.
    pub cam_setting: f64,
    /// Catalog yarn identifier shared by all four pieces.
    pub yarn_id: String,
    /// Primary carrier; an open neck may require a second carrier.
    pub carrier_id: CarrierId,
    /// Caller-authorized absolute mismatch for each individual manual seam.
    pub max_seam_length_mismatch_mm: f64,
    /// Existing child drawstring rules apply to every piece.
    #[serde(default)]
    pub has_hood_or_neck_drawstring: bool,
}

/// Piece names are stable directory names in a complete garment bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulloverPieceRole {
    /// Front bodice.
    Front,
    /// Back bodice.
    Back,
    /// Wearer's left sleeve.
    LeftSleeve,
    /// Wearer's right sleeve.
    RightSleeve,
}

impl PulloverPieceRole {
    /// Stable spelling for file names and construction instructions.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::LeftSleeve => "left_sleeve",
            Self::RightSleeve => "right_sleeve",
        }
    }
}

/// One ordinary, independently compiled knitting document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PulloverPiece {
    /// Role in the assembled garment.
    pub role: PulloverPieceRole,
    /// Source graph; stitch identifiers are local to this document.
    pub doc: KnitDoc,
}

/// Which face of the source stitch supplies a construction edge.
///
/// Left/right here always mean increasing-chart-coordinate left/right, not wearer
/// left/right. A corner stitch can supply both a horizontal and a vertical edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GarmentEdgeKind {
    /// Horizontal cast-on edge.
    CastOn,
    /// Horizontal, chain-bound terminal edge.
    BindOff,
    /// Horizontal center loops awaiting manual neckline completion.
    HeldNeck,
    /// Outer chart-left edge, one edge unit per formed course.
    LeftSelvedge,
    /// Outer chart-right edge, one edge unit per formed course.
    RightSelvedge,
    /// Inner edge of the chart-left shoulder.
    LeftNeckline,
    /// Inner edge of the chart-right shoulder.
    RightNeckline,
}

/// An ordered, explicitly identified source boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GarmentEdge {
    /// Piece containing these stitch IDs.
    pub piece: PulloverPieceRole,
    /// Horizontal/vertical source edge identity.
    pub kind: GarmentEdgeKind,
    /// IDs in joining order; reverse order is represented explicitly in this list.
    pub stitch_ids: Vec<u32>,
    /// Gauge-plane polyline length, including lateral movement along tapered edges.
    pub length_mm: f64,
}

/// One evenly distributed stitch/row grouping in a manual seam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamJoinGroup {
    /// Half-open index range into seam edge a.stitch_ids.
    pub a_indices: [u32; 2],
    /// Half-open index range into seam edge b.stitch_ids.
    pub b_indices: [u32; 2],
}

/// A manual seam, with no missing or silently duplicated stitch/row edge units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblySeam {
    /// Stable construction step name.
    pub name: String,
    /// First edge, in joining order.
    pub a: GarmentEdge,
    /// Second edge, in matching joining order.
    pub b: GarmentEdge,
    /// Nonempty groups partitioning both ordered edge lists exactly once.
    pub groups: Vec<SeamJoinGroup>,
    /// Signed gauge-plane mismatch: a.length_mm - b.length_mm.
    pub length_mismatch_mm: f64,
}

/// Boundary that remains open after all ten seams have been completed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GarmentOpening {
    /// Neck, hem, left cuff, or right cuff.
    pub name: String,
    /// Ordered boundary sections, including held loops when applicable.
    pub edges: Vec<GarmentEdge>,
    /// Sum of the source boundary lengths; this is not a stretch/fit measurement.
    pub length_mm: f64,
}

/// Four piece graphs and the complete manual construction plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamedPulloverDraft {
    /// Must equal SEAMED_PULLOVER_SCHEMA.
    pub schema: String,
    /// Original finished-dimension targets and explicit seam tolerance.
    pub spec: SeamedPulloverSpec,
    /// Front, back, left sleeve, right sleeve, in that stable order.
    pub pieces: Vec<PulloverPiece>,
    /// Two shoulders, two body sides, four sleeve-head halves, two sleeve underarms.
    pub seams: Vec<AssemblySeam>,
    /// The neckline, hem, and two cuffs remain open.
    pub openings: Vec<GarmentOpening>,
    /// Actual gauge-plane dimension minus its requested value, in millimetres.
    pub residuals_mm: BTreeMap<String, f64>,
    /// Construction order and manual finishing obligations.
    pub instructions: Vec<String>,
}

impl SeamedPulloverDraft {
    /// Reconstructs the assembly from the actual piece graphs and checks every stored
    /// edge, pairing, residual and instruction. Machine qualification remains per piece.
    ///
    /// # Errors
    /// Refuses malformed pieces, stale/tampered assembly data, or excessive seam mismatch.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != SEAMED_PULLOVER_SCHEMA {
            return Err(invalid("unsupported seamed pullover schema"));
        }
        let assembly = assemble(&self.spec, &self.pieces)?;
        if self.seams != assembly.seams
            || self.openings != assembly.openings
            || self.residuals_mm != assembly.residuals_mm
            || self.instructions != assembly.instructions
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "garment assembly does not match its source piece graphs and specification",
            ));
        }
        Ok(())
    }
}

struct Counts {
    body_rows: u32,
    sleeve_rows: u32,
    arm_rows: u32,
    hem_wales: u32,
    chest_wales: u32,
    cuff_wales: u32,
    head_wales: u32,
    neck_half: u32,
    front_neck_rows: u32,
    back_neck_rows: u32,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn bounded_round(value: f64, min: u32, max: u32, label: &str) -> Result<u32, Refusal> {
    Refusal::finite(label, value)?;
    let rounded = value.round();
    if rounded < f64::from(min) || rounded > f64::from(max) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("{label} quantizes to {rounded}, outside {min}..={max}"),
        ));
    }
    Ok(rounded as u32)
}

fn even_wales(mm: f64, pitch: f64, cap: u32, label: &str) -> Result<u32, Refusal> {
    let count = bounded_round(mm / pitch, 4, cap, label)?;
    let even = count + count % 2;
    if even > cap {
        return Err(Refusal::new(
            RefusalCode::MachineStateInfeasible,
            format!("{label} needs {even} symmetric needles, exceeding {cap}"),
        ));
    }
    Ok(even)
}

fn counts(
    spec: &SeamedPulloverSpec,
    gauge: &GaugeSpec,
    needle_cap: u32,
) -> Result<Counts, Refusal> {
    gauge.validate()?;
    let needle_cap = needle_cap.min(MAX_GARMENT_STITCHES as u32);
    let pitch = gauge.blocked_wale_pitch_mm();
    let height = gauge.blocked_course_height_mm();
    Refusal::finite("blocked wale pitch", pitch)?;
    Refusal::finite("blocked course height", height)?;
    if pitch <= 0.0 || height <= 0.0 {
        return Err(invalid("gauge must resolve to positive finite dimensions"));
    }
    if spec.name.trim().is_empty() || spec.yarn_id.trim().is_empty() {
        return Err(invalid("pullover name and yarn_id must be nonempty"));
    }
    CarrierId::new(spec.carrier_id.as_str())?;
    if spec.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "a four-piece wearable pullover cannot use calibration-swatch exemptions",
        ));
    }
    if spec.has_hood_or_neck_drawstring && spec.wearer_age_class.is_child() {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "child pullovers cannot include a hood or neck drawstring",
        ));
    }
    for (label, value) in [
        ("chest_girth_mm", spec.chest_girth_mm),
        ("hem_girth_mm", spec.hem_girth_mm),
        ("body_length_mm", spec.body_length_mm),
        ("armhole_depth_mm", spec.armhole_depth_mm),
        ("sleeve_length_mm", spec.sleeve_length_mm),
        ("cuff_girth_mm", spec.cuff_girth_mm),
        ("neck_width_mm", spec.neck_width_mm),
    ] {
        Refusal::finite(label, value)?;
        if value <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("{label} must be positive"),
            ));
        }
    }
    for (label, value) in [
        ("front_neck_depth_mm", spec.front_neck_depth_mm),
        ("back_neck_depth_mm", spec.back_neck_depth_mm),
        (
            "max_seam_length_mismatch_mm",
            spec.max_seam_length_mismatch_mm,
        ),
    ] {
        Refusal::finite(label, value)?;
        if value < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("{label} cannot be negative"),
            ));
        }
    }
    Refusal::finite("cam_setting", spec.cam_setting)?;
    let body_rows = bounded_round(spec.body_length_mm / height, 6, MAX_COURSES, "body courses")?;
    let sleeve_rows = bounded_round(
        spec.sleeve_length_mm / height,
        6,
        MAX_COURSES,
        "sleeve courses",
    )?;
    let arm_rows = bounded_round(
        spec.armhole_depth_mm / height,
        1,
        body_rows - 1,
        "armhole courses",
    )?;
    let neck_rows = |depth: f64, label: &str| {
        if depth == 0.0 {
            Ok(0)
        } else {
            bounded_round(depth / height, 1, arm_rows, label)
        }
    };
    let hem_wales = even_wales(spec.hem_girth_mm * 0.5, pitch, needle_cap, "body hem")?;
    let chest_wales = even_wales(spec.chest_girth_mm * 0.5, pitch, needle_cap, "body chest")?;
    let cuff_wales = even_wales(spec.cuff_girth_mm, pitch, needle_cap, "sleeve cuff")?;
    let head_wales = even_wales(
        2.0 * f64::from(arm_rows) * height,
        pitch,
        needle_cap,
        "sleeve head",
    )?;
    let neck_half = bounded_round(
        spec.neck_width_mm * 0.5 / pitch,
        1,
        chest_wales / 2 - 1,
        "half neckline",
    )?;
    if cuff_wales > head_wales {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "this drop-shoulder sleeve requires a cuff no wider than its derived sleeve head",
        ));
    }
    if spec.body_rib_courses.saturating_add(2) > body_rows
        || spec.cuff_rib_courses.saturating_add(2) > sleeve_rows
        || spec.body_rib_courses >= body_rows - arm_rows
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "rib courses must leave body shaping below the armholes and sleeve shaping above the cuff",
        ));
    }
    let estimate = 2 * u64::from(body_rows + 1) * u64::from(hem_wales.max(chest_wales))
        + 2 * u64::from(sleeve_rows + 1) * u64::from(head_wales);
    if estimate > MAX_GARMENT_STITCHES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("four-piece garment exceeds the {MAX_GARMENT_STITCHES} stitch budget"),
        ));
    }
    Ok(Counts {
        body_rows,
        sleeve_rows,
        arm_rows,
        hem_wales,
        chest_wales,
        cuff_wales,
        head_wales,
        neck_half,
        front_neck_rows: neck_rows(spec.front_neck_depth_mm, "front neck courses")?,
        back_neck_rows: neck_rows(spec.back_neck_depth_mm, "back neck courses")?,
    })
}

/// Drafts four independently knitted pieces and a complete manual joining plan.
///
/// Sleeve-head width derives from the quantized front-plus-back armhole height.
/// All dimensions include the caller's chosen ease. A positive neck drop retains
/// the existing held-loop/manual-finish construction and its machine holding limits.
///
/// # Errors
/// Refuses impossible dimensions, exhausted budgets, excessive seam mismatch, or
/// any refusal from the existing panel drafter. This does not qualify machine output.
pub fn draft_seamed_pullover(
    spec: &SeamedPulloverSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
) -> Result<SeamedPulloverDraft, Refusal> {
    profile.validate()?;
    let n = counts(spec, gauge, profile.needle_count)?;
    let pitch = gauge.blocked_wale_pitch_mm();
    let mut pieces = Vec::with_capacity(4);
    for role in [
        PulloverPieceRole::Front,
        PulloverPieceRole::Back,
        PulloverPieceRole::LeftSleeve,
        PulloverPieceRole::RightSleeve,
    ] {
        let body = matches!(role, PulloverPieceRole::Front | PulloverPieceRole::Back);
        let neck_depth = match role {
            PulloverPieceRole::Front => spec.front_neck_depth_mm,
            PulloverPieceRole::Back => spec.back_neck_depth_mm,
            PulloverPieceRole::LeftSleeve | PulloverPieceRole::RightSleeve => 0.0,
        };
        let panel = ParametricPanelSpec {
            name: format!("{}-{}", spec.name, role.as_str()),
            wearer_age_class: spec.wearer_age_class,
            hem_width_mm: if body {
                spec.hem_girth_mm * 0.5
            } else {
                spec.cuff_girth_mm
            },
            chest_width_mm: if body {
                spec.chest_girth_mm * 0.5
            } else {
                f64::from(n.head_wales) * pitch
            },
            length_mm: if body {
                spec.body_length_mm
            } else {
                spec.sleeve_length_mm
            },
            armhole_depth_mm: if body { spec.armhole_depth_mm } else { 0.0 },
            armhole_inset_total_mm: 0.0,
            neck_width_mm: if neck_depth > 0.0 {
                spec.neck_width_mm
            } else {
                0.0
            },
            neck_depth_mm: neck_depth,
            rib_hem_courses: if body {
                spec.body_rib_courses
            } else {
                spec.cuff_rib_courses
            },
            cam_setting: spec.cam_setting,
            yarn_id: spec.yarn_id.clone(),
            carrier_id: spec.carrier_id.clone(),
            has_hood_or_neck_drawstring: spec.has_hood_or_neck_drawstring,
        };
        let mut doc = draft_shaped_panel(&panel, gauge, profile)?;
        doc.metadata.insert(
            "garment_construction".to_string(),
            "four_piece_seamed_drop_shoulder".to_string(),
        );
        doc.metadata
            .insert("garment_piece_role".to_string(), role.as_str().to_string());
        pieces.push(PulloverPiece { role, doc });
    }
    let assembly = assemble(spec, &pieces)?;
    Ok(SeamedPulloverDraft {
        schema: SEAMED_PULLOVER_SCHEMA.to_string(),
        spec: spec.clone(),
        pieces,
        seams: assembly.seams,
        openings: assembly.openings,
        residuals_mm: assembly.residuals_mm,
        instructions: assembly.instructions,
    })
}

struct Boundary<'a> {
    role: PulloverPieceRole,
    rows: Vec<Vec<&'a StitchNode>>,
    pitch: f64,
    height: f64,
    neck_half: i32,
    neck_rows: u32,
}

impl<'a> Boundary<'a> {
    fn new(
        piece: &'a PulloverPiece,
        spec: &SeamedPulloverSpec,
        n: &Counts,
        gauge: &GaugeSpec,
    ) -> Result<Self, Refusal> {
        let doc = &piece.doc;
        doc.validate()?;
        if doc.gauge != *gauge
            || doc.wearer_age_class != spec.wearer_age_class
            || doc.has_hood_or_neck_drawstring != spec.has_hood_or_neck_drawstring
            || doc.name != format!("{}-{}", spec.name, piece.role.as_str())
            || !doc.short_row_turns.is_empty()
            || !doc.cable_groups.is_empty()
        {
            return Err(invalid(
                "pullover piece identity, gauge, or construction differs from the specification",
            ));
        }
        let body = matches!(
            piece.role,
            PulloverPieceRole::Front | PulloverPieceRole::Back
        );
        let courses = if body { n.body_rows } else { n.sleeve_rows };
        let (bottom, top) = if body {
            (n.hem_wales, n.chest_wales)
        } else {
            (n.cuff_wales, n.head_wales)
        };
        let neck_rows = match piece.role {
            PulloverPieceRole::Front => n.front_neck_rows,
            PulloverPieceRole::Back => n.back_neck_rows,
            PulloverPieceRole::LeftSleeve | PulloverPieceRole::RightSleeve => 0,
        };
        let neck_start = courses - neck_rows + 1;
        let mut rows = vec![Vec::new(); courses as usize + 1];
        for node in &doc.stitches {
            if node.course > courses
                || !matches!(node.bed, Bed::Front | Bed::Back)
                || node.yarn_id != spec.yarn_id
                || node.cam_stitch_setting.setting != spec.cam_setting
            {
                return Err(invalid(
                    "pullover piece contains an unexpected course, bed, yarn, or cam setting",
                ));
            }
            rows[node.course as usize].push(node);
        }
        for (course, row) in rows.iter_mut().enumerate() {
            row.sort_by_key(|node| node.wale);
            let Some(first) = row.first() else {
                return Err(invalid("pullover piece has a missing course"));
            };
            let last = row.last().expect("nonempty course");
            if i64::from(first.wale) + i64::from(last.wale) != -1 || first.wale >= 0 {
                return Err(invalid(
                    "pullover piece must have symmetric outer selvedges",
                ));
            }
            let half = (-i64::from(first.wale)) as u32;
            if half > MAX_GARMENT_STITCHES as u32 / 2 {
                return Err(invalid("pullover piece width exceeds the garment budget"));
            }
            let open_neck = neck_rows > 0 && course >= neck_start as usize;
            if open_neck && half <= n.neck_half {
                return Err(invalid("pullover neckline removes a shoulder"));
            }
            let missing = if open_neck { 2 * n.neck_half } else { 0 };
            if row.len() as u64 != u64::from(2 * half - missing) {
                return Err(invalid(
                    "pullover piece contains a hole or duplicated chart cell",
                ));
            }
            for (index, node) in row.iter().enumerate() {
                let skip = if open_neck && index >= (half - n.neck_half) as usize {
                    missing
                } else {
                    0
                };
                let expected_wale = -i64::from(half) + index as i64 + i64::from(skip);
                if i64::from(node.wale) != expected_wale {
                    return Err(invalid(
                        "pullover row is not a contiguous panel or its declared shoulder pair",
                    ));
                }
                if (course == 0 && node.kind != LoopKind::CastOn)
                    || (course == courses as usize && node.kind != LoopKind::BindOff)
                {
                    return Err(invalid(
                        "pullover piece requires an actual cast-on and terminal chain bind-off",
                    ));
                }
            }
            if (course == 0 && 2 * half != bottom)
                || (course == courses as usize && 2 * half != top)
                || (body && course >= (courses - n.arm_rows) as usize && 2 * half != top)
            {
                return Err(invalid(
                    "pullover piece dimensions or straight armhole edges differ from the specification",
                ));
            }
        }
        let result = Self {
            role: piece.role,
            rows,
            pitch: gauge.blocked_wale_pitch_mm(),
            height: gauge.blocked_course_height_mm(),
            neck_half: n.neck_half as i32,
            neck_rows,
        };
        if neck_rows > 0 {
            let held = result.horizontal(
                courses - neck_rows,
                -result.neck_half..result.neck_half,
                GarmentEdgeKind::HeldNeck,
                false,
            );
            let expected_ids = held
                .stitch_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            if doc.metadata.get("held_neck_stitch_ids") != Some(&expected_ids)
                || doc.metadata.get("shoulder_start_course") != Some(&neck_start.to_string())
            {
                return Err(invalid(
                    "held neckline metadata does not identify the actual center edge",
                ));
            }
        } else if doc.metadata.contains_key("held_neck_stitch_ids")
            || doc.metadata.contains_key("shoulder_start_course")
        {
            return Err(invalid(
                "a straight bound neckline cannot declare held center loops",
            ));
        }
        Ok(result)
    }

    fn horizontal(
        &self,
        course: u32,
        wales: std::ops::Range<i32>,
        kind: GarmentEdgeKind,
        reverse: bool,
    ) -> GarmentEdge {
        let mut stitch_ids: Vec<_> = self.rows[course as usize]
            .iter()
            .filter(|node| wales.contains(&node.wale))
            .map(|node| node.id)
            .collect();
        if reverse {
            stitch_ids.reverse();
        }
        GarmentEdge {
            piece: self.role,
            kind,
            length_mm: stitch_ids.len() as f64 * self.pitch,
            stitch_ids,
        }
    }

    fn vertical(&self, first: u32, last: u32, kind: GarmentEdgeKind, reverse: bool) -> GarmentEdge {
        let at = |course: u32| -> &StitchNode {
            let row = &self.rows[course as usize];
            match kind {
                GarmentEdgeKind::LeftSelvedge => row[0],
                GarmentEdgeKind::RightSelvedge => row[row.len() - 1],
                GarmentEdgeKind::LeftNeckline => row
                    .iter()
                    .copied()
                    .find(|node| node.wale == -self.neck_half - 1)
                    .expect("validated inner shoulder edge"),
                GarmentEdgeKind::RightNeckline => row
                    .iter()
                    .copied()
                    .find(|node| node.wale == self.neck_half)
                    .expect("validated inner shoulder edge"),
                _ => unreachable!("vertical edges require a selvedge kind"),
            }
        };
        let mut stitch_ids = Vec::new();
        let mut length_mm = 0.0;
        for course in first..=last {
            let node = at(course);
            let previous = at(course - 1);
            let dx = f64::from(node.wale - previous.wale) * self.pitch;
            length_mm += det::hypot(dx, self.height);
            stitch_ids.push(node.id);
        }
        if reverse {
            stitch_ids.reverse();
        }
        GarmentEdge {
            piece: self.role,
            kind,
            stitch_ids,
            length_mm,
        }
    }

    fn neck_center(&self) -> GarmentEdge {
        let course = self.rows.len() as u32 - 1 - self.neck_rows;
        self.horizontal(
            course,
            -self.neck_half..self.neck_half,
            if self.neck_rows > 0 {
                GarmentEdgeKind::HeldNeck
            } else {
                GarmentEdgeKind::BindOff
            },
            false,
        )
    }

    fn neck_side(&self, left: bool, reverse: bool) -> Option<GarmentEdge> {
        if self.neck_rows == 0 {
            return None;
        }
        let last = self.rows.len() as u32 - 1;
        Some(self.vertical(
            last - self.neck_rows + 1,
            last,
            if left {
                GarmentEdgeKind::LeftNeckline
            } else {
                GarmentEdgeKind::RightNeckline
            },
            reverse,
        ))
    }
}

struct Assembly {
    seams: Vec<AssemblySeam>,
    openings: Vec<GarmentOpening>,
    residuals_mm: BTreeMap<String, f64>,
    instructions: Vec<String>,
}

fn seam(
    name: &str,
    a: GarmentEdge,
    b: GarmentEdge,
    tolerance: f64,
) -> Result<AssemblySeam, Refusal> {
    Refusal::finite("first seam edge length", a.length_mm)?;
    Refusal::finite("second seam edge length", b.length_mm)?;
    let difference = a.length_mm - b.length_mm;
    if a.stitch_ids.is_empty() || b.stitch_ids.is_empty() {
        return Err(invalid("a garment seam cannot contain an empty edge"));
    }
    if difference.abs() > tolerance {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("seam {name} differs by {difference:.6} mm, exceeding the explicit {tolerance:.6} mm tolerance"),
        ).repair("change the requested dimensions or gauge, or explicitly accept the reported seam ease"));
    }
    let na = a.stitch_ids.len() as u64;
    let nb = b.stitch_ids.len() as u64;
    let count = na.min(nb);
    let groups = (0..count)
        .map(|index| SeamJoinGroup {
            a_indices: [
                (index * na / count) as u32,
                ((index + 1) * na / count) as u32,
            ],
            b_indices: [
                (index * nb / count) as u32,
                ((index + 1) * nb / count) as u32,
            ],
        })
        .collect();
    Ok(AssemblySeam {
        name: name.to_string(),
        a,
        b,
        groups,
        length_mismatch_mm: difference,
    })
}

fn opening(name: &str, edges: Vec<GarmentEdge>) -> Result<GarmentOpening, Refusal> {
    if edges.is_empty() || edges.iter().any(|edge| edge.stitch_ids.is_empty()) {
        return Err(invalid(
            "a garment opening requires nonempty boundary sections",
        ));
    }
    let length_mm = edges.iter().map(|edge| edge.length_mm).sum();
    Refusal::finite("opening length", length_mm)?;
    Ok(GarmentOpening {
        name: name.to_string(),
        edges,
        length_mm,
    })
}

fn assemble(spec: &SeamedPulloverSpec, pieces: &[PulloverPiece]) -> Result<Assembly, Refusal> {
    let roles = [
        PulloverPieceRole::Front,
        PulloverPieceRole::Back,
        PulloverPieceRole::LeftSleeve,
        PulloverPieceRole::RightSleeve,
    ];
    if pieces.len() != roles.len()
        || pieces
            .iter()
            .zip(roles)
            .any(|(piece, role)| piece.role != role)
    {
        return Err(invalid(
            "pullover requires exactly front, back, left_sleeve, right_sleeve in that order",
        ));
    }
    let total = pieces.iter().fold(0u64, |sum, piece| {
        sum.saturating_add(piece.doc.stitches.len() as u64)
    });
    if total > MAX_GARMENT_STITCHES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "pullover piece graphs exceed the stitch budget",
        ));
    }
    let gauge = &pieces[0].doc.gauge;
    let n = counts(spec, gauge, MAX_GARMENT_STITCHES as u32)?;
    let boundaries: Vec<_> = pieces
        .iter()
        .map(|piece| Boundary::new(piece, spec, &n, gauge))
        .collect::<Result<_, _>>()?;
    let front = &boundaries[0];
    let back = &boundaries[1];
    let left = &boundaries[2];
    let right = &boundaries[3];
    for (a, b) in [(front, back), (left, right)] {
        if a.rows
            .iter()
            .zip(&b.rows)
            .any(|(ra, rb)| ra[0].wale != rb[0].wale)
        {
            return Err(invalid(
                "paired body pieces and sleeves must have identical outer shaping",
            ));
        }
    }
    let chest_half = (n.chest_wales / 2) as i32;
    let neck_half = n.neck_half as i32;
    let head_half = (n.head_wales / 2) as i32;
    let underarm = n.body_rows - n.arm_rows;
    let bind = GarmentEdgeKind::BindOff;
    let left_edge = GarmentEdgeKind::LeftSelvedge;
    let right_edge = GarmentEdgeKind::RightSelvedge;
    let tolerance = spec.max_seam_length_mismatch_mm;
    let mut seams = vec![
        seam(
            "left_shoulder",
            front.horizontal(n.body_rows, neck_half..chest_half, bind, true),
            back.horizontal(n.body_rows, -chest_half..-neck_half, bind, false),
            tolerance,
        )?,
        seam(
            "right_shoulder",
            front.horizontal(n.body_rows, -chest_half..-neck_half, bind, false),
            back.horizontal(n.body_rows, neck_half..chest_half, bind, true),
            tolerance,
        )?,
    ];
    // Each sleeve midpoint meets its shoulder seam. Both halves run from the
    // underarm corner toward that midpoint, never around the neckline.
    for (name, body, side, sleeve, negative_half) in [
        ("left_front_armhole", front, right_edge, left, true),
        ("left_back_armhole", back, left_edge, left, false),
        ("right_front_armhole", front, left_edge, right, false),
        ("right_back_armhole", back, right_edge, right, true),
    ] {
        seams.push(seam(
            name,
            body.vertical(underarm + 1, n.body_rows, side, false),
            sleeve.horizontal(
                n.sleeve_rows,
                if negative_half {
                    -head_half..0
                } else {
                    0..head_half
                },
                bind,
                !negative_half,
            ),
            tolerance,
        )?);
    }
    seams.push(seam(
        "left_body_side",
        front.vertical(1, underarm, right_edge, false),
        back.vertical(1, underarm, left_edge, false),
        tolerance,
    )?);
    seams.push(seam(
        "right_body_side",
        front.vertical(1, underarm, left_edge, false),
        back.vertical(1, underarm, right_edge, false),
        tolerance,
    )?);
    for (name, sleeve) in [
        ("left_sleeve_underarm", left),
        ("right_sleeve_underarm", right),
    ] {
        seams.push(seam(
            name,
            sleeve.vertical(1, n.sleeve_rows, left_edge, false),
            sleeve.vertical(1, n.sleeve_rows, right_edge, false),
            tolerance,
        )?);
    }
    let mut neckline = vec![front.neck_center()];
    neckline.extend(front.neck_side(false, false));
    neckline.extend(back.neck_side(true, true));
    neckline.push(back.neck_center());
    neckline.extend(back.neck_side(false, false));
    neckline.extend(front.neck_side(true, true));
    let openings = vec![
        opening("neck", neckline)?,
        opening(
            "hem",
            vec![
                front.horizontal(0, i32::MIN..i32::MAX, GarmentEdgeKind::CastOn, false),
                back.horizontal(0, i32::MIN..i32::MAX, GarmentEdgeKind::CastOn, false),
            ],
        )?,
        opening(
            "left_cuff",
            vec![left.horizontal(0, i32::MIN..i32::MAX, GarmentEdgeKind::CastOn, false)],
        )?,
        opening(
            "right_cuff",
            vec![right.horizontal(0, i32::MIN..i32::MAX, GarmentEdgeKind::CastOn, false)],
        )?,
    ];
    let pitch = gauge.blocked_wale_pitch_mm();
    let height = gauge.blocked_course_height_mm();
    let residuals_mm = BTreeMap::from([
        (
            "chest_girth_mm".to_string(),
            2.0 * f64::from(n.chest_wales) * pitch - spec.chest_girth_mm,
        ),
        (
            "hem_girth_mm".to_string(),
            2.0 * f64::from(n.hem_wales) * pitch - spec.hem_girth_mm,
        ),
        (
            "body_length_mm".to_string(),
            f64::from(n.body_rows) * height - spec.body_length_mm,
        ),
        (
            "armhole_depth_mm".to_string(),
            f64::from(n.arm_rows) * height - spec.armhole_depth_mm,
        ),
        (
            "sleeve_length_mm".to_string(),
            f64::from(n.sleeve_rows) * height - spec.sleeve_length_mm,
        ),
        (
            "cuff_girth_mm".to_string(),
            f64::from(n.cuff_wales) * pitch - spec.cuff_girth_mm,
        ),
        (
            "neck_width_mm".to_string(),
            2.0 * f64::from(n.neck_half) * pitch - spec.neck_width_mm,
        ),
        (
            "front_neck_depth_mm".to_string(),
            f64::from(n.front_neck_rows) * height - spec.front_neck_depth_mm,
        ),
        (
            "back_neck_depth_mm".to_string(),
            f64::from(n.back_neck_rows) * height - spec.back_neck_depth_mm,
        ),
        (
            "sleeve_head_width_mm".to_string(),
            f64::from(n.head_wales) * pitch - 2.0 * spec.armhole_depth_mm,
        ),
        (
            "sleeve_head_to_armhole_mm".to_string(),
            f64::from(n.head_wales) * pitch - 2.0 * f64::from(n.arm_rows) * height,
        ),
    ]);
    for (label, value) in &residuals_mm {
        Refusal::finite(label, *value)?;
    }
    let mut instructions = vec![
        "Knit and finish the four pieces independently. Each piece requires its own machine verification and qualification; this assembly plan is a manual construction plan.".to_string(),
        "Mark chart-left and chart-right edges before removing each piece. Orient the front so chart-right is the wearer's left, and the back oppositely; choose the garment's outer fabric face consistently.".to_string(),
        format!(
            "Join the two shoulder seams from the outer corners toward the neck, matching {} bound stitches per shoulder. Leave the center {} stitches of each body piece open for the neckline.",
            n.chest_wales / 2 - n.neck_half, 2 * n.neck_half,
        ),
        format!(
            "With the body open at its side seams, place each sleeve-head midpoint at its shoulder seam. Join each {}-stitch sleeve-head half to the corresponding {}-row front/back armhole, working from underarm toward shoulder in the recorded edge order.",
            n.head_wales / 2, n.arm_rows,
        ),
        "Join the marked boundary edges without an added seam allowance. If the chosen joining method consumes edge stitches or changes gauge, account for that take-up in the requested dimensions and verify after assembly.".to_string(),
        "Use the seam groups in order. Each half-open index range identifies the next stitches or row edges to join; groups consume both edges exactly once and distribute unequal row/stitch counts evenly.".to_string(),
        format!(
            "Close each sleeve underarm from cuff to sleeve head, then the matching body side from hem through {} formed rows after cast-on. The final {} body rows on each side are already joined to the sleeve head.",
            underarm, n.arm_rows,
        ),
        format!(
            "The sleeve-head circumference is {:.3} mm and the nominal neckline perimeter is {:.3} mm before finishing. Confirm head passage, intended ease, and reach on the wearer; these gauge-plane dimensions are not proof of fit.",
            f64::from(n.head_wales) * pitch, openings[0].length_mm,
        ),
        "Secure bind-off tails and seam ends manually. The hem and two cuffs remain open; no neckband, extra trim, stretch allowance, or automatic sewing operation is generated.".to_string(),
    ];
    if n.front_neck_rows > 0 || n.back_neck_rows > 0 {
        instructions.push(
            "Finish the declared held neckline loops manually without joining them into a shoulder seam. Preserve the neck opening; use the individual piece instructions for carrier changes and held-loop IDs. Machine export remains subject to holding and roller limits.".to_string(),
        );
    }
    Ok(Assembly {
        seams,
        openings,
        residuals_mm,
        instructions,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn fixture() -> (SeamedPulloverSpec, GaugeSpec, MachineProfile) {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 20.0,
            courses_per_100mm: 25.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let spec = SeamedPulloverSpec {
            name: "synthetic-seamed-pullover".to_string(),
            wearer_age_class: WearerAgeClass::Child3To14,
            chest_girth_mm: 600.0,
            hem_girth_mm: 560.0,
            body_length_mm: 320.0,
            armhole_depth_mm: 96.0,
            sleeve_length_mm: 240.0,
            cuff_girth_mm: 120.0,
            neck_width_mm: 100.0,
            front_neck_depth_mm: 16.0,
            back_neck_depth_mm: 0.0,
            body_rib_courses: 0,
            cuff_rib_courses: 0,
            cam_setting: 8.0,
            yarn_id: "synthetic-test-yarn".to_string(),
            carrier_id: CarrierId("1".to_string()),
            max_seam_length_mismatch_mm: 1.0,
            has_hood_or_neck_drawstring: false,
        };
        (spec, gauge, profile)
    }

    #[test]
    fn complete_garment_has_four_pieces_ten_seams_and_four_openings() {
        let (spec, gauge, profile) = fixture();
        let draft = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
        draft.validate().unwrap();
        assert_eq!(draft.pieces.len(), 4);
        assert_eq!(draft.seams.len(), 10);
        assert_eq!(
            draft
                .openings
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            vec!["neck", "hem", "left_cuff", "right_cuff"]
        );
        assert_eq!(draft.openings[0].length_mm, 232.0);
        assert_eq!(draft.openings[1].length_mm, 560.0);
        assert_eq!(draft.openings[2].length_mm, 120.0);
        assert_eq!(draft.residuals_mm["sleeve_head_width_mm"], -2.0);
        for seam in &draft.seams {
            if seam.name.ends_with("_armhole") {
                assert_eq!(seam.a.stitch_ids.len(), 24);
                assert_eq!(seam.b.stitch_ids.len(), 19);
                assert_eq!(seam.a.length_mm, 96.0);
                assert_eq!(seam.b.length_mm, 95.0);
                assert_eq!(seam.length_mismatch_mm, 1.0);
            } else {
                assert_eq!(seam.length_mismatch_mm, 0.0);
            }
            let mut a = 0;
            let mut b = 0;
            for group in &seam.groups {
                assert_eq!(group.a_indices[0], a);
                assert_eq!(group.b_indices[0], b);
                assert!(group.a_indices[1] > a);
                assert!(group.b_indices[1] > b);
                a = group.a_indices[1];
                b = group.b_indices[1];
            }
            assert_eq!(a as usize, seam.a.stitch_ids.len());
            assert_eq!(b as usize, seam.b.stitch_ids.len());
        }
        let underarm = draft
            .seams
            .iter()
            .find(|s| s.name == "left_sleeve_underarm")
            .unwrap();
        assert!(
            underarm.a.length_mm > 240.0,
            "tapered edge must include lateral movement"
        );
        assert_eq!(underarm.a.length_mm, underarm.b.length_mm);
    }

    #[test]
    fn every_boundary_face_is_accounted_once_and_orientation_meets_at_shoulders() {
        let (spec, gauge, profile) = fixture();
        let draft = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
        let mut actual = BTreeSet::new();
        for edge in draft
            .seams
            .iter()
            .flat_map(|s| [&s.a, &s.b])
            .chain(draft.openings.iter().flat_map(|opening| &opening.edges))
        {
            for id in &edge.stitch_ids {
                assert!(
                    actual.insert((edge.piece, edge.kind, *id)),
                    "edge face used twice"
                );
            }
        }
        let mut expected = BTreeSet::new();
        for piece in &draft.pieces {
            let doc = &piece.doc;
            let last = doc.stitches.iter().map(|s| s.course).max().unwrap();
            let by_id: BTreeMap<_, _> = doc.stitches.iter().map(|s| (s.id, s)).collect();
            for node in &doc.stitches {
                if node.course == 0 {
                    expected.insert((piece.role, GarmentEdgeKind::CastOn, node.id));
                }
                if node.course == last {
                    expected.insert((piece.role, GarmentEdgeKind::BindOff, node.id));
                }
            }
            for course in 1..=last {
                let row: Vec<_> = doc.stitches.iter().filter(|s| s.course == course).collect();
                let left = row.iter().min_by_key(|s| s.wale).unwrap();
                let right = row.iter().max_by_key(|s| s.wale).unwrap();
                expected.insert((piece.role, GarmentEdgeKind::LeftSelvedge, left.id));
                expected.insert((piece.role, GarmentEdgeKind::RightSelvedge, right.id));
            }
            if let Some(ids) = doc.metadata.get("held_neck_stitch_ids") {
                let held: Vec<_> = ids
                    .split(',')
                    .map(|id| by_id[&id.parse::<u32>().unwrap()])
                    .collect();
                let lower = held.iter().map(|s| s.wale).min().unwrap();
                let upper = held.iter().map(|s| s.wale).max().unwrap();
                for node in &held {
                    expected.insert((piece.role, GarmentEdgeKind::HeldNeck, node.id));
                }
                for node in doc.stitches.iter().filter(|s| s.course > held[0].course) {
                    if node.wale == lower - 1 {
                        expected.insert((piece.role, GarmentEdgeKind::LeftNeckline, node.id));
                    } else if node.wale == upper + 1 {
                        expected.insert((piece.role, GarmentEdgeKind::RightNeckline, node.id));
                    }
                }
            }
        }
        assert_eq!(actual, expected);
        let shoulder = draft
            .seams
            .iter()
            .find(|s| s.name == "left_shoulder")
            .unwrap();
        let front = &draft.pieces[0].doc;
        let back = &draft.pieces[1].doc;
        let wale = |doc: &KnitDoc, id| doc.stitches.iter().find(|s| s.id == id).unwrap().wale;
        assert_eq!(wale(front, shoulder.a.stitch_ids[0]), 29);
        assert_eq!(wale(front, *shoulder.a.stitch_ids.last().unwrap()), 10);
        assert_eq!(wale(back, shoulder.b.stitch_ids[0]), -30);
        assert_eq!(wale(back, *shoulder.b.stitch_ids.last().unwrap()), -11);
        let front_arm = draft
            .seams
            .iter()
            .find(|s| s.name == "left_front_armhole")
            .unwrap();
        let back_arm = draft
            .seams
            .iter()
            .find(|s| s.name == "left_back_armhole")
            .unwrap();
        let sleeve = &draft.pieces[2].doc;
        assert_eq!(wale(sleeve, front_arm.b.stitch_ids[0]), -19);
        assert_eq!(wale(sleeve, *front_arm.b.stitch_ids.last().unwrap()), -1);
        assert_eq!(wale(sleeve, back_arm.b.stitch_ids[0]), 18);
        assert_eq!(wale(sleeve, *back_arm.b.stitch_ids.last().unwrap()), 0);
        let front_arm = draft
            .seams
            .iter()
            .find(|s| s.name == "right_front_armhole")
            .unwrap();
        let back_arm = draft
            .seams
            .iter()
            .find(|s| s.name == "right_back_armhole")
            .unwrap();
        let sleeve = &draft.pieces[3].doc;
        assert_eq!(wale(sleeve, front_arm.b.stitch_ids[0]), 18);
        assert_eq!(wale(sleeve, *front_arm.b.stitch_ids.last().unwrap()), 0);
        assert_eq!(wale(sleeve, back_arm.b.stitch_ids[0]), -19);
        assert_eq!(wale(sleeve, *back_arm.b.stitch_ids.last().unwrap()), -1);
    }

    #[test]
    fn straight_bound_neck_centers_are_left_open_and_positive_back_drop_is_held() {
        let (mut spec, gauge, profile) = fixture();
        spec.front_neck_depth_mm = 0.0;
        let straight = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
        assert_eq!(straight.openings[0].length_mm, 200.0);
        assert_eq!(straight.openings[0].edges.len(), 2);
        for edge in &straight.openings[0].edges {
            assert_eq!(edge.kind, GarmentEdgeKind::BindOff);
            assert_eq!(edge.stitch_ids.len(), 20);
            assert!(
                !straight
                    .seams
                    .iter()
                    .flat_map(|s| [&s.a, &s.b])
                    .any(|seam_edge| seam_edge.piece == edge.piece
                        && seam_edge.kind == edge.kind
                        && seam_edge
                            .stitch_ids
                            .iter()
                            .any(|id| edge.stitch_ids.contains(id)))
            );
        }
        spec.front_neck_depth_mm = 16.0;
        spec.back_neck_depth_mm = 8.0;
        let shaped = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
        shaped.validate().unwrap();
        assert_eq!(shaped.openings[0].length_mm, 248.0);
        assert_eq!(shaped.openings[0].edges.len(), 6);
        assert_eq!(
            shaped.openings[0]
                .edges
                .iter()
                .filter(|e| e.kind == GarmentEdgeKind::HeldNeck)
                .count(),
            2
        );
    }

    #[test]
    fn saved_assembly_roundtrips_and_rejects_stale_edges_pairings_and_piece_holes() {
        let (spec, gauge, profile) = fixture();
        let draft = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
        let encoded = serde_json::to_vec(&draft).unwrap();
        let decoded: SeamedPulloverDraft = serde_json::from_slice(&encoded).unwrap();
        decoded.validate().unwrap();
        let mut bad = draft.clone();
        bad.seams[0].groups[0].b_indices[1] += 1;
        assert_eq!(
            bad.validate().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let mut bad = draft.clone();
        bad.openings[0].edges[0].stitch_ids.pop();
        assert_eq!(
            bad.validate().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let mut bad = draft.clone();
        bad.pieces[1].role = PulloverPieceRole::Front;
        assert_eq!(
            bad.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let mut bad = draft;
        // Retain IDs/parents, but create a hole and a duplicate source chart cell.
        bad.pieces[2].doc.stitches[0].wale += 1;
        assert_eq!(
            bad.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }

    #[test]
    fn explicit_mismatch_tolerance_capacity_and_child_rules_refuse_without_partial_results() {
        let (mut spec, gauge, profile) = fixture();
        spec.max_seam_length_mismatch_mm = 0.999;
        let mismatch = draft_seamed_pullover(&spec, &gauge, &profile).unwrap_err();
        assert_eq!(mismatch.code, RefusalCode::OutOfRange);
        assert!(mismatch.message.contains("left_front_armhole"));
        spec.max_seam_length_mismatch_mm = 1.0;
        spec.has_hood_or_neck_drawstring = true;
        assert_eq!(
            draft_seamed_pullover(&spec, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );
        spec.has_hood_or_neck_drawstring = false;
        spec.wearer_age_class = WearerAgeClass::CalibrationSwatchOnly;
        assert_eq!(
            draft_seamed_pullover(&spec, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );
        spec.wearer_age_class = WearerAgeClass::Adult;
        spec.chest_girth_mm = 1.0e9;
        assert!(draft_seamed_pullover(&spec, &gauge, &profile).is_err());
        spec.chest_girth_mm = 600.0;
        spec.armhole_depth_mm = 320.0;
        assert!(draft_seamed_pullover(&spec, &gauge, &profile).is_err());
        spec.armhole_depth_mm = 96.0;
        spec.sleeve_length_mm = f64::NAN;
        assert_eq!(
            draft_seamed_pullover(&spec, &gauge, &profile)
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
    }

    #[test]
    fn all_four_pieces_compile_with_real_hand_profile_and_no_qualification_exemption() {
        let (mut spec, mut gauge, _) = fixture();
        let profile = MachineProfile::hand_knit(4.0);
        gauge.needle_pitch_mm = profile.needle_pitch_mm;
        spec.cam_setting = 13.0;
        for (front_drop, back_drop) in [(0.0, 0.0), (16.0, 8.0)] {
            spec.front_neck_depth_mm = front_drop;
            spec.back_neck_depth_mm = back_drop;
            let draft = draft_seamed_pullover(&spec, &gauge, &profile).unwrap();
            for piece in &draft.pieces {
                super::super::compile::compile_knit_doc(&piece.doc, &profile, None, None, None)
                    .unwrap_or_else(|error| panic!("{}: {error:?}", piece.role.as_str()));
            }
        }
    }
}

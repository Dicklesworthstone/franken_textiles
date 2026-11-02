//! Source-qualified cold-knife path planning on the existing final roll/finite-stock geometry.
//! Every internal cut precedes every exterior release. Paths retain their authored start and
//! direction; raised-tool travel may be reordered or explicitly routed around qualified planar
//! fixtures. Acceleration requires an explicit exact-stop profile; kerf and three-dimensional
//! machine trajectories are not inferred.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{
    Refusal, RefusalCode, Vec2,
    polyline::{point_in_polygon, signed_area},
    receipt::sha256_hex,
};
use ftx_pattern::{
    Pattern,
    material_catalog::ResolvedMaterial,
    materials::{Evidence, Source},
};
use serde::{Deserialize, Serialize};

use super::{CutPlacement, CutPlan, RollMarker, bad, bbox, geometry, stock};

const MAX_PATHS: usize = 16_384;
const MAX_POINTS: usize = 250_000;
const MAX_GEOMETRY_WORK: usize = 20_000_000;
const MAX_SEARCH_WORK: usize = 2_000_000;

fn default_geometry_work() -> usize {
    1_000_000
}

/// Deterministic ordering of unchanged paths, separately within internal/exterior phases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    /// Retain original placement/feature order within each phase.
    #[default]
    Authored,
    /// Bounded nearest authored start; retain the complete baseline unless the candidate improves
    /// declared motion-profile time, or travel distance when the profile is omitted.
    NearestPath,
}

/// Opt-in conservative routing of raised moves using the already qualified planar tool/fixtures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TravelRouting {
    /// `fixture_box_visibility/1`: checked straight links between expanded fixture-box corners.
    pub model: String,
    /// Positive additional offset of candidate corners beyond the raised-tool radius, in mm.
    /// This chooses the search graph, not a physical safety margin or a new tool envelope.
    pub waypoint_offset_mm: f64,
}

/// Authored location of one complete stock item on the machine bed, without changing its marker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockLoading {
    /// Index in the final roll or finite-stock marker array, including unused finite-stock entries.
    pub marker: usize,
    /// Machine-frame location of the whole stock bounding-box lower-left corner, in mm.
    /// This is a modeled translation, not measured registration or a workholding qualification.
    pub lower_left_mm: Vec2,
}

/// Optional rest-to-rest timing for every retained straight cut or raised-travel segment.
/// The complete process qualification must cover these accelerations and exact-stop behavior.
/// Collinear authored vertices remain stops; no controller blending or blade rotation is inferred.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionProfile {
    /// `trapezoidal_exact_stop/1`: symmetric acceleration/braking, with a triangular short move.
    pub model: String,
    /// Positive tangential acceleration and braking magnitude during cutting, in mm/s².
    pub cutting_acceleration_mm_s2: f64,
    /// Positive tangential acceleration and braking magnitude during raised travel, in mm/s².
    pub travel_acceleration_mm_s2: f64,
}

/// One fixed cold through-drill sharing the knife head's XY centre and raised travel envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillingTool {
    /// `coaxial_cold_through_drill/1`; no inferred offset, thermal tool or interchangeable bits.
    pub model: String,
    /// Exact bit/tool identity locked by the complete machine digest.
    pub tool: String,
    /// Positive physical hole diameter in mm; every authored hole must match exactly.
    pub diameter_mm: f64,
    /// Maximum admitted uncompressed single-ply thickness, in mm.
    pub maximum_thickness_mm: f64,
    /// In-place selection time from the initially selected knife to this drill, in seconds.
    pub select_drill_seconds: f64,
    /// In-place selection time back to the knife after the drill phase, in seconds.
    pub select_knife_seconds: f64,
}

/// Explicit cold-knife configuration. A qualification locks every field, not just its display ID.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Stable machine/configuration identity.
    pub id: String,
    /// Exact blade/tool identity; thermal cutting is not part of this model.
    pub blade: String,
    /// Rectangular admitted bed, [width, length] in mm, with origin at its lower-left corner.
    pub bed_mm: [f64; 2],
    /// Maximum admitted uncompressed sheet thickness, in mm.
    pub maximum_thickness_mm: f64,
    /// Maximum continuous cutting feed, in mm/s.
    pub maximum_feed_mm_s: f64,
    /// Modeled raised-tool travel speed, in mm/s.
    pub travel_mm_s: f64,
    /// Modeled complete raise/reposition-height/lower duration for each separate path, in seconds.
    /// This excludes travel, which is counted independently.
    pub lift_lower_seconds: f64,
    /// Known initial head coordinate for each independently loaded marker.
    pub start_mm: Vec2,
    /// Include a final raised-tool straight return to the same initial coordinate.
    pub return_to_start: bool,
    /// Independently sourced fixed fixture footprints and tool envelopes in this machine frame.
    /// Absence makes no tool/fixture clearance claim and preserves the original machine digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture_clearance: Option<FixtureClearance>,
    /// Independently qualified exact-stop acceleration model. Omission preserves constant-rate
    /// timing and the original machine digest; adding or changing it requires a new qualification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_profile: Option<MotionProfile>,
    /// Optional fixed drill; adding or changing it requires a new complete machine qualification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drilling: Option<DrillingTool>,
}

/// Filled, stationary exclusion polygon at both declared tool elevations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Unique fixture identity in this complete loaded configuration.
    pub id: String,
    /// Simple machine-frame polygon in mm, either winding, with implicit closing edge.
    pub outline_mm: Vec<Vec2>,
}

/// Optional source-qualified planar checks of complete cutting and raised-travel sweeps.
/// These footprints must cover the fixture at both elevations; no height, grip or registration
/// is inferred. Every process qualification also locks this complete policy via the machine hash.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureClearance {
    /// `planar_disk_sweep/1`; no inferred 3D tool geometry or automatic obstacle routing.
    pub model: String,
    /// Positive radius of the complete cutting tool envelope, centered on the path, in mm.
    pub cutting_radius_mm: f64,
    /// Positive radius of the complete raised tool envelope, centered on its travel, in mm.
    pub raised_radius_mm: f64,
    /// Every fixed filled fixture footprint; 1..=64 simple polygons, at most 1024 total vertices.
    pub fixtures: Vec<Fixture>,
    /// Acquisition of the tool envelopes, fixture geometry and declared frame/elevations.
    pub source: Source,
    /// Geometry acquisition and applicability protocol, not just a display identity.
    pub method: String,
    /// Retained fixture, loading and registration assumptions.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Geometry evidence is admitted independently of the process rate and textile evidence.
    pub evidence: Evidence,
    /// Separate explicit permission for estimated geometry/applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Independently supplied process applicability and rate for one exact textile snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// Stable observation identity retained by the marker program.
    pub id: String,
    /// Exact source-bearing material digest, including its conditioning and estimate policy.
    pub material_sha256: String,
    /// Complete typed machine digest from [`machine_sha256`].
    pub machine_sha256: String,
    /// Qualified uncompressed sheet-thickness interval, in mm.
    pub thickness_mm: [f64; 2],
    /// Source-qualified selected continuous cutting feed, in mm/s.
    pub feed_mm_s: f64,
    /// Must be `all_in_plane`; a warp-only test cannot qualify an arbitrary closed pattern.
    pub directions: String,
    /// Origin of this complete process observation, including the locked timing configuration.
    pub evidence: Evidence,
    /// External or privately retained local coupon/process observation.
    pub source: Source,
    /// Protocol and timing-phase definitions; an identifier alone is insufficient.
    pub method: String,
    /// Specimen, workholding, finish, conditioning and other applicability constraints.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Separate explicit consent for an estimated process observation.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Independently sourced through-hole cycle and applicability for this exact material/machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drilling: Option<DrillQualification>,
}

/// Independently qualified fixed-bit drill cycle, beginning and ending at raised travel height.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillQualification {
    /// Stable drill observation identity retained by every marker that uses it.
    pub id: String,
    /// Qualified uncompressed single-ply thickness interval, in mm.
    pub thickness_mm: [f64; 2],
    /// Positive time from raised height through the complete material, in seconds.
    pub plunge_seconds: f64,
    /// Nonnegative full-depth dwell time, in seconds.
    pub dwell_seconds: f64,
    /// Positive complete retraction time back to raised travel height, in seconds.
    pub retract_seconds: f64,
    /// External or privately retained local drill-cycle observation.
    pub source: Source,
    /// Protocol defining the full cycle and the fixed tool's applicability.
    pub method: String,
    /// Specimen, backing, workholding, conditioning and other applicability constraints.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence for drilling, independent of the parent knife observation.
    pub evidence: Evidence,
    /// Separate explicit consent for estimated drill process evidence.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Optional program policy on ordinary roll or finite-stock cutting options.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `qualified_cold_knife_paths/1`; optional explicit cold drilling, no inferred kerf or heat.
    pub model: String,
    /// Admitted machine and complete modeled timing configuration.
    pub machine: Machine,
    /// One unambiguous recipe per material; independently qualified unused alternatives may remain.
    pub qualifications: Vec<Qualification>,
    /// Default authored order, or bounded nearest-path heuristic.
    #[serde(default)]
    pub ordering: Ordering,
    /// Optional checked detours around the machine's qualified stationary fixtures. Omission
    /// retains straight raised travel; cutting paths and their authored endpoints never change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub travel_routing: Option<TravelRouting>,
    /// Optional complete-stock locations; unlisted markers retain their origin-based placement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stock_loading: Vec<StockLoading>,
    /// Shared candidate-comparison budget across all markers, in 1..=2000000.
    pub work_limit: usize,
    /// Shared geometric-validation budget across all markers, in 1..=20000000.
    /// Defaults to one million; studies reserve this whole limit across every alternative.
    #[serde(default = "default_geometry_work")]
    pub geometry_work_limit: usize,
}

/// Reference into the retained final marker, without regenerating or rounding its cut geometry.
#[derive(Clone, Debug, Serialize)]
pub struct PathRef {
    /// Index into the marker's physical placements, preserving copy and expanded instance identity.
    pub placement: usize,
    /// Index into `internal_cut_mm`; absence identifies the placement's exterior `cut_mm` ring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_cut: Option<usize>,
}

/// One complete cut and its preceding raised-tool travel.
#[derive(Clone, Debug, Serialize)]
pub struct Run {
    /// Exact unchanged polyline in the surrounding cut plan.
    pub path: PathRef,
    /// Source piece ID.
    pub piece: String,
    /// Expanded mirrored/unfolded physical instance ID.
    pub instance: String,
    /// Zero-based complete garment copy.
    pub copy: usize,
    /// Closed paths finish at their unchanged first point; open slits finish at their last point.
    pub closed: bool,
    /// Raised-tool endpoints from the preceding path end (or marker start).
    pub travel_mm: [Vec2; 2],
    /// Intermediate machine-frame vertices of a checked detour, in traversal order. Omission
    /// means the original straight move; these points are never cutting coordinates.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub travel_waypoints_mm: Vec<Vec2>,
    /// Exact beginning and end of the cut, retaining source traversal direction.
    pub cut_endpoints_mm: [Vec2; 2],
    /// Complete polyline length, including the closing edge exactly once, in mm.
    pub cut_length_mm: f64,
}

/// One complete qualified through-hole and its preceding raised-tool travel.
#[derive(Clone, Debug, Serialize)]
pub struct DrillRun {
    /// Exact physical placement in the surrounding marker.
    pub placement: usize,
    /// Exact authored expanded hole index in that placement's `drills_mm` inventory.
    pub drill: usize,
    /// Source piece ID.
    pub piece: String,
    /// Expanded mirrored/unfolded physical instance ID.
    pub instance: String,
    /// Zero-based complete garment copy.
    pub copy: usize,
    /// Exact hole centre in the modeled loaded machine frame, in mm.
    pub center_mm: Vec2,
    /// Explicit hole diameter, exactly matching the fixed qualified bit, in mm.
    pub diameter_mm: f64,
    /// Raised-tool endpoints from marker start or the preceding hole centre.
    pub travel_mm: [Vec2; 2],
    /// Checked intermediate raised vertices in the machine frame, in traversal order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub travel_waypoints_mm: Vec<Vec2>,
}

/// A complete marker traversal. No partially cut marker is returned when optimization runs out.
#[derive(Clone, Debug, Serialize)]
pub struct Traversal {
    /// Fixed authored placement/hole order, completed before every knife path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub drills: Vec<DrillRun>,
    /// Every declared internal path followed by every physical exterior exactly once.
    pub runs: Vec<Run>,
    /// Optional final raised-tool endpoints; no cutting occurs during this return.
    pub return_mm: Option<[Vec2; 2]>,
    /// Intermediate machine-frame vertices of the requested final raised return.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub return_waypoints_mm: Vec<Vec2>,
    /// Total complete cut length, in mm.
    pub cut_length_mm: f64,
    /// Total raised-tool travel including requested return, in mm.
    pub travel_length_mm: f64,
    /// Constant-rate or explicitly qualified exact-stop cutting time, in seconds.
    pub cutting_seconds: f64,
    /// Constant-rate or explicitly qualified exact-stop raised-travel time, in seconds.
    pub travel_seconds: f64,
    /// One declared lift/lower cycle per path, in seconds.
    pub lift_lower_seconds: f64,
    /// Complete plunge/dwell/retract cycles, with no additional knife lift/lower charge.
    #[serde(skip_serializing_if = "is_zero")]
    pub drilling_seconds: f64,
    /// One in-place drill selection and one knife selection for a nonempty drill phase.
    #[serde(skip_serializing_if = "is_zero")]
    pub tool_change_seconds: f64,
    /// Sum of the declared cut, travel, lift/lower, drilling and tool-selection phases.
    pub modeled_path_seconds: f64,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl DrillRun {
    /// Complete preceding raised-tool polyline in machine-frame millimetres.
    pub fn travel_points(&self) -> impl Iterator<Item = Vec2> + '_ {
        std::iter::once(self.travel_mm[0])
            .chain(self.travel_waypoints_mm.iter().copied())
            .chain(std::iter::once(self.travel_mm[1]))
    }
}

impl Run {
    /// Complete raised-tool polyline in machine-frame millimetres, including both endpoints.
    pub fn travel_points(&self) -> impl Iterator<Item = Vec2> + '_ {
        std::iter::once(self.travel_mm[0])
            .chain(self.travel_waypoints_mm.iter().copied())
            .chain(std::iter::once(self.travel_mm[1]))
    }
}

impl Traversal {
    /// Complete requested final raised return in machine-frame millimetres.
    pub fn return_points(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.return_mm
            .iter()
            .map(|p| p[0])
            .chain(self.return_waypoints_mm.iter().copied())
            .chain(self.return_mm.iter().map(|p| p[1]))
    }

    /// Recompute all retained raised legs, including detours and the final return, in mm.
    /// This checks finite connected coordinates, not source applicability or fixture clearance.
    ///
    /// # Errors
    /// Empty/disconnected traversal, orphan return vertices, or non-finite length/coordinates.
    pub fn checked_travel_length(&self) -> Result<f64, Refusal> {
        let first = self
            .runs
            .first()
            .ok_or_else(|| bad("knife traversal has no cutting paths"))?;
        let start = self
            .drills
            .first()
            .map_or(first.travel_mm[0], |d| d.travel_mm[0]);
        let mut position = start;
        let mut length = 0.;
        for drill in &self.drills {
            if drill.travel_mm[0] != position
                || drill.travel_mm[1] != drill.center_mm
                || !drill.center_mm.is_finite()
                || !positive(drill.diameter_mm)
            {
                return Err(bad(
                    "drill raised travel is disconnected from its hole centres",
                ));
            }
            length += raised_length(drill.travel_points())?;
            position = drill.center_mm;
        }
        for run in &self.runs {
            if run.travel_mm[0] != position
                || run.travel_mm[1] != run.cut_endpoints_mm[0]
                || run.cut_endpoints_mm.iter().any(|p| !p.is_finite())
            {
                return Err(bad(
                    "knife raised travel is disconnected from its cutting paths",
                ));
            }
            length += raised_length(run.travel_points())?;
            position = run.cut_endpoints_mm[1];
        }
        if let Some([start, end]) = self.return_mm {
            if start != position
                || end
                    != self
                        .drills
                        .first()
                        .map_or(first.travel_mm[0], |d| d.travel_mm[0])
            {
                return Err(bad("knife final return is disconnected from its traversal"));
            }
            length += raised_length(self.return_points())?;
        } else if !self.return_waypoints_mm.is_empty() {
            return Err(bad("knife return waypoints require a final return"));
        }
        if !length.is_finite() {
            return Err(bad("knife raised-travel length overflows"));
        }
        Ok(length)
    }
}

fn raised_length(mut points: impl Iterator<Item = Vec2>) -> Result<f64, Refusal> {
    let mut previous = points
        .next()
        .filter(|p| p.is_finite())
        .ok_or_else(|| bad("knife raised travel needs finite coordinates"))?;
    let mut length = 0.;
    for point in points {
        if !point.is_finite() {
            return Err(bad("knife raised travel needs finite coordinates"));
        }
        length += previous.dist(point);
        previous = point;
    }
    if !length.is_finite() {
        return Err(bad("knife raised-travel length overflows"));
    }
    Ok(length)
}

/// The exact physical hide/remnant and its explicitly reported modeled loading position.
#[derive(Clone, Debug, Serialize)]
pub struct FiniteStock {
    /// Unique physical inventory identity, shared with finite-stock purchasing.
    pub stock_id: String,
    /// Exact frozen stock entry, including material, original geometry, sources and prior cuts.
    pub stock_sha256: String,
    /// Translation from unchanged final marker coordinates into the machine bed, in mm.
    /// The complete stock bounding-box lower-left corner uses the authored location or bed origin.
    pub marker_to_machine_offset_mm: Vec2,
    /// Composed rigid affine transform from original stock coordinates to the modeled machine bed.
    pub source_to_machine: [[f64; 3]; 2],
}

/// Retained translation of an explicitly positioned whole stock item, with its complete envelope.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Loading {
    /// Authored machine-frame lower-left position, in mm.
    pub lower_left_mm: Vec2,
    /// Translation applied to every unchanged marker path and annotation, in mm.
    pub marker_to_machine_offset_mm: Vec2,
    /// Complete loaded stock envelope [xmin, ymin, xmax, ymax], in mm. Roll width uses the source's
    /// upper reported bound when present, independently of the narrower layout/purchase basis.
    pub stock_bounds_mm: [f64; 4],
}

/// Complete baseline and selected traversal for one exact material marker.
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    /// Index into the cut plan's `rolls` or finite-stock `markers` array; unused stocks are omitted.
    pub marker: usize,
    /// Present only for individually inventoried finite stock; no stock boundary is cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finite_stock: Option<FiniteStock>,
    /// Present only for an explicitly authored loading; default output keeps its original fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<Loading>,
    /// Full unchanged material identity.
    pub material_sha256: String,
    /// Digest of all final placements, including directions, internal cuts, copies and transforms.
    pub geometry_sha256: String,
    /// Exact source qualification used, retained independently of any display name.
    pub qualification: String,
    /// Independent drill-cycle observation, present only when this marker actually has holes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drill_qualification: Option<String>,
    /// Admitted nominal thickness or full source-reported interval, in mm.
    pub thickness_mm: [f64; 2],
    /// Complete authored two-phase traversal.
    pub baseline: Traversal,
    /// Complete selected traversal; never slower than baseline under an explicit motion profile,
    /// or never more raised-tool distance under the original constant-rate model.
    pub selected: Traversal,
    /// Whether a complete nearest-path candidate improved modeled time (explicit motion profile)
    /// or raised distance (original constant-rate model) on the baseline.
    pub improved: bool,
    /// A candidate could not complete its checked raised route, so the checked baseline was kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_clearance_refusal: Option<Refusal>,
}

/// Source-qualified modeled cut planning, independent of hardware authorization.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-knife-cut-program/1` for roll stock, or `/2` for finite-stock identities/bed transforms.
    pub schema: &'static str,
    /// Full machine configuration and timing lock used by all admitted recipes.
    pub machine_sha256: String,
    /// Exact optional source geometry checked against both baseline and selected traversals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_clearance: Option<FixtureClearance>,
    /// Exact optional motion profile admitted by every complete machine/process qualification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_profile: Option<MotionProfile>,
    /// Exact optional fixed drill locked by every complete machine/process qualification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drilling: Option<DrillingTool>,
    /// Exact optional raised-move routing policy, independent of physical source qualification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travel_routing: Option<TravelRouting>,
    /// Exact authored locations, including any unused finite stocks which produce no toolpaths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stock_loading: Vec<StockLoading>,
    /// Complete marker-specific programs, in the original material/marker order.
    pub markers: Vec<Marker>,
    /// Actual bounded ordering and routing search work shared by every marker.
    pub search_work: usize,
    /// Actual geometric-validation work shared by every marker, independently of ordering search.
    pub geometry_work: usize,
    /// True when a complete baseline was retained because a nearest-path candidate was unfinished.
    pub budget_exhausted: bool,
    /// Always false: the nearest-path heuristic does not prove the shortest traversal.
    pub optimality_proven: bool,
    /// Always false: source admission does not validate an actual physical cutting cycle.
    pub physical_cutting_validated: bool,
    /// Always false: no device command, interlock or operator execution authorization is produced.
    pub hardware_execution_authorized: bool,
    /// Explicit modeled/unsupported process boundaries.
    pub limitations: Vec<&'static str>,
}

fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "knife program geometry/work budget exceeded",
    )
}
fn geometry_result(value: Option<bool>, message: &str) -> Result<(), Refusal> {
    match value {
        Some(true) => Ok(()),
        Some(false) => Err(bad(message)),
        None => Err(budget()),
    }
}

/// Digest of the complete typed machine, including phase timing and head/bed coordinates.
/// It does not itself qualify source evidence or validate machine settings.
///
/// # Errors
/// Serialization failure.
pub fn machine_sha256(machine: &Machine) -> Result<String, Refusal> {
    let value = serde_json::to_value(machine).map_err(|e| bad(e.to_string()))?;
    Ok(sha256_hex(
        &serde_json::to_vec(&value).map_err(|e| bad(e.to_string()))?,
    ))
}

impl MotionProfile {
    fn validate(&self) -> Result<(), Refusal> {
        if self.model != "trapezoidal_exact_stop/1"
            || !positive(self.cutting_acceleration_mm_s2)
            || !positive(self.travel_acceleration_mm_s2)
        {
            return Err(bad(
                "knife motion profile needs trapezoidal_exact_stop/1 and finite positive cutting/travel acceleration",
            ));
        }
        Ok(())
    }
}

impl DrillingTool {
    fn validate(&self) -> Result<(), Refusal> {
        if self.model != "coaxial_cold_through_drill/1"
            || !text(&self.tool)
            || !positive(self.diameter_mm)
            || !positive(self.diameter_mm * 0.5)
            || self.diameter_mm * 0.5 * 2.0 != self.diameter_mm
            || !positive(self.maximum_thickness_mm)
            || [self.select_drill_seconds, self.select_knife_seconds]
                .iter()
                .any(|&v| !v.is_finite() || v < 0.0)
            || !(self.select_drill_seconds + self.select_knife_seconds).is_finite()
        {
            return Err(bad(
                "drilling needs a fixed coaxial cold through-drill, explicit positive diameter/thickness and finite nonnegative in-place selection times",
            ));
        }
        Ok(())
    }
}

impl DrillQualification {
    /// Complete independent hole cycle; knife lift/lower time is not added to this cycle.
    pub(crate) fn cycle_seconds(&self) -> Result<f64, Refusal> {
        if !positive(self.plunge_seconds)
            || !self.dwell_seconds.is_finite()
            || self.dwell_seconds < 0.0
            || !positive(self.retract_seconds)
        {
            return Err(bad(
                "drill cycles require finite positive plunge/retract and nonnegative dwell times",
            ));
        }
        let seconds = self.plunge_seconds + self.dwell_seconds + self.retract_seconds;
        if !positive(seconds) {
            return Err(bad("complete drill cycle time cannot be represented"));
        }
        Ok(seconds)
    }

    fn validate(&self) -> Result<(), Refusal> {
        self.cycle_seconds()?;
        if !text(&self.id)
            || self.thickness_mm.iter().any(|&v| !positive(v))
            || self.thickness_mm[0] > self.thickness_mm[1]
            || !text(&self.source.reference)
            || !text(&self.source.accessed_on)
            || !text(&self.source.note)
            || !text(&self.method)
            || self.conditions.len() > 64
            || self.conditions.iter().any(|(k, v)| !text(k) || !text(v))
        {
            return Err(bad(
                "drilling needs independent complete source, method, conditions and positive thickness applicability",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated drill qualification requires its own explicit consent",
            ));
        }
        Ok(())
    }
}

// Each retained segment starts and ends at zero speed. Compare the triangular half-time with
// the time needed to reach the speed cap; this avoids squaring large speed/acceleration inputs.
fn exact_stop_seconds(length: f64, speed: f64, acceleration: f64) -> Result<f64, Refusal> {
    if !length.is_finite() || length < 0.0 || !positive(speed) || !positive(acceleration) {
        return Err(bad(
            "knife motion needs finite nonnegative length and positive speed/acceleration",
        ));
    }
    if length == 0.0 {
        return Ok(0.0);
    }
    let half_triangle = length.sqrt() / acceleration.sqrt();
    let acceleration_time = speed / acceleration;
    let seconds = if half_triangle <= acceleration_time {
        2.0 * half_triangle
    } else {
        let constant_rate_time = length / speed;
        if !positive(acceleration_time) || !positive(constant_rate_time) {
            return Err(bad(
                "positive knife motion phase cannot be represented as a finite time",
            ));
        }
        constant_rate_time + acceleration_time
    };
    if !positive(seconds) {
        return Err(bad(
            "positive knife motion cannot be represented as a finite time",
        ));
    }
    Ok(seconds)
}

impl Machine {
    /// Time of one complete retained cut; a repeated explicit closing endpoint is counted once.
    /// Source-marker coordinates preserve the producer's rigid-translation-independent arithmetic.
    pub(crate) fn cutting_seconds(
        &self,
        points: &[Vec2],
        closed: bool,
        feed: f64,
    ) -> Result<f64, Refusal> {
        let points = if closed && points.len() > 1 && points.first() == points.last() {
            &points[..points.len() - 1]
        } else {
            points
        };
        if !positive(feed)
            || points.len() < if closed { 3 } else { 2 }
            || points.iter().any(|p| !p.is_finite())
        {
            return Err(bad(
                "knife cut timing needs a complete finite path and positive feed",
            ));
        }
        if let Some(profile) = &self.motion_profile {
            profile.validate()?;
        }
        let mut sum = 0.0;
        for (a, b) in edges(points, closed) {
            let length = a.dist(b);
            if !positive(length) {
                return Err(bad("positive knife cut length cannot be represented"));
            }
            sum += if let Some(profile) = &self.motion_profile {
                exact_stop_seconds(length, feed, profile.cutting_acceleration_mm_s2)?
            } else {
                length
            };
        }
        if self.motion_profile.is_none() {
            sum /= feed;
        }
        if !positive(sum) {
            return Err(bad("positive knife cut timing overflows or underflows"));
        }
        Ok(sum)
    }

    /// Time of every retained raised chord, including detours and the requested final return.
    pub(crate) fn travel_seconds(&self, traversal: &Traversal) -> Result<f64, Refusal> {
        let length = traversal.checked_travel_length()?;
        if !positive(self.travel_mm_s) {
            return Err(bad("knife raised timing needs positive travel speed"));
        }
        let Some(profile) = &self.motion_profile else {
            let seconds = length / self.travel_mm_s;
            if !seconds.is_finite() || (length > 0.0 && seconds <= 0.0) {
                return Err(bad("knife raised timing overflows or underflows"));
            }
            return Ok(seconds);
        };
        profile.validate()?;
        let mut seconds = 0.0;
        for [a, b] in traversal
            .drills
            .iter()
            .flat_map(|drill| raised_segments(drill.travel_points()))
            .chain(
                traversal
                    .runs
                    .iter()
                    .flat_map(|run| raised_segments(run.travel_points())),
            )
            .chain(raised_segments(traversal.return_points()))
        {
            let length = a.dist(b);
            if a != b && !positive(length) {
                return Err(bad("positive knife raised length cannot be represented"));
            }
            seconds +=
                exact_stop_seconds(length, self.travel_mm_s, profile.travel_acceleration_mm_s2)?;
        }
        if !seconds.is_finite() || (length > 0.0 && seconds <= 0.0) {
            return Err(bad("knife raised timing overflows or underflows"));
        }
        Ok(seconds)
    }
}

impl FixtureClearance {
    fn validate(&self, work: &mut geometry::Work) -> Result<(), Refusal> {
        if self.model != "planar_disk_sweep/1"
            || [self.cutting_radius_mm, self.raised_radius_mm]
                .iter()
                .any(|&r| !positive(r) || r > 1_000_000.0)
            || !text(&self.source.reference)
            || !text(&self.source.accessed_on)
            || !text(&self.source.note)
            || !text(&self.method)
            || self.conditions.len() > 64
            || self.conditions.iter().any(|(k, v)| !text(k) || !text(v))
        {
            return Err(bad(
                "knife fixture clearance needs explicit positive tool envelopes and complete source geometry/applicability evidence",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated knife fixture geometry requires its own explicit consent",
            ));
        }
        if self.fixtures.is_empty()
            || self.fixtures.len() > 64
            || self
                .fixtures
                .iter()
                .fold(0usize, |n, f| n.saturating_add(f.outline_mm.len()))
                > 1024
        {
            return Err(budget());
        }
        let mut ids = BTreeSet::new();
        for fixture in &self.fixtures {
            let ring = &fixture.outline_mm;
            work.spend(ring.len()).ok_or_else(budget)?;
            if !text(&fixture.id)
                || !ids.insert(fixture.id.as_str())
                || ring.len() < 3
                || ring
                    .iter()
                    .any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()) > 1_000_000.0)
                || signed_area(ring).abs() <= 1e-8
            {
                return Err(bad(
                    "knife fixtures need unique identities and finite, nondegenerate machine-frame polygons with implicit closing edges",
                ));
            }
            geometry_result(
                geometry::simple(ring, work),
                "knife fixture polygon is not simple",
            )?;
        }
        Ok(())
    }

    fn check_segment(
        &self,
        [a, b]: [Vec2; 2],
        cutting: bool,
        work: &mut geometry::Work,
    ) -> Result<(), Refusal> {
        if let Some(fixture) = self.blocking_fixture([a, b], cutting, work)? {
            return Err(bad(format!(
                "knife {} tool envelope touches or crosses fixture {}, or strict clearance could not be resolved numerically",
                if cutting { "cutting" } else { "raised-travel" },
                fixture.id
            )));
        }
        Ok(())
    }

    fn blocking_fixture(
        &self,
        [a, b]: [Vec2; 2],
        cutting: bool,
        work: &mut geometry::Work,
    ) -> Result<Option<&Fixture>, Refusal> {
        let radius = if cutting {
            self.cutting_radius_mm
        } else {
            self.raised_radius_mm
        };
        for fixture in &self.fixtures {
            if !geometry::swept_disk_clear(a, b, radius, &fixture.outline_mm, work)
                .ok_or_else(budget)?
            {
                return Ok(Some(fixture));
            }
        }
        Ok(None)
    }

    fn check_travel(&self, route: &Traversal, work: &mut geometry::Work) -> Result<(), Refusal> {
        for drill in &route.drills {
            for segment in raised_segments(drill.travel_points()) {
                self.check_segment(segment, false, work)?;
            }
        }
        for run in &route.runs {
            for segment in raised_segments(run.travel_points()) {
                self.check_segment(segment, false, work)?;
            }
        }
        for segment in raised_segments(route.return_points()) {
            self.check_segment(segment, false, work)?;
        }
        Ok(())
    }
}

fn raised_segments(points: impl Iterator<Item = Vec2>) -> impl Iterator<Item = [Vec2; 2]> {
    let mut previous = None;
    points.filter_map(move |point| {
        let segment = previous.map(|p| [p, point]);
        previous = Some(point);
        segment
    })
}

impl TravelRouting {
    fn nodes(
        &self,
        machine: &Machine,
        clearance: &FixtureClearance,
        search: &mut geometry::Work,
        geometry: &mut geometry::Work,
    ) -> Result<Vec<Vec2>, Refusal> {
        let offset = clearance.raised_radius_mm + self.waypoint_offset_mm;
        let mut nodes = Vec::with_capacity(clearance.fixtures.len() * 4);
        for fixture in &clearance.fixtures {
            geometry
                .spend(fixture.outline_mm.len())
                .ok_or_else(budget)?;
            let b = bbox(&fixture.outline_mm)?;
            // Outward rounding is numerical containment, not an extra physical clearance.
            let (left, right) = ((b[0] - offset).next_down(), (b[2] + offset).next_up());
            let (bottom, top) = ((b[1] - offset).next_down(), (b[3] + offset).next_up());
            for point in [
                Vec2::new(left, bottom),
                Vec2::new(right, bottom),
                Vec2::new(right, top),
                Vec2::new(left, top),
            ] {
                search.spend(1).ok_or_else(budget)?;
                if on_bed(point, machine) {
                    nodes.push(point);
                }
            }
        }
        Ok(nodes)
    }
}

fn detour(
    endpoints: [Vec2; 2],
    corners: &[Vec2],
    machine: &Machine,
    clearance: &FixtureClearance,
    search: &mut geometry::Work,
    geometry: &mut geometry::Work,
) -> Result<Vec<Vec2>, Refusal> {
    if clearance
        .blocking_fixture(endpoints, false, geometry)?
        .is_none()
    {
        return Ok(Vec::new());
    }
    // A blocked endpoint cannot be repaired by translating an authored cut or the machine start.
    for point in endpoints {
        clearance.check_segment([point, point], false, geometry)?;
    }
    let nodes: Vec<_> = endpoints
        .into_iter()
        .chain(corners.iter().copied())
        .collect();
    search.spend(nodes.len()).ok_or_else(budget)?;
    let mut distance = vec![f64::INFINITY; nodes.len()];
    let mut previous = vec![usize::MAX; nodes.len()];
    let mut settled = vec![false; nodes.len()];
    distance[0] = 0.;
    loop {
        let mut best: Option<usize> = None;
        for i in 0..nodes.len() {
            search.spend(1).ok_or_else(budget)?;
            if !settled[i]
                && distance[i].is_finite()
                && best.is_none_or(|old| distance[i] < distance[old])
            {
                best = Some(i);
            }
        }
        let Some(i) = best else {
            return Err(bad(
                "no complete checked raised-travel route in the conservative fixture-corner graph; this does not prove general path infeasibility",
            ));
        };
        if i == 1 {
            break;
        }
        settled[i] = true;
        for j in 0..nodes.len() {
            search.spend(1).ok_or_else(budget)?;
            if settled[j] || i == j {
                continue;
            }
            let length = nodes[i].dist(nodes[j]);
            let weight = if let Some(profile) = &machine.motion_profile {
                exact_stop_seconds(
                    length,
                    machine.travel_mm_s,
                    profile.travel_acceleration_mm_s2,
                )?
            } else {
                length
            };
            let candidate = distance[i] + weight;
            if !candidate.is_finite() {
                return Err(bad("knife raised-route cost overflows"));
            }
            if candidate < distance[j]
                && clearance
                    .blocking_fixture([nodes[i], nodes[j]], false, geometry)?
                    .is_none()
            {
                distance[j] = candidate;
                previous[j] = i;
            }
        }
    }
    let mut path = Vec::new();
    let mut node = previous[1];
    while node != 0 {
        search.spend(1).ok_or_else(budget)?;
        path.push(nodes[node]);
        node = previous[node];
    }
    path.reverse();
    Ok(path)
}

fn admit_travel(
    route: &mut Traversal,
    machine: &Machine,
    corners: Option<&[Vec2]>,
    search: &mut geometry::Work,
    geometry: &mut geometry::Work,
) -> Result<(), Refusal> {
    let Some(clearance) = &machine.fixture_clearance else {
        return Ok(());
    };
    if let Some(corners) = corners {
        for drill in &mut route.drills {
            drill.travel_waypoints_mm = detour(
                drill.travel_mm,
                corners,
                machine,
                clearance,
                search,
                geometry,
            )?;
        }
        for run in &mut route.runs {
            run.travel_waypoints_mm =
                detour(run.travel_mm, corners, machine, clearance, search, geometry)?;
        }
        if let Some(endpoints) = route.return_mm {
            route.return_waypoints_mm =
                detour(endpoints, corners, machine, clearance, search, geometry)?;
        }
        let points = route
            .drills
            .iter()
            .fold(route.return_waypoints_mm.len(), |n, drill| {
                n.saturating_add(drill.travel_waypoints_mm.len().saturating_add(2))
            });
        let points = route.runs.iter().fold(points, |n, run| {
            n.saturating_add(run.travel_waypoints_mm.len().saturating_add(2))
        });
        geometry
            .spend(points.saturating_add(2))
            .ok_or_else(budget)?;
        route.travel_length_mm = route.checked_travel_length()?;
        route.travel_seconds = if machine.motion_profile.is_some() || !route.drills.is_empty() {
            machine.travel_seconds(route)?
        } else {
            route.travel_length_mm / machine.travel_mm_s
        };
        route.modeled_path_seconds =
            route.cutting_seconds + route.travel_seconds + route.lift_lower_seconds;
        if !route.drills.is_empty() {
            route.modeled_path_seconds += route.drilling_seconds + route.tool_change_seconds;
        }
        if !route.modeled_path_seconds.is_finite() {
            return Err(bad("knife routed timing arithmetic overflows"));
        }
    }
    // Recheck the complete retained route, including every selected intermediate and final return.
    clearance.check_travel(route, geometry)
}

impl Policy {
    fn validate(&self) -> Result<(String, BTreeMap<&str, &Qualification>), Refusal> {
        let m = &self.machine;
        if self.model != "qualified_cold_knife_paths/1"
            || !text(&m.id)
            || !text(&m.blade)
            || m.bed_mm.iter().any(|&v| !positive(v) || v > 1_000_000.0)
            || !positive(m.maximum_thickness_mm)
            || !positive(m.maximum_feed_mm_s)
            || !positive(m.travel_mm_s)
            || !m.lift_lower_seconds.is_finite()
            || m.lift_lower_seconds < 0.0
            || !on_bed(m.start_mm, m)
        {
            return Err(bad(
                "invalid cold-knife machine, model or timing configuration",
            ));
        }
        if let Some(profile) = &m.motion_profile {
            profile.validate()?;
        }
        if let Some(drilling) = &m.drilling {
            drilling.validate()?;
            if let Some(clearance) = &m.fixture_clearance
                && (clearance.cutting_radius_mm < drilling.diameter_mm * 0.5
                    || clearance.raised_radius_mm < drilling.diameter_mm * 0.5)
            {
                return Err(bad(
                    "complete cutting and raised fixture envelopes must enclose the fixed drill bit",
                ));
            }
        }
        if let Some(routing) = &self.travel_routing
            && (routing.model != "fixture_box_visibility/1"
                || !positive(routing.waypoint_offset_mm)
                || routing.waypoint_offset_mm > 1_000_000.0
                || m.fixture_clearance.is_none())
        {
            return Err(bad(
                "raised-travel routing requires qualified fixture geometry, fixture_box_visibility/1 and a positive waypoint_offset_mm up to 1000000",
            ));
        }
        let mut loaded_markers = BTreeSet::new();
        if self.stock_loading.len() > 4096
            || self.stock_loading.iter().any(|loading| {
                !loaded_markers.insert(loading.marker) || !on_bed(loading.lower_left_mm, m)
            })
        {
            return Err(bad(
                "stock loading needs at most 4096 unique marker indices and finite lower-left positions on the machine bed",
            ));
        }
        if !(1..=MAX_SEARCH_WORK).contains(&self.work_limit)
            || !(1..=MAX_GEOMETRY_WORK).contains(&self.geometry_work_limit)
            || self.qualifications.is_empty()
            || self.qualifications.len() > 4096
        {
            return Err(budget());
        }
        let machine = machine_sha256(m)?;
        let mut recipes = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut drill_ids = BTreeSet::new();
        for q in &self.qualifications {
            if !text(&q.id)
                || !ids.insert(q.id.as_str())
                || !hash(&q.material_sha256)
                || q.machine_sha256 != machine
                || q.thickness_mm.iter().any(|&v| !positive(v))
                || q.thickness_mm[0] > q.thickness_mm[1]
                || !positive(q.feed_mm_s)
                || q.feed_mm_s > m.maximum_feed_mm_s
                || q.directions != "all_in_plane"
                || !text(&q.source.reference)
                || !text(&q.source.accessed_on)
                || !text(&q.source.note)
                || !text(&q.method)
                || q.conditions.len() > 64
                || q.conditions.iter().any(|(k, v)| !text(k) || !text(v))
                || recipes.insert(q.material_sha256.as_str(), q).is_some()
            {
                return Err(bad(
                    "knife qualifications require unique identities, exact machine/material locks, supported directions and complete positive process evidence",
                ));
            }
            if q.evidence == Evidence::Estimate && !q.allow_estimates {
                return Err(bad(
                    "estimated knife qualification requires its own explicit consent",
                ));
            }
            if let Some(drilling) = &q.drilling {
                drilling.validate()?;
                if m.drilling.is_none() || !drill_ids.insert(drilling.id.as_str()) {
                    return Err(bad(
                        "drill qualifications need the explicit machine tool and unique observation identities",
                    ));
                }
            }
        }
        Ok((machine, recipes))
    }
}

fn on_bed(p: Vec2, m: &Machine) -> bool {
    p.is_finite() && p.x >= 0.0 && p.y >= 0.0 && p.x <= m.bed_mm[0] && p.y <= m.bed_mm[1]
}

pub(super) fn validate_source(pattern: &Pattern, indices: &BTreeSet<usize>) -> Result<(), Refusal> {
    let mut drill_count = 0usize;
    for &index in indices {
        let piece = &pattern.raw()["pieces"][index];
        // The rendering/export annotation reader deliberately tolerates incomplete annotations.
        // A process program must not qualify a path after that reader has dropped source points.
        if let Some(drills) = piece.get("drills") {
            let drills = drills
                .as_array()
                .ok_or_else(|| bad("declared drills must be an explicit array"))?;
            drill_count = drill_count.checked_add(drills.len()).ok_or_else(budget)?;
            if drill_count > MAX_PATHS {
                return Err(budget());
            }
            let source = drills.iter().map(|drill| {
                let xy = drill["p"].as_array().filter(|xy| xy.len() == 2)
                    .ok_or_else(|| bad("drills require complete explicit finite 2D source coordinates"))?;
                let x = xy[0].as_f64().filter(|x| x.is_finite())
                    .ok_or_else(|| bad("drills require finite source coordinates"))?;
                let y = xy[1].as_f64().filter(|y| y.is_finite())
                    .ok_or_else(|| bad("drills require finite source coordinates"))?;
                if !drill["d"].as_f64().is_some_and(positive) {
                    return Err(bad("drills require an explicit positive diameter; the drawing default is not process evidence"));
                }
                Ok(Vec2::new(x, y))
            }).collect::<Result<Vec<_>, Refusal>>()?;
            for instance in pattern.instances.iter().filter(|i| i.piece_index == index) {
                let maps = crate::geom::feature_maps(piece, Some(instance));
                let mut expanded = Vec::new();
                for point in &source {
                    let mut images = Vec::with_capacity(maps.len());
                    for map in &maps {
                        let image = map.apply(*point);
                        if !image.is_finite() {
                            return Err(bad("expanded drill source coordinates overflow"));
                        }
                        // An exact self-image on the fold is one physical hole. Distinct authored
                        // holes or merely nearby reflected images must never be silently discarded.
                        if !images.contains(&image) {
                            images.push(image);
                        }
                    }
                    expanded.extend(images);
                }
                validate_distinct_drill_sources(expanded)?;
            }
        }
        if let Some(internal) = piece.get("internal") {
            let internal = internal
                .as_array()
                .ok_or_else(|| bad("knife internal paths must be an array"))?;
            for path in internal.iter().filter(|p| p["kind"] == "cut") {
                if !path["closed"].is_boolean() {
                    return Err(bad(
                        "knife internal cuts need an explicit boolean closed flag; export inference is not process evidence",
                    ));
                }
                if !path["pts"].as_array().is_some_and(|points| {
                    points.len() >= 2
                        && points.iter().all(|point| {
                            point.as_array().is_some_and(|xy| {
                                xy.len() == 2
                                    && xy.iter().all(|x| x.as_f64().is_some_and(f64::is_finite))
                            })
                        })
                }) {
                    return Err(bad(
                        "knife internal cuts require complete finite 2D source coordinates; export point filtering cannot qualify a different path",
                    ));
                }
            }
        }
    }
    Ok(())
}

// The annotation renderer merges centres within 1e-6 mm. Refuse any such lossy source inventory
// before using it for manufacturing. A sorted sweep keeps this pre-normalization check bounded
// by O(n log n); neighbouring accepted points are separated, so each range has bounded density.
fn validate_distinct_drill_sources(mut points: Vec<Vec2>) -> Result<(), Refusal> {
    if points.len() > MAX_PATHS {
        return Err(budget());
    }
    fn ordered(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits >> 63 == 0 {
            bits ^ (1 << 63)
        } else {
            !bits
        }
    }
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let mut active = BTreeSet::new();
    let mut first = 0;
    for (i, p) in points.iter().enumerate() {
        while first < i && points[first].x < (p.x - 1e-6).next_down() {
            active.remove(&(ordered(points[first].y), first));
            first += 1;
        }
        let lower = (ordered((p.y - 1e-6).next_down()), 0usize);
        let upper = (ordered((p.y + 1e-6).next_up()), usize::MAX);
        for &(_, j) in active.range(lower..=upper) {
            if p.dist(points[j]) <= 1e-6 {
                return Err(bad(
                    "distinct authored or reflected drill holes would be lost by drawing annotation deduplication",
                ));
            }
        }
        active.insert((ordered(p.y), i));
    }
    Ok(())
}

struct Path<'a> {
    reference: PathRef,
    points: &'a [Vec2],
    offset: Option<Vec2>,
    closed: bool,
    length: f64,
}

impl Path<'_> {
    fn start(&self) -> Vec2 {
        self.offset.map_or(self.points[0], |d| self.points[0] + d)
    }
    fn end(&self) -> Vec2 {
        if self.closed {
            self.start()
        } else {
            let p = self.points[self.points.len() - 1];
            self.offset.map_or(p, |d| p + d)
        }
    }
}

#[derive(Clone, Copy)]
struct Envelope {
    size_mm: [f64; 2],
    origin_mm: Vec2,
    offset: Option<Vec2>,
}

enum MarkerSource<'a> {
    Roll(usize, &'a RollMarker),
    Finite(usize, &'a stock::Marker, &'a stock::Stock),
}

fn digest(value: &impl Serialize) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| bad(e.to_string()))?,
    ))
}

impl MarkerSource<'_> {
    fn index(&self) -> usize {
        match self {
            Self::Roll(i, _) | Self::Finite(i, _, _) => *i,
        }
    }
    fn material(&self) -> &ResolvedMaterial {
        match self {
            Self::Roll(_, m) => &m.material,
            Self::Finite(_, m, _) => &m.material,
        }
    }
    fn placements(&self) -> &[CutPlacement] {
        match self {
            Self::Roll(_, m) => &m.placements,
            Self::Finite(_, m, _) => &m.placements,
        }
    }
    fn frame(
        &self,
        machine: &Machine,
        loading: Option<&StockLoading>,
        work: &mut geometry::Work,
    ) -> Result<(Envelope, Option<FiniteStock>, Option<Loading>, String), Refusal> {
        let origin = loading.map_or(Vec2::ZERO, |loading| loading.lower_left_mm);
        let receipt = |offset, size: [f64; 2]| {
            loading.map(|_| Loading {
                lower_left_mm: origin,
                marker_to_machine_offset_mm: offset,
                stock_bounds_mm: [origin.x, origin.y, origin.x + size[0], origin.y + size[1]],
            })
        };
        if loading.is_some() {
            work.spend(2).ok_or_else(budget)?;
        }
        match self {
            Self::Roll(_, marker) => {
                let width = marker
                    .layout_width
                    .as_ref()
                    .map_or(marker.width_m, |w| w.width_m)
                    * 1000.0;
                // A conservative layout does not make the loaded physical stock narrower.
                let stock_width = marker
                    .material
                    .material()
                    .properties
                    .get("width")
                    .ok_or_else(|| bad("knife program needs the material's source width"))?
                    .bounds
                    .map_or(marker.width_m, |bounds| bounds[1])
                    * 1000.0;
                let length = marker.length_m * 1000.0;
                if !positive(width)
                    || !positive(stock_width)
                    || !positive(length)
                    || !on_bed(origin, machine)
                    || !on_bed(origin + Vec2::new(stock_width, length), machine)
                {
                    return Err(bad(
                        "complete roll marker exceeds the declared knife bed; repartition the marker explicitly",
                    ));
                }
                Ok((
                    Envelope {
                        size_mm: [width, length],
                        origin_mm: origin,
                        offset: loading.map(|_| origin),
                    },
                    None,
                    receipt(origin, [stock_width, length]),
                    if loading.is_some() {
                        digest(&(&marker.placements, origin))?
                    } else {
                        digest(&marker.placements)?
                    },
                ))
            }
            Self::Finite(_, marker, original) => {
                let b = bbox(&marker.outline_mm)?;
                let offset = if loading.is_some() {
                    origin - Vec2::new(b[0], b[1])
                } else {
                    Vec2::new(-b[0], -b[1])
                };
                let size_mm = [b[2] - b[0], b[3] - b[1]];
                if size_mm.iter().any(|&v| !positive(v))
                    || !on_bed(origin, machine)
                    || !on_bed(origin + Vec2::new(size_mm[0], size_mm[1]), machine)
                {
                    return Err(bad(
                        "complete finite stock exceeds the declared knife bed; a fitting cut-piece subset cannot shrink the physical hide or remnant",
                    ));
                }
                // Stock validation/nesting already checked actual polygon exclusions. Loading
                // translates that complete inventory, including defects and earlier cuts.
                for p in marker
                    .outline_mm
                    .iter()
                    .chain(marker.defects_mm.iter().flatten())
                    .chain(marker.occupied_mm.iter().flatten())
                    .chain(marker.zones.iter().flat_map(|z| z.outline_mm.iter()))
                {
                    work.spend(1).ok_or_else(budget)?;
                    if !on_bed(*p + offset, machine) {
                        return Err(bad("finite-stock loading transform exceeds the knife bed"));
                    }
                }
                let mut source_to_machine = marker.source_to_marker;
                source_to_machine[0][2] += offset.x;
                source_to_machine[1][2] += offset.y;
                if source_to_machine.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(bad("finite-stock loading transform overflows"));
                }
                let identity = FiniteStock {
                    stock_id: marker.id.clone(),
                    stock_sha256: digest(original)?,
                    marker_to_machine_offset_mm: offset,
                    source_to_machine,
                };
                let geometry_sha256 = digest(&(
                    marker.source_to_marker,
                    &marker.outline_mm,
                    &marker.defects_mm,
                    &marker.occupied_mm,
                    &marker.zones,
                    &marker.placements,
                    offset,
                ))?;
                Ok((
                    Envelope {
                        size_mm,
                        origin_mm: origin,
                        offset: Some(offset),
                    },
                    Some(identity),
                    receipt(offset, size_mm),
                    geometry_sha256,
                ))
            }
        }
    }
}

fn edges(path: &[Vec2], closed: bool) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    path.iter()
        .copied()
        .zip(path.iter().copied().cycle().skip(1))
        .take(if closed {
            path.len()
        } else {
            path.len().saturating_sub(1)
        })
}

fn simple_path(points: &[Vec2], closed: bool, work: &mut geometry::Work) -> Result<(), Refusal> {
    if closed {
        return geometry_result(
            geometry::simple(points, work),
            "knife cut ring is not simple",
        );
    }
    for (i, (a, b)) in edges(points, false).enumerate() {
        for (j, (c, d)) in edges(points, false).enumerate().skip(i + 1) {
            work.spend(1).ok_or_else(budget)?;
            if j == i + 1 {
                let u = b - a;
                let v = d - c;
                if u.cross(v) == 0.0 && u.dot(v) <= 0.0 {
                    return Err(bad("knife slit reverses along its own cut"));
                }
            } else if crate::outline::segments_touch(a, b, c, d) {
                return Err(bad("knife slit intersects itself"));
            }
        }
    }
    Ok(())
}

fn paths<'a>(
    placements: &'a [CutPlacement],
    envelope: Envelope,
    machine: &Machine,
    work: &mut geometry::Work,
) -> Result<Vec<Path<'a>>, Refusal> {
    let mut paths = Vec::new();
    let drill_count = placements
        .iter()
        .fold(0usize, |n, p| n.saturating_add(p.drills_mm.len()));
    if drill_count > MAX_PATHS {
        return Err(budget());
    }
    let mut point_count = drill_count;
    // One complete internal phase preserves every part's exterior support until all features finish.
    for internal_phase in [true, false] {
        for (placement, piece) in placements.iter().enumerate() {
            let sources: Vec<_> = if internal_phase {
                piece
                    .internal_cut_mm
                    .iter()
                    .enumerate()
                    .map(|(i, (p, c))| (Some(i), p.as_slice(), *c))
                    .collect()
            } else {
                vec![(None, piece.cut_mm.as_slice(), true)]
            };
            for (internal_cut, points, closed) in sources {
                // An explicit repeated closing endpoint describes the same closed path, not a zero-length cut.
                let points = if closed && points.len() > 1 && points.first() == points.last() {
                    &points[..points.len() - 1]
                } else {
                    points
                };
                point_count = point_count.checked_add(points.len()).ok_or_else(budget)?;
                if point_count > MAX_POINTS || paths.len().saturating_add(drill_count) >= MAX_PATHS
                {
                    return Err(budget());
                }
                if points.len() < if closed { 3 } else { 2 }
                    || points.iter().any(|&p| {
                        let loaded = envelope.offset.map_or(p, |d| p + d);
                        !on_bed(loaded, machine)
                            || loaded.x < envelope.origin_mm.x
                            || loaded.y < envelope.origin_mm.y
                            || loaded.x > envelope.origin_mm.x + envelope.size_mm[0]
                            || loaded.y > envelope.origin_mm.y + envelope.size_mm[1]
                    })
                    || edges(points, closed).any(|(a, b)| !positive(a.dist(b)))
                {
                    return Err(bad(
                        "knife paths must be finite, nondegenerate and inside the admitted marker/bed",
                    ));
                }
                simple_path(points, closed, work)?;
                if let Some(clearance) = &machine.fixture_clearance {
                    for (a, b) in edges(points, closed) {
                        let load = |p| envelope.offset.map_or(p, |d| p + d);
                        clearance.check_segment([load(a), load(b)], true, work)?;
                    }
                }
                if internal_cut.is_some() {
                    geometry_result(
                        geometry::path_inside(points, closed, &piece.cut_mm, work),
                        "knife internal path must stay strictly inside its complete exterior",
                    )?;
                }
                let length = edges(points, closed).map(|(a, b)| a.dist(b)).sum::<f64>();
                if !positive(length) {
                    return Err(bad("knife path length overflows"));
                }
                paths.push(Path {
                    reference: PathRef {
                        placement,
                        internal_cut,
                    },
                    points,
                    offset: envelope.offset,
                    closed,
                    length,
                });
            }
        }
    }
    if paths.is_empty() {
        return Err(bad("knife program needs a nonempty marker"));
    }
    // Crossing/nested voids would require a separate removal/support model. Do not guess one.
    for (i, a) in paths
        .iter()
        .enumerate()
        .filter(|(_, p)| p.reference.internal_cut.is_some())
    {
        for b in paths.iter().skip(i + 1).filter(|p| {
            p.reference.internal_cut.is_some() && p.reference.placement == a.reference.placement
        }) {
            work.spend(a.points.len().saturating_add(b.points.len()))
                .ok_or_else(budget)?;
            if (a.closed && point_in_polygon(b.points[0], a.points))
                || (b.closed && point_in_polygon(a.points[0], b.points))
            {
                return Err(bad(
                    "nested knife cutouts or slits in removed regions need an explicit support/removal model",
                ));
            }
            for (a0, a1) in edges(a.points, a.closed) {
                for (b0, b1) in edges(b.points, b.closed) {
                    work.spend(1).ok_or_else(budget)?;
                    if crate::outline::segments_touch(a0, a1, b0, b1) {
                        return Err(bad(
                            "intersecting knife features need an explicit support/removal model",
                        ));
                    }
                }
            }
        }
    }
    Ok(paths)
}

fn drills(
    source: &MarkerSource<'_>,
    envelope: Envelope,
    machine: &Machine,
    paths: &[Path<'_>],
    work: &mut geometry::Work,
) -> Result<Vec<DrillRun>, Refusal> {
    let placements = source.placements();
    if placements.iter().all(|p| p.drills_mm.is_empty()) {
        return Ok(Vec::new());
    }
    let tool = machine
        .drilling
        .as_ref()
        .ok_or_else(|| bad("declared drills require an explicit qualified fixed drill tool"))?;
    let radius = tool.diameter_mm * 0.5;
    let load = |p| envelope.offset.map_or(p, |d| p + d);
    let rectangle = |origin: Vec2, size: [f64; 2]| {
        [
            origin,
            origin + Vec2::new(size[0], 0.0),
            origin + Vec2::new(size[0], size[1]),
            origin + Vec2::new(0.0, size[1]),
        ]
    };
    let bed = rectangle(Vec2::ZERO, machine.bed_mm);
    let stock_box = rectangle(envelope.origin_mm, envelope.size_mm);
    let mut loaded_paths = Vec::with_capacity(paths.len());
    let mut exteriors = vec![None; placements.len()];
    let mut internal_paths = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        work.spend(path.points.len()).ok_or_else(budget)?;
        loaded_paths.push(path.points.iter().copied().map(load).collect::<Vec<_>>());
        if path.reference.internal_cut.is_some() {
            internal_paths.push(index);
        } else {
            exteriors[path.reference.placement] = Some(index);
        }
    }
    let mut stock_outline = Vec::new();
    let mut exclusions = Vec::new();
    if let MarkerSource::Finite(_, marker, _) = source {
        work.spend(marker.outline_mm.len()).ok_or_else(budget)?;
        stock_outline = marker.outline_mm.iter().copied().map(load).collect();
        for outline in marker.defects_mm.iter().chain(&marker.occupied_mm) {
            work.spend(outline.len()).ok_or_else(budget)?;
            exclusions.push(outline.iter().copied().map(load).collect::<Vec<_>>());
        }
    }
    let mut result: Vec<DrillRun> = Vec::new();
    let mut position = machine.start_mm;
    for (placement, piece) in placements.iter().enumerate() {
        for (drill, &(point, diameter)) in piece.drills_mm.iter().enumerate() {
            work.spend(1).ok_or_else(budget)?;
            let center = load(point);
            if !center.is_finite() || !positive(diameter) || diameter != tool.diameter_mm {
                return Err(bad(
                    "every transformed drill hole needs its explicit finite centre and the exact fixed tool diameter",
                ));
            }
            let exterior = exteriors[placement]
                .ok_or_else(|| bad("drill hole lost its complete exterior path"))?;
            for boundary in [
                loaded_paths[exterior].as_slice(),
                bed.as_slice(),
                stock_box.as_slice(),
            ] {
                geometry_result(
                    geometry::strict_disk_inside(center, radius, boundary, work),
                    "complete drill hole must stay strictly inside its exterior, stock and machine bed",
                )?;
            }
            if !stock_outline.is_empty() {
                geometry_result(
                    geometry::strict_disk_inside(center, radius, &stock_outline, work),
                    "complete drill hole must stay strictly inside the actual finite-stock outline",
                )?;
            }
            for exclusion in &exclusions {
                geometry_result(
                    geometry::disk_clear_path(center, radius, exclusion, true, work),
                    "drill hole intersects a finite-stock defect or previously occupied region",
                )?;
            }
            for &index in &internal_paths {
                geometry_result(
                    geometry::disk_clear_path(
                        center,
                        radius,
                        &loaded_paths[index],
                        paths[index].closed,
                        work,
                    ),
                    "drill hole touches an internal cut or lies in a removed closed void",
                )?;
            }
            for previous in &result {
                geometry_result(
                    geometry::equal_disks_clear(center, previous.center_mm, radius, work),
                    "drill holes overlap, touch or cannot be certified disjoint",
                )?;
            }
            if let Some(clearance) = &machine.fixture_clearance {
                clearance.check_segment([center, center], true, work)?;
            }
            result.push(DrillRun {
                placement,
                drill,
                piece: piece.piece.clone(),
                instance: piece.instance.clone(),
                copy: piece.copy,
                center_mm: center,
                diameter_mm: diameter,
                travel_mm: [position, center],
                travel_waypoints_mm: Vec::new(),
            });
            position = center;
        }
    }
    Ok(result)
}

fn traversal(
    placements: &[CutPlacement],
    paths: &[Path<'_>],
    order: &[usize],
    machine: &Machine,
    feed: f64,
    drills: &[DrillRun],
    drill_cycle_seconds: f64,
) -> Result<Traversal, Refusal> {
    let mut position = machine.start_mm;
    let mut travel_length_mm = 0.0;
    for drill in drills {
        travel_length_mm += position.dist(drill.center_mm);
        position = drill.center_mm;
    }
    let mut cut_length_mm = 0.0;
    let mut cutting_seconds = 0.0;
    let mut runs = Vec::with_capacity(order.len());
    for &i in order {
        let path = &paths[i];
        let piece = &placements[path.reference.placement];
        travel_length_mm += position.dist(path.start());
        cut_length_mm += path.length;
        if machine.motion_profile.is_some() {
            cutting_seconds += machine.cutting_seconds(path.points, path.closed, feed)?;
        }
        runs.push(Run {
            path: path.reference.clone(),
            piece: piece.piece.clone(),
            instance: piece.instance.clone(),
            copy: piece.copy,
            closed: path.closed,
            travel_mm: [position, path.start()],
            travel_waypoints_mm: Vec::new(),
            cut_endpoints_mm: [path.start(), path.end()],
            cut_length_mm: path.length,
        });
        position = path.end();
    }
    let return_mm = machine
        .return_to_start
        .then_some([position, machine.start_mm]);
    if let Some([a, b]) = return_mm {
        travel_length_mm += a.dist(b);
    }
    if machine.motion_profile.is_none() {
        cutting_seconds = cut_length_mm / feed;
    }
    let travel_seconds = travel_length_mm / machine.travel_mm_s;
    let lift_lower_seconds = order.len() as f64 * machine.lift_lower_seconds;
    let (drilling_seconds, tool_change_seconds) = if drills.is_empty() {
        (0.0, 0.0)
    } else {
        let tool = machine
            .drilling
            .as_ref()
            .ok_or_else(|| bad("drill traversal needs its fixed tool"))?;
        let seconds = drills.len() as f64 * drill_cycle_seconds;
        if !positive(seconds) {
            return Err(bad("complete drilling phase time cannot be represented"));
        }
        (
            seconds,
            tool.select_drill_seconds + tool.select_knife_seconds,
        )
    };
    let mut modeled_path_seconds = cutting_seconds + travel_seconds + lift_lower_seconds;
    if !drills.is_empty() {
        modeled_path_seconds += drilling_seconds + tool_change_seconds;
    }
    if !modeled_path_seconds.is_finite()
        || !travel_length_mm.is_finite()
        || !cut_length_mm.is_finite()
    {
        return Err(bad("knife timing/length arithmetic overflows"));
    }
    let mut traversal = Traversal {
        drills: drills.to_vec(),
        runs,
        return_mm,
        return_waypoints_mm: Vec::new(),
        cut_length_mm,
        travel_length_mm,
        cutting_seconds,
        travel_seconds,
        lift_lower_seconds,
        drilling_seconds,
        tool_change_seconds,
        modeled_path_seconds,
    };
    if machine.motion_profile.is_some() || !traversal.drills.is_empty() {
        traversal.travel_seconds = machine.travel_seconds(&traversal)?;
        traversal.modeled_path_seconds =
            cutting_seconds + traversal.travel_seconds + lift_lower_seconds;
        if !drills.is_empty() {
            traversal.modeled_path_seconds += drilling_seconds + tool_change_seconds;
        }
        if !traversal.modeled_path_seconds.is_finite() {
            return Err(bad("knife exact-stop timing arithmetic overflows"));
        }
    }
    Ok(traversal)
}

fn nearest(paths: &[Path<'_>], start: Vec2, work: &mut geometry::Work) -> Option<Vec<usize>> {
    let mut order = Vec::with_capacity(paths.len());
    let mut used = vec![false; paths.len()];
    let mut position = start;
    for internal in [true, false] {
        let remaining = paths
            .iter()
            .filter(|p| p.reference.internal_cut.is_some() == internal)
            .count();
        for _ in 0..remaining {
            let mut best: Option<(usize, f64)> = None;
            for (i, path) in paths
                .iter()
                .enumerate()
                .filter(|(i, p)| !used[*i] && p.reference.internal_cut.is_some() == internal)
            {
                work.spend(1)?;
                let distance = position.dist(path.start());
                // Equal distances retain the original stable placement/feature order.
                if best.is_none_or(|(_, old)| distance < old) {
                    best = Some((i, distance));
                }
            }
            let i = best?.0;
            used[i] = true;
            order.push(i);
            position = paths[i].end();
        }
    }
    Some(order)
}

/// Build complete source-qualified programs from the exact final roll markers.
/// Ordinary [`CutPlan::build`] calls this automatically when `options.program` is present.
///
/// # Errors
/// Missing/mismatched knife or drill evidence, unsupported material state, unqualified thickness,
/// invalid/oversized geometry, or exhaustion of the fixed geometric validation budget.
pub(super) fn build(plan: &CutPlan, policy: &Policy) -> Result<Report, Refusal> {
    if policy.stock_loading.len() > 4096
        || policy
            .stock_loading
            .iter()
            .any(|loading| loading.marker >= plan.rolls.len())
    {
        return Err(bad("stock loading names an unknown roll marker"));
    }
    let sources = plan
        .rolls
        .iter()
        .enumerate()
        .map(|(i, marker)| MarkerSource::Roll(i, marker))
        .collect::<Vec<_>>();
    build_markers(&sources, policy, false)
}

pub(super) fn build_finite(plan: &stock::Plan, policy: &Policy) -> Result<Report, Refusal> {
    if plan.markers().len() != plan.request().stocks.len() {
        return Err(bad(
            "knife program lost finite-stock inventory correspondence",
        ));
    }
    if policy.stock_loading.len() > 4096
        || policy
            .stock_loading
            .iter()
            .any(|loading| loading.marker >= plan.markers().len())
    {
        return Err(bad("stock loading names an unknown finite-stock marker"));
    }
    let sources = plan
        .markers()
        .iter()
        .zip(&plan.request().stocks)
        .enumerate()
        .filter(|(_, (marker, _))| !marker.placements.is_empty())
        .map(|(i, (marker, original))| MarkerSource::Finite(i, marker, original))
        .collect::<Vec<_>>();
    build_markers(&sources, policy, true)
}

fn build_markers(
    sources: &[MarkerSource<'_>],
    policy: &Policy,
    finite: bool,
) -> Result<Report, Refusal> {
    let (machine_sha256, recipes) = policy.validate()?;
    let mut geometry_work = geometry::Work::new(policy.geometry_work_limit);
    geometry_work
        .spend(policy.stock_loading.len())
        .ok_or_else(budget)?;
    let loading: BTreeMap<_, _> = policy
        .stock_loading
        .iter()
        .map(|item| (item.marker, item))
        .collect();
    if let Some(clearance) = &policy.machine.fixture_clearance {
        clearance.validate(&mut geometry_work)?;
    }
    let mut search_work = geometry::Work::new(policy.work_limit);
    let corners = policy
        .travel_routing
        .as_ref()
        .map(|routing| {
            routing.nodes(
                &policy.machine,
                policy
                    .machine
                    .fixture_clearance
                    .as_ref()
                    .expect("routing policy requires fixtures"),
                &mut search_work,
                &mut geometry_work,
            )
        })
        .transpose()?;
    let mut markers = Vec::with_capacity(sources.len());
    if sources.is_empty() || sources.len() > 4096 {
        return Err(budget());
    }
    for source in sources {
        let material = source.material();
        let q = recipes.get(material.sha256()).ok_or_else(|| bad(format!("material {} has no exact knife process qualification; material substitution must requalify cutting", material.material().id)))?;
        let thickness = material.unloaded_thickness()?;
        let thickness_mm = thickness
            .bounds
            .unwrap_or([thickness.value, thickness.value])
            .map(|v| v * 1000.0);
        if thickness_mm.iter().any(|&v| !positive(v))
            || thickness_mm[0] < q.thickness_mm[0]
            || thickness_mm[1] > q.thickness_mm[1]
            || thickness_mm[1] > policy.machine.maximum_thickness_mm
        {
            return Err(bad(
                "full source thickness range exceeds the knife process or machine envelope",
            ));
        }
        if !loading.is_empty() {
            geometry_work.spend(1).ok_or_else(budget)?;
        }
        let (envelope, finite_stock, loading, geometry_sha256) = source.frame(
            &policy.machine,
            loading.get(&source.index()).copied(),
            &mut geometry_work,
        )?;
        let placements = source.placements();
        let has_drills = placements.iter().any(|p| !p.drills_mm.is_empty());
        let drill_qualification = if has_drills {
            let tool =
                policy.machine.drilling.as_ref().ok_or_else(|| {
                    bad("declared drills require an explicit fixed drilling tool")
                })?;
            let observation = q.drilling.as_ref().ok_or_else(|| {
                bad("declared drills require their own source-qualified drilling cycle")
            })?;
            if thickness_mm[0] < observation.thickness_mm[0]
                || thickness_mm[1] > observation.thickness_mm[1]
                || thickness_mm[1] > tool.maximum_thickness_mm
            {
                return Err(bad(
                    "full source thickness range exceeds the drill observation or fixed tool envelope",
                ));
            }
            Some(observation)
        } else {
            None
        };
        let drill_cycle_seconds =
            drill_qualification.map_or(Ok(0.0), DrillQualification::cycle_seconds)?;
        let paths = paths(placements, envelope, &policy.machine, &mut geometry_work)?;
        let drills = drills(
            source,
            envelope,
            &policy.machine,
            &paths,
            &mut geometry_work,
        )?;
        let knife_start = drills
            .last()
            .map_or(policy.machine.start_mm, |d| d.center_mm);
        let baseline_order: Vec<_> = (0..paths.len()).collect();
        let mut baseline = traversal(
            placements,
            &paths,
            &baseline_order,
            &policy.machine,
            q.feed_mm_s,
            &drills,
            drill_cycle_seconds,
        )?;
        admit_travel(
            &mut baseline,
            &policy.machine,
            corners.as_deref(),
            &mut search_work,
            &mut geometry_work,
        )?;
        let mut candidate = if policy.ordering == Ordering::NearestPath {
            nearest(&paths, knife_start, &mut search_work)
                .map(|order| {
                    traversal(
                        placements,
                        &paths,
                        &order,
                        &policy.machine,
                        q.feed_mm_s,
                        &drills,
                        drill_cycle_seconds,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let mut candidate_clearance_refusal = None;
        if let Some(route) = &mut candidate
            && let Err(refusal) = admit_travel(
                route,
                &policy.machine,
                corners.as_deref(),
                &mut search_work,
                &mut geometry_work,
            )
        {
            if refusal.code == RefusalCode::BudgetExceeded
                && !(search_work.exhausted && !geometry_work.exhausted)
            {
                return Err(refusal);
            }
            candidate_clearance_refusal = Some(refusal);
            candidate = None;
        }
        let improved = candidate.as_ref().is_some_and(|c| {
            if policy.machine.motion_profile.is_some() {
                c.modeled_path_seconds < baseline.modeled_path_seconds
            } else {
                c.travel_length_mm < baseline.travel_length_mm
            }
        });
        let selected = if improved {
            candidate.expect("checked candidate")
        } else {
            baseline.clone()
        };
        markers.push(Marker {
            marker: source.index(),
            finite_stock,
            loading,
            material_sha256: material.sha256().into(),
            geometry_sha256,
            qualification: q.id.clone(),
            drill_qualification: drill_qualification.map(|d| d.id.clone()),
            thickness_mm,
            baseline,
            selected,
            improved,
            candidate_clearance_refusal,
        });
    }
    let uses_drilling = markers.iter().any(|m| m.drill_qualification.is_some());
    Ok(Report {
        schema: if finite {
            "ftx-knife-cut-program/2"
        } else {
            "ftx-knife-cut-program/1"
        },
        machine_sha256,
        fixture_clearance: policy.machine.fixture_clearance.clone(),
        motion_profile: policy.machine.motion_profile.clone(),
        drilling: policy.machine.drilling.clone(),
        travel_routing: policy.travel_routing.clone(),
        stock_loading: policy.stock_loading.clone(),
        markers,
        search_work: search_work.used,
        geometry_work: geometry_work.used,
        budget_exhausted: search_work.exhausted,
        optimality_proven: false,
        physical_cutting_validated: false,
        hardware_execution_authorized: false,
        limitations: {
            let mut limits = vec![
                "Single-ply cold-knife planning uses exact final marker paths. All internal paths finish before exterior releases; paths retain their authored start, direction and closing edge.",
                "Source qualifications are exact material and full machine locks. A changed textile, finish, thickness, blade, bed or timing configuration requires new applicable process evidence.",
                if policy.machine.motion_profile.is_some() {
                    "Modeled time uses the source-qualified symmetric acceleration/braking profile and speed caps, with zero speed at every retained cut/travel vertex, including collinear authored vertices, detours and the final return. Triangular short moves never attain their speed cap. Lift/lower cycles are separate. No controller blending, jerk bound, blade turning, loading, workholding change or inspection time is inferred."
                } else if uses_drilling {
                    "Modeled time includes constant-rate knife cutting, every retained raised-travel chord, knife lift/lower cycles, complete qualified drill cycles and both in-place tool selections. Acceleration, corner slowing, blade turning, loading, workholding changes and inspection are not inferred."
                } else if policy.travel_routing.is_some() {
                    "Modeled time includes only constant-rate cutting, every retained raised-travel chord and explicit lift/lower cycles. Acceleration, corner slowing, blade turning, loading, workholding changes and inspection are not inferred."
                } else {
                    "Modeled time includes only constant-rate cutting, raised-tool straight travel and explicit lift/lower cycles. Acceleration, corner slowing, blade turning, loading, workholding changes and inspection are not inferred."
                },
                if uses_drilling {
                    "No kerf compensation, common-line cutting, thermal processing, three-dimensional tool/body collision check, measured hole or cut quality, physical cycle-time claim or hardware execution authorization."
                } else {
                    "No kerf compensation, common-line cutting, drill operation, thermal processing, three-dimensional tool/body collision check, measured cut quality, physical cycle-time claim or hardware execution authorization."
                },
                if policy.machine.motion_profile.is_some() {
                    "Nearest-path ordering is a bounded heuristic; an unfinished candidate or one without strictly lower modeled time retains the complete authored baseline. Raised detours minimize exact-stop time within their finite checked graph, without a general fastest-path proof."
                } else {
                    "Nearest-path ordering is a bounded heuristic; an unfinished or longer candidate retains the complete authored baseline, without a shortest-path proof."
                },
            ];
            if uses_drilling {
                limits.push("One fixed independently qualified coaxial cold through-drill completes all explicit holes in authored placement/hole order before knife internals and exterior releases. Every hole has the exact tool diameter and lies strictly inside usable stock and its exterior, clear of other holes, cuts and removed voids. Modeled time includes each complete plunge/dwell/retract cycle, both in-place tool selections once per used marker, and every raised drill/knife/return chord; knife lift/lower time is not charged again for holes. No bit changes, spindle dynamics, backing adequacy, chip evacuation, workholding registration or physical hole quality is inferred.");
            }
            if policy.machine.fixture_clearance.is_some() {
                if policy.travel_routing.is_some() {
                    limits.push("Raised moves use checked straight links through a conservative expanded fixture-box corner graph, including the final return. The explicit waypoint offset chooses candidate vertices, not a new physical clearance margin. Every retained chord passes the original exact tool/fixture sweep; cutting paths, starts and directions remain unchanged. Failure to find a complete checked route refuses that candidate and proves no general infeasibility or globally shortest path. No fixture motion, grip strength, registration or 3D clearance is inferred.");
                } else {
                    limits.push("Declared fixed machine-frame fixtures are checked against complete cutting and raised-travel circular envelopes, including the final return. Touching and numerically unresolved strict clearance refuse, without adding a physical margin. The complete authored baseline must clear every fixture; a blocked ordering candidate falls back to that checked baseline. No automatic routing, fixture motion, grip strength, registration or 3D clearance is inferred.");
                }
            } else {
                limits.push(
                    "No fixture geometry was supplied; tool/fixture clearance was not evaluated.",
                );
            }
            if !policy.stock_loading.is_empty() {
                limits.push("Explicit stock loading translates the complete stock envelope, unchanged cuts and annotations to the authored lower-left machine position. Unlisted markers retain origin placement; unused finite stocks create no toolpaths. No physical registration accuracy, loading time, grip adequacy or workholding qualification is inferred.");
            }
            if finite && policy.stock_loading.is_empty() {
                limits.push("Each used physical stock item is independently loaded with its complete marker bounding-box lower-left corner at the bed origin. The reported translation preserves source orientation, all defects and prior occupied regions; stock boundaries, defects, occupied areas and zones are not cutting paths. This modeled loading position is not measured machine registration or workholding qualification.");
            }
            limits
        },
    })
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    use serde_json::json;

    fn machine() -> Machine {
        Machine {
            id: "synthetic-motion".into(),
            blade: "synthetic-knife".into(),
            bed_mm: [100.0, 100.0],
            maximum_thickness_mm: 1.0,
            maximum_feed_mm_s: 10.0,
            travel_mm_s: 10.0,
            lift_lower_seconds: 0.0,
            start_mm: Vec2::new(0.0, 0.0),
            return_to_start: true,
            fixture_clearance: None,
            motion_profile: Some(MotionProfile {
                model: "trapezoidal_exact_stop/1".into(),
                cutting_acceleration_mm_s2: 4.0,
                travel_acceleration_mm_s2: 4.0,
            }),
            drilling: None,
        }
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= 1e-12, "{actual} != {expected}");
    }

    #[test]
    fn source_drills_preserve_exact_fold_self_images_but_refuse_lost_holes() {
        let document = json!({
            "schema":"fabric-pattern/1","units":"mm","meta":{"sa_mm":0},
            "pieces":[{"id":"folded","cut":{"qty":1,"on_fold":"left"},
                "vertices":{"a":[0,0],"b":[10,0],"c":[10,20],"d":[0,20]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "drills":[{"p":[0,10],"d":2},{"p":[2,5],"d":2}]}],"seams":[]
        });
        let pattern = Pattern::new(document.clone());
        assert_eq!(pattern.instances.len(), 1);
        let indices = BTreeSet::from([0]);
        validate_source(&pattern, &indices).unwrap();
        for holes in [
            json!([{"p":[0,10],"d":2},{"p":[0,10],"d":2}]),
            json!([{"p":[2,5],"d":2},{"p":[-2,5],"d":2}]),
            json!([{"p":[0.00000001,10],"d":2}]),
            json!([{"p":[2,5]}]),
            json!([{"p":[2,5],"d":0}]),
            json!([{"p":[2,5,7],"d":2}]),
        ] {
            let mut altered = document.clone();
            altered["pieces"][0]["drills"] = holes;
            assert!(validate_source(&Pattern::new(altered), &indices).is_err());
        }
        assert_eq!(
            validate_distinct_drill_sources(vec![Vec2::ZERO; MAX_PATHS + 1])
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn independent_drill_cycle_rejects_invalid_phases_and_overflow() {
        let mut observation: DrillQualification = serde_json::from_value(json!({
            "id":"synthetic-drill","thickness_mm":[0.5,1.5],
            "plunge_seconds":0.3,"dwell_seconds":0.2,"retract_seconds":0.5,
            "source":{"reference":"test:drill","accessed_on":"2026-11-02","note":"Synthetic cycle control"},
            "method":"Complete raised-to-raised cold through-hole cycle",
            "evidence":"estimate","allow_estimates":true
        })).unwrap();
        near(observation.cycle_seconds().unwrap(), 1.0);
        observation.dwell_seconds = 0.0;
        near(observation.cycle_seconds().unwrap(), 0.8);
        for time in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            observation.plunge_seconds = time;
            assert!(observation.cycle_seconds().is_err());
        }
        observation.plunge_seconds = f64::MAX;
        observation.retract_seconds = f64::MAX;
        assert!(observation.cycle_seconds().is_err());
    }

    #[test]
    fn exact_stop_drill_travel_includes_detours_first_knife_link_and_return() {
        let start = Vec2::ZERO;
        let a = Vec2::new(3., 0.);
        let b = Vec2::new(6., 0.);
        let cut = Vec2::new(9., 0.);
        let mut route = Traversal {
            drills: vec![
                DrillRun {
                    placement: 0,
                    drill: 0,
                    piece: "test".into(),
                    instance: "test".into(),
                    copy: 0,
                    center_mm: a,
                    diameter_mm: 2.,
                    travel_mm: [start, a],
                    travel_waypoints_mm: vec![Vec2::new(1., 0.)],
                },
                DrillRun {
                    placement: 0,
                    drill: 1,
                    piece: "test".into(),
                    instance: "test".into(),
                    copy: 0,
                    center_mm: b,
                    diameter_mm: 2.,
                    travel_mm: [a, b],
                    travel_waypoints_mm: vec![],
                },
            ],
            runs: vec![Run {
                path: PathRef {
                    placement: 0,
                    internal_cut: None,
                },
                piece: "test".into(),
                instance: "test".into(),
                copy: 0,
                closed: true,
                travel_mm: [b, cut],
                travel_waypoints_mm: vec![],
                cut_endpoints_mm: [cut, cut],
                cut_length_mm: 1.,
            }],
            return_mm: Some([cut, start]),
            return_waypoints_mm: vec![],
            cut_length_mm: 1.,
            travel_length_mm: 18.,
            cutting_seconds: 0.,
            travel_seconds: 0.,
            lift_lower_seconds: 0.,
            drilling_seconds: 2.,
            tool_change_seconds: 1.,
            modeled_path_seconds: 0.,
        };
        near(route.checked_travel_length().unwrap(), 18.);
        // a=4 makes these triangular chords take sqrt(length): 1 + sqrt2 + sqrt3 + sqrt3 + 3.
        near(
            machine().travel_seconds(&route).unwrap(),
            4. + 2_f64.sqrt() + 2. * 3_f64.sqrt(),
        );
        route.runs[0].travel_mm[0] = start;
        assert!(route.checked_travel_length().is_err());
        route.runs[0].travel_mm[0] = b;
        route.drills[1].travel_mm[0] = start;
        assert!(route.checked_travel_length().is_err());
        route.drills[1].travel_mm[0] = a;
        route.return_mm = Some([cut, a]);
        assert!(route.checked_travel_length().is_err());
    }

    #[test]
    fn triangular_threshold_and_trapezoidal_moves_have_known_times() {
        // At a=4, a 9 mm move reaches 6 mm/s in 1.5 s and brakes for another 1.5 s.
        near(exact_stop_seconds(9.0, 10.0, 4.0).unwrap(), 3.0);
        // Reaching 10 mm/s consumes 12.5 mm at each end. The threshold has no cruise.
        near(exact_stop_seconds(25.0, 10.0, 4.0).unwrap(), 5.0);
        // Another 20 mm at 10 mm/s adds two seconds to the five-second ramp/brake pair.
        near(exact_stop_seconds(45.0, 10.0, 4.0).unwrap(), 7.0);
        near(exact_stop_seconds(0.0, 10.0, 4.0).unwrap(), 0.0);
        assert!(exact_stop_seconds(25.0_f64.next_down(), 10.0, 4.0).unwrap() <= 5.0);
        assert!(exact_stop_seconds(25.0_f64.next_up(), 10.0, 4.0).unwrap() >= 5.0);
    }

    #[test]
    fn finite_positive_moves_do_not_disappear_through_intermediate_arithmetic() {
        let very_short = exact_stop_seconds(f64::from_bits(1), 1.0, 1e308).unwrap();
        assert!(very_short.is_finite() && very_short > 0.0);
        let high_cap = exact_stop_seconds(1.0, 1e308, 1e308).unwrap();
        assert!((high_cap / 2e-154 - 1.0).abs() < 1e-14);
        for (length, speed, acceleration) in [
            (f64::MAX, 1e-308, 1.0),
            (f64::MAX, f64::MAX, f64::from_bits(1)),
            (1.0, 1e-300, 1e308), // positive ramp time underflows
            (f64::INFINITY, 1.0, 1.0),
            (1.0, f64::NAN, 1.0),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 1.0),
        ] {
            assert!(exact_stop_seconds(length, speed, acceleration).is_err());
        }
    }

    #[test]
    fn closing_edge_is_counted_once_and_authored_collinear_vertices_remain_stops() {
        let machine = machine();
        let square = [
            Vec2::new(0.0, 0.0),
            Vec2::new(9.0, 0.0),
            Vec2::new(9.0, 9.0),
            Vec2::new(0.0, 9.0),
        ];
        near(machine.cutting_seconds(&square, true, 10.0).unwrap(), 12.0);
        let mut repeated = square.to_vec();
        repeated.push(square[0]);
        assert_eq!(
            machine.cutting_seconds(&square, true, 10.0).unwrap(),
            machine.cutting_seconds(&repeated, true, 10.0).unwrap()
        );
        let mut subdivided = square.to_vec();
        subdivided.insert(1, Vec2::new(4.5, 0.0));
        near(
            machine.cutting_seconds(&subdivided, true, 10.0).unwrap(),
            9.0 + 3.0 * 2.0_f64.sqrt(),
        );
        let translated: Vec<_> = square.iter().map(|p| *p + Vec2::new(20.0, 30.0)).collect();
        near(
            machine.cutting_seconds(&translated, true, 10.0).unwrap(),
            12.0,
        );
    }

    #[test]
    fn raised_timing_includes_every_detour_chord_and_the_complete_return() {
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(9.0, 0.0);
        let mut route = Traversal {
            drills: Vec::new(),
            runs: vec![Run {
                path: PathRef {
                    placement: 0,
                    internal_cut: None,
                },
                piece: "synthetic".into(),
                instance: "synthetic".into(),
                copy: 0,
                closed: true,
                travel_mm: [start, end],
                travel_waypoints_mm: vec![Vec2::new(3.0, 0.0), Vec2::new(6.0, 0.0)],
                cut_endpoints_mm: [end, end],
                cut_length_mm: 1.0,
            }],
            return_mm: Some([end, start]),
            return_waypoints_mm: vec![Vec2::new(6.0, 0.0), Vec2::new(3.0, 0.0)],
            cut_length_mm: 1.0,
            travel_length_mm: 18.0,
            cutting_seconds: 0.0,
            travel_seconds: 0.0,
            lift_lower_seconds: 0.0,
            drilling_seconds: 0.0,
            tool_change_seconds: 0.0,
            modeled_path_seconds: 0.0,
        };
        near(
            machine().travel_seconds(&route).unwrap(),
            6.0 * 3.0_f64.sqrt(),
        );
        route.runs[0].travel_waypoints_mm.clear();
        route.return_waypoints_mm.clear();
        near(machine().travel_seconds(&route).unwrap(), 6.0);
        route.runs[0].travel_mm = [start, start];
        route.runs[0].cut_endpoints_mm = [start, start];
        route.return_mm = Some([start, start]);
        near(machine().travel_seconds(&route).unwrap(), 0.0);
    }

    #[test]
    fn complete_nearest_path_candidate_is_rejected_when_shorter_but_slower() {
        let internal: Vec<_> = [(78.0, 88.0), (14.0, 31.0), (90.0, 3.0)]
            .into_iter()
            .map(|(x, y)| {
                json!({"kind":"cut","closed":true,
                    "pts":[[x,y],[x+1.0,y],[x+1.0,y+1.0],[x,y+1.0]]})
            })
            .collect();
        let pattern = Pattern::new(json!({
            "schema":"fabric-pattern/1","units":"mm","meta":{"sa_mm":0},
            "fabric":{"material":{"id":"synthetic-sheet","allow_estimates":true,"record":{
                "id":"synthetic-sheet","name":"Synthetic timing control","form":"woven",
                "properties":{
                    "width":{"value":0.12,"unit":"m","source":"control","evidence":"estimate","method":"Synthetic test"},
                    "thickness":{"value":1.0,"unit":"mm","source":"control","evidence":"estimate","method":"Synthetic test"}
                },
                "sources":{"control":{"reference":"test:motion","accessed_on":"2026-11-01","note":"Synthetic software control; no measured machine or textile"}}
            }}},
            "pieces":[{"id":"sheet","cut":{"qty":1},"grain":[[0,0],[0,10]],
                "vertices":{"a":[0,0],"b":[100,0],"c":[100,100],"d":[0,100]},
                "edges":[{"id":"bottom","from":"a","to":"b"},{"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},{"id":"left","from":"d","to":"a"}],
                "internal":internal}],
            "seams":[]
        }));
        let options = serde_json::from_value(json!({
            "copies":1,"gap_mm":0,"edge_margin_mm":0,"end_margin_mm":0
        }))
        .unwrap();
        let plan = CutPlan::build(&pattern, &options).unwrap();
        let mut machine = machine();
        machine.bed_mm = [120.0, 120.0];
        machine.maximum_feed_mm_s = 1000.0;
        machine.travel_mm_s = 1000.0;
        let mut policy: Policy = serde_json::from_value(json!({
            "model":"qualified_cold_knife_paths/1","machine":machine,
            "qualifications":[{"id":"synthetic-motion","material_sha256":plan.rolls[0].material.sha256(),
                "machine_sha256":machine_sha256(&machine).unwrap(),"thickness_mm":[0.5,1.5],
                "feed_mm_s":1000.0,"directions":"all_in_plane","evidence":"estimate","allow_estimates":true,
                "source":{"reference":"test:motion","accessed_on":"2026-11-01","note":"Synthetic exact-stop timing"},
                "method":"Independently authored synthetic motion control"}],
            "ordering":"nearest_path","work_limit":10000
        })).unwrap();
        let accelerated = build(&plan, &policy).unwrap();
        let marker = &accelerated.markers[0];
        assert!(!marker.improved);
        assert_eq!(
            serde_json::to_value(&marker.selected).unwrap(),
            serde_json::to_value(&marker.baseline).unwrap()
        );
        policy.machine.motion_profile = None;
        policy.qualifications[0].machine_sha256 = machine_sha256(&policy.machine).unwrap();
        let constant = build(&plan, &policy).unwrap();
        let constant_marker = &constant.markers[0];
        assert!(constant_marker.improved);
        assert!(constant_marker.selected.travel_length_mm < marker.baseline.travel_length_mm);
        // The old selection really is faster under the old model but slower when every segment
        // accelerates. The same actual cut paths and complete producer selection are exercised.
        assert!(
            machine.travel_seconds(&constant_marker.selected).unwrap()
                > marker.baseline.travel_seconds + 1.0
        );
        assert_eq!(constant_marker.selected.runs[0].path.internal_cut, Some(1));
        assert_eq!(constant_marker.selected.runs[1].path.internal_cut, Some(2));
        assert_eq!(constant_marker.selected.runs[2].path.internal_cut, Some(0));
    }

    #[test]
    fn checked_fixture_graph_chooses_longer_two_leg_route_when_its_motion_is_faster() {
        let clearance: FixtureClearance = serde_json::from_value(json!({
            "model":"planar_disk_sweep/1","cutting_radius_mm":0.5,"raised_radius_mm":0.5,
            "fixtures":[{"id":"synthetic-clamp","outline_mm":[[40,30],[60,30],[60,70],[40,70]]}],
            "source":{"reference":"test:motion-clamp","accessed_on":"2026-11-01","note":"Synthetic planar geometry"},
            "method":"Declared disk and stationary polygon control","evidence":"estimate","allow_estimates":true
        })).unwrap();
        let mut accelerated = machine();
        accelerated.travel_mm_s = 1000.0;
        let mut constant = accelerated.clone();
        constant.motion_profile = None;
        let routing = TravelRouting {
            model: "fixture_box_visibility/1".into(),
            waypoint_offset_mm: 1.5,
        };
        let mut search = geometry::Work::new(10000);
        let mut geometry = geometry::Work::new(100000);
        clearance.validate(&mut geometry).unwrap();
        let nodes = routing
            .nodes(&accelerated, &clearance, &mut search, &mut geometry)
            .unwrap();
        let endpoints = [Vec2::new(88.0, 79.0), Vec2::new(36.0, 30.0)];
        let shortest = detour(
            endpoints,
            &nodes,
            &constant,
            &clearance,
            &mut search,
            &mut geometry,
        )
        .unwrap();
        let fastest = detour(
            endpoints,
            &nodes,
            &accelerated,
            &clearance,
            &mut search,
            &mut geometry,
        )
        .unwrap();
        assert_eq!(shortest.len(), 2);
        assert_eq!(fastest.len(), 1);
        let metrics = |waypoints: &[Vec2]| {
            let lengths: Vec<_> = raised_segments(
                std::iter::once(endpoints[0])
                    .chain(waypoints.iter().copied())
                    .chain(std::iter::once(endpoints[1])),
            )
            .map(|[a, b]| a.dist(b))
            .collect();
            (
                lengths.iter().sum::<f64>(),
                lengths.iter().map(|length| length.sqrt()).sum::<f64>(),
            )
        };
        let (short_length, short_time) = metrics(&shortest);
        let (fast_length, fast_time) = metrics(&fastest);
        near(short_length, 84.07351425019303);
        near(fast_length, 92.53521432871463);
        // a=4 and the high speed cap make each rest-to-rest leg take sqrt(length) seconds.
        near(short_time, 14.14682063243067);
        near(fast_time, 13.589875720396915);
        assert!(fast_length > short_length && fast_time < short_time);
    }
}

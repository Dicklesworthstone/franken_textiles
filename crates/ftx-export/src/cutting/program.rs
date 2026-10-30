//! Source-qualified cold-knife path planning on the existing final roll/finite-stock geometry.
//! Every internal cut precedes every exterior release. Paths retain their authored start and
//! direction; raised-tool travel may be reordered, but kerf and machine trajectories are not inferred.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, polyline::point_in_polygon, receipt::sha256_hex};
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
    /// Bounded nearest authored start; retain the complete baseline if the candidate travels farther.
    NearestPath,
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
}

/// Optional program policy on ordinary roll or finite-stock cutting options.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `qualified_cold_knife_paths/1`; no laser, hot-wire, drill or inferred kerf model.
    pub model: String,
    /// Admitted machine and complete modeled timing configuration.
    pub machine: Machine,
    /// One unambiguous recipe per material; independently qualified unused alternatives may remain.
    pub qualifications: Vec<Qualification>,
    /// Default authored order, or bounded nearest-path heuristic.
    #[serde(default)]
    pub ordering: Ordering,
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
    /// Raised-tool straight segment from the preceding path end (or marker start).
    pub travel_mm: [Vec2; 2],
    /// Exact beginning and end of the cut, retaining source traversal direction.
    pub cut_endpoints_mm: [Vec2; 2],
    /// Complete polyline length, including the closing edge exactly once, in mm.
    pub cut_length_mm: f64,
}

/// A complete marker traversal. No partially cut marker is returned when optimization runs out.
#[derive(Clone, Debug, Serialize)]
pub struct Traversal {
    /// Every declared internal path followed by every physical exterior exactly once.
    pub runs: Vec<Run>,
    /// Optional final raised-tool travel; no cutting occurs along this segment.
    pub return_mm: Option<[Vec2; 2]>,
    /// Total complete cut length, in mm.
    pub cut_length_mm: f64,
    /// Total raised-tool travel including requested return, in mm.
    pub travel_length_mm: f64,
    /// Cut length / independently qualified feed rate, in seconds.
    pub cutting_seconds: f64,
    /// Raised-tool travel length / the locked travel speed, in seconds.
    pub travel_seconds: f64,
    /// One declared lift/lower cycle per path, in seconds.
    pub lift_lower_seconds: f64,
    /// Sum of declared constant-speed cut, travel and lift/lower phases only.
    pub modeled_path_seconds: f64,
}

/// The exact physical hide/remnant and its explicitly reported modeled loading position.
#[derive(Clone, Debug, Serialize)]
pub struct FiniteStock {
    /// Unique physical inventory identity, shared with finite-stock purchasing.
    pub stock_id: String,
    /// Exact frozen stock entry, including material, original geometry, sources and prior cuts.
    pub stock_sha256: String,
    /// Translation from unchanged final marker coordinates into the machine bed, in mm.
    /// The complete stock bounding-box lower-left corner is placed at the bed origin.
    pub marker_to_machine_offset_mm: Vec2,
    /// Composed rigid affine transform from original stock coordinates to the modeled machine bed.
    pub source_to_machine: [[f64; 3]; 2],
}

/// Complete baseline and selected traversal for one exact material marker.
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    /// Index into the cut plan's `rolls` or finite-stock `markers` array; unused stocks are omitted.
    pub marker: usize,
    /// Present only for individually inventoried finite stock; no stock boundary is cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finite_stock: Option<FiniteStock>,
    /// Full unchanged material identity.
    pub material_sha256: String,
    /// Digest of all final placements, including directions, internal cuts, copies and transforms.
    pub geometry_sha256: String,
    /// Exact source qualification used, retained independently of any display name.
    pub qualification: String,
    /// Admitted nominal thickness or full source-reported interval, in mm.
    pub thickness_mm: [f64; 2],
    /// Complete authored two-phase traversal.
    pub baseline: Traversal,
    /// Complete selected traversal; never more raised-tool distance than the baseline.
    pub selected: Traversal,
    /// Whether a complete nearest-path candidate strictly improved on the baseline.
    pub improved: bool,
}

/// Source-qualified modeled cut planning, independent of hardware authorization.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-knife-cut-program/1` for roll stock, or `/2` for finite-stock identities/bed transforms.
    pub schema: &'static str,
    /// Full machine configuration and timing lock used by all admitted recipes.
    pub machine_sha256: String,
    /// Complete marker-specific programs, in the original material/marker order.
    pub markers: Vec<Marker>,
    /// Actual bounded nearest-path candidate comparisons shared by every marker.
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
        }
        Ok((machine, recipes))
    }
}

fn on_bed(p: Vec2, m: &Machine) -> bool {
    p.is_finite() && p.x >= 0.0 && p.y >= 0.0 && p.x <= m.bed_mm[0] && p.y <= m.bed_mm[1]
}

pub(super) fn validate_source(pattern: &Pattern, indices: &BTreeSet<usize>) -> Result<(), Refusal> {
    for &index in indices {
        let piece = &pattern.raw()["pieces"][index];
        // The rendering/export annotation reader deliberately tolerates incomplete annotations.
        // A process program must not qualify a path after that reader has dropped source points.
        if let Some(drills) = piece.get("drills")
            && !drills.as_array().is_some_and(Vec::is_empty)
        {
            return Err(bad(
                "knife programs do not support declared drills or malformed drill inventories",
            ));
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
        work: &mut geometry::Work,
    ) -> Result<(Envelope, Option<FiniteStock>, String), Refusal> {
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
                    || stock_width > machine.bed_mm[0]
                    || length > machine.bed_mm[1]
                {
                    return Err(bad(
                        "complete roll marker exceeds the declared knife bed; repartition the marker explicitly",
                    ));
                }
                Ok((
                    Envelope {
                        size_mm: [width, length],
                        offset: None,
                    },
                    None,
                    digest(&marker.placements)?,
                ))
            }
            Self::Finite(_, marker, original) => {
                let b = bbox(&marker.outline_mm)?;
                let offset = Vec2::new(-b[0], -b[1]);
                let size_mm = [b[2] - b[0], b[3] - b[1]];
                if size_mm.iter().any(|&v| !positive(v))
                    || size_mm[0] > machine.bed_mm[0]
                    || size_mm[1] > machine.bed_mm[1]
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
                        offset: Some(offset),
                    },
                    Some(identity),
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
    let mut point_count = 0usize;
    // One complete internal phase preserves every part's exterior support until all features finish.
    for internal_phase in [true, false] {
        for (placement, piece) in placements.iter().enumerate() {
            if !piece.drills_mm.is_empty() {
                return Err(bad(
                    "knife programs do not silently omit drills or turn drill marks into knife circles",
                ));
            }
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
                if point_count > MAX_POINTS || paths.len() >= MAX_PATHS {
                    return Err(budget());
                }
                if points.len() < if closed { 3 } else { 2 }
                    || points.iter().any(|&p| {
                        let loaded = envelope.offset.map_or(p, |d| p + d);
                        !on_bed(loaded, machine)
                            || loaded.x > envelope.size_mm[0]
                            || loaded.y > envelope.size_mm[1]
                    })
                    || edges(points, closed).any(|(a, b)| !positive(a.dist(b)))
                {
                    return Err(bad(
                        "knife paths must be finite, nondegenerate and inside the admitted marker/bed",
                    ));
                }
                simple_path(points, closed, work)?;
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

fn traversal(
    placements: &[CutPlacement],
    paths: &[Path<'_>],
    order: &[usize],
    machine: &Machine,
    feed: f64,
) -> Result<Traversal, Refusal> {
    let mut position = machine.start_mm;
    let mut travel_length_mm = 0.0;
    let mut cut_length_mm = 0.0;
    let mut runs = Vec::with_capacity(order.len());
    for &i in order {
        let path = &paths[i];
        let piece = &placements[path.reference.placement];
        travel_length_mm += position.dist(path.start());
        cut_length_mm += path.length;
        runs.push(Run {
            path: path.reference.clone(),
            piece: piece.piece.clone(),
            instance: piece.instance.clone(),
            copy: piece.copy,
            closed: path.closed,
            travel_mm: [position, path.start()],
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
    let cutting_seconds = cut_length_mm / feed;
    let travel_seconds = travel_length_mm / machine.travel_mm_s;
    let lift_lower_seconds = order.len() as f64 * machine.lift_lower_seconds;
    let modeled_path_seconds = cutting_seconds + travel_seconds + lift_lower_seconds;
    if !modeled_path_seconds.is_finite()
        || !travel_length_mm.is_finite()
        || !cut_length_mm.is_finite()
    {
        return Err(bad("knife timing/length arithmetic overflows"));
    }
    Ok(Traversal {
        runs,
        return_mm,
        cut_length_mm,
        travel_length_mm,
        cutting_seconds,
        travel_seconds,
        lift_lower_seconds,
        modeled_path_seconds,
    })
}

fn nearest(paths: &[Path<'_>], machine: &Machine, work: &mut geometry::Work) -> Option<Vec<usize>> {
    let mut order = Vec::with_capacity(paths.len());
    let mut used = vec![false; paths.len()];
    let mut position = machine.start_mm;
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
/// Missing/mismatched process evidence, unsupported drills or material state, unqualified thickness,
/// invalid/oversized geometry, or exhaustion of the fixed geometric validation budget.
pub(super) fn build(plan: &CutPlan, policy: &Policy) -> Result<Report, Refusal> {
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
    let mut search_work = geometry::Work::new(policy.work_limit);
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
        let (envelope, finite_stock, geometry_sha256) =
            source.frame(&policy.machine, &mut geometry_work)?;
        let placements = source.placements();
        let paths = paths(placements, envelope, &policy.machine, &mut geometry_work)?;
        let baseline_order: Vec<_> = (0..paths.len()).collect();
        let baseline = traversal(
            placements,
            &paths,
            &baseline_order,
            &policy.machine,
            q.feed_mm_s,
        )?;
        let candidate = if policy.ordering == Ordering::NearestPath {
            nearest(&paths, &policy.machine, &mut search_work)
                .map(|order| traversal(placements, &paths, &order, &policy.machine, q.feed_mm_s))
                .transpose()?
        } else {
            None
        };
        let improved = candidate
            .as_ref()
            .is_some_and(|c| c.travel_length_mm < baseline.travel_length_mm);
        let selected = if improved {
            candidate.expect("checked candidate")
        } else {
            baseline.clone()
        };
        markers.push(Marker {
            marker: source.index(),
            finite_stock,
            material_sha256: material.sha256().into(),
            geometry_sha256,
            qualification: q.id.clone(),
            thickness_mm,
            baseline,
            selected,
            improved,
        });
    }
    Ok(Report {
        schema: if finite {
            "ftx-knife-cut-program/2"
        } else {
            "ftx-knife-cut-program/1"
        },
        machine_sha256,
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
                "Modeled time includes only constant-rate cutting, raised-tool straight travel and explicit lift/lower cycles. Acceleration, corner slowing, blade turning, loading, workholding changes and inspection are not inferred.",
                "No kerf compensation, common-line cutting, drill operation, thermal processing, tool/body collision check, measured cut quality, physical cycle-time claim or hardware execution authorization.",
                "Nearest-path ordering is a bounded heuristic; an unfinished or longer candidate retains the complete authored baseline, without a shortest-path proof.",
            ];
            if finite {
                limits.push("Each used physical stock item is independently loaded with its complete marker bounding-box lower-left corner at the bed origin. The reported translation preserves source orientation, all defects and prior occupied regions; stock boundaries, defects, occupied areas and zones are not cutting paths. This modeled loading position is not measured machine registration or workholding qualification.");
            }
            limits
        },
    })
}

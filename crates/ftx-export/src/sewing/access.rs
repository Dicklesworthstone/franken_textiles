//! Static planar exclusion geometry in the existing operation-local assembly frame. A circular
//! tool envelope sweeps every complete needle chord; no grip, approach motion, machine registration
//! or collision-free three-dimensional execution is inferred.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, polyline::point_in_polygon, polyline::signed_area};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};

use super::{
    SewingMachine, SewingQualification,
    assembly::AssemblyReceipt,
    bad, positive,
    production::workholding::{QualificationRef, machine_sha256, qualification_sha256},
    qualified_text, sha256_text,
};
use crate::outline::{ring_is_simple, segments_touch};

const MAX_FIXTURES: usize = 64;
const MAX_VERTICES: usize = 1024;
const MAX_QUALIFICATIONS: usize = 256;
const MAX_WORK: usize = 50_000_000;
const MAX_COORDINATE: f64 = 1_000_000.0;

/// A fixed filled exclusion polygon, with no inferred height or material support beneath it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Unique physical fixture identity within this operation's source policy.
    pub id: String,
    /// Simple polygon in the same millimetre frame as `FlatAssembly` (either winding).
    /// The closing point is implicit. Holes and self-intersections are unsupported.
    pub outline_mm: Vec<Vec2>,
}

/// Source-qualified static tool/fixture geometry for one explicitly assembled sewing operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `planar_disk_sweep/1`: a fixed circular envelope centered on the moving needle.
    pub model: String,
    /// Complete machine/configuration lock, including reverse and pressure capabilities.
    pub machine_sha256: String,
    /// Allowed exact process qualifications; the current operation must admit one of them.
    /// Alternatives support explicitly qualified material/recipe substitutions.
    pub qualifications: Vec<QualificationRef>,
    /// Positive circular exclusion radius, millimetres, independent of pressure contact area.
    pub tool_radius_mm: f64,
    /// Required nonnegative distance from the tool envelope to every filled fixture polygon.
    /// Positive margins are inclusive; geometric contact is always refused, even at zero margin.
    pub minimum_clearance_mm: f64,
    /// Static exclusions in this operation's declared flat assembly. All are checked.
    pub fixtures: Vec<Fixture>,
    /// Acquisition covering tool geometry, fixture geometry and their declared applicability.
    pub source: Source,
    /// Geometry acquisition and applicability protocol; an ID alone is not source evidence.
    pub method: String,
    /// Retained conditions, not live measurements or machine-frame calibration.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Evidence for this geometry policy, independent of the seam qualification.
    pub evidence: Evidence,
    /// Separate explicit permission for estimated geometry and applicability.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Deterministic nearest pair on one canonical needle chord and one fixture edge.
#[derive(Clone, Debug, Serialize)]
pub struct Witness {
    /// Source fixture identity, never a geometric validity or grip-strength certificate.
    pub fixture: String,
    /// Consecutive indices in the owning assembly's canonical `needle_mm` array.
    pub needle_segment: [usize; 2],
    /// Indices in the fixture's retained polygon, including its implicit closing edge.
    pub fixture_edge: [usize; 2],
    /// Nearest centerline point in the operation-local frame, in millimetres.
    pub needle_mm: Vec2,
    /// Nearest fixture-boundary point in the same frame, in millimetres.
    pub fixture_mm: Vec2,
}

/// Numerical clearance for the declared planar model, attached to the source-locked sewing plan.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// `ftx-sewing-access/1`.
    pub schema: &'static str,
    /// `planar_disk_sweep/1`.
    pub model: &'static str,
    /// The existing assembly frame; never a calibrated machine or robot frame.
    pub frame: &'static str,
    /// Exact machine content lock admitted against the current sewing options.
    pub machine_sha256: String,
    /// Exact admitted material/process qualification and its full content lock.
    pub qualification: QualificationRef,
    /// Declared tool radius; not inferred from needle size or compression reference area.
    pub tool_radius_mm: f64,
    /// Requested separation from every fixture, retained without added safety factors.
    pub required_clearance_mm: f64,
    /// Smallest tool-boundary distance over all full needle chords and filled fixtures, mm.
    pub minimum_clearance_mm: f64,
    /// Smallest centerline-to-fixture distance before subtracting the circular radius, mm.
    pub minimum_centerline_distance_mm: f64,
    /// Number of unique canonical chords; reverse/backtack travel revisits the same sweep.
    pub checked_segments: usize,
    /// Number of independently declared fixed exclusions checked for every chord.
    pub fixture_count: usize,
    /// First minimum in canonical chord, authored fixture and polygon-edge order.
    pub witness: Witness,
    /// Static geometric separation cannot establish fixture grip or physical seam accessibility.
    pub physical_access_validated: bool,
}

/// Aggregate source-validation and geometric work across all operations in one sewing plan.
#[derive(Default)]
pub(super) struct Budget(usize);

impl Budget {
    fn charge(&mut self, work: usize) -> Result<(), Refusal> {
        self.0 = self.0.saturating_add(work);
        if self.0 > MAX_WORK {
            Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "planar sewing access exceeds the shared 50000000 edge-work budget",
            ))
        } else {
            Ok(())
        }
    }
}

fn bounded(point: Vec2) -> bool {
    point.is_finite() && point.x.abs().max(point.y.abs()) <= MAX_COORDINATE
}

fn edges(ring: &[Vec2]) -> impl Iterator<Item = (usize, Vec2, Vec2)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
        .enumerate()
        .map(|(index, (a, b))| (index, a, b))
}

fn nearest(point: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let delta = b - a;
    if delta.norm2() == 0.0 {
        a
    } else {
        a + delta * ((point - a).dot(delta) / delta.norm2()).clamp(0.0, 1.0)
    }
}

// For two disjoint planar segments a nearest pair includes an endpoint of at least one segment.
fn closest(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> (f64, Vec2, Vec2) {
    let pairs = [
        (a, nearest(a, c, d)),
        (b, nearest(b, c, d)),
        (nearest(c, a, b), c),
        (nearest(d, a, b), d),
    ];
    let mut best = (f64::INFINITY, a, c);
    for (needle, fixture) in pairs {
        let squared = (needle - fixture).norm2();
        if squared < best.0 {
            best = (squared, needle, fixture);
        }
    }
    best
}

impl Policy {
    pub(super) fn validate(
        &self,
        machine: &SewingMachine,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let vertices = self.fixtures.iter().fold(0usize, |n, fixture| {
            n.saturating_add(fixture.outline_mm.len())
        });
        if self.fixtures.is_empty()
            || self.fixtures.len() > MAX_FIXTURES
            || vertices > MAX_VERTICES
            || self.qualifications.is_empty()
            || self.qualifications.len() > MAX_QUALIFICATIONS
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "planar access needs 1..64 fixtures, at most 1024 vertices and 1..256 qualification references",
            ));
        }
        if self.model != "planar_disk_sweep/1"
            || !sha256_text(&self.machine_sha256)
            || self.machine_sha256 != machine_sha256(machine)?
            || !positive(self.tool_radius_mm)
            || self.tool_radius_mm > MAX_COORDINATE
            || !self.minimum_clearance_mm.is_finite()
            || !(0.0..=MAX_COORDINATE).contains(&self.minimum_clearance_mm)
            || !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(key, value)| !qualified_text(key) || !qualified_text(value))
        {
            return Err(bad(
                "invalid planar access geometry/source or full machine content lock",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated planar access needs its own explicit consent",
            ));
        }
        let mut qualifications = BTreeSet::new();
        for reference in &self.qualifications {
            if !qualified_text(&reference.id)
                || !sha256_text(&reference.sha256)
                || !qualifications.insert(&reference.id)
            {
                return Err(bad("invalid or duplicate planar access qualification lock"));
            }
        }
        let mut fixtures = BTreeSet::new();
        for fixture in &self.fixtures {
            let ring = &fixture.outline_mm;
            budget.charge(ring.len().saturating_mul(ring.len()))?;
            if !qualified_text(&fixture.id)
                || !fixtures.insert(&fixture.id)
                || ring.len() < 3
                || ring.iter().any(|&point| !bounded(point))
                || edges(ring).any(|(_, a, b)| a == b)
                || signed_area(ring).abs() <= 1e-8
                || !ring_is_simple(ring)
            {
                return Err(bad(
                    "planar fixtures need unique IDs and bounded, nondegenerate simple polygons",
                ));
            }
        }
        Ok(())
    }

    /// Called only after ordinary geometry and unique process admission. All full canonical chords
    /// are traversed by the complete seam run; reverse and lock runs add no unexamined segments.
    pub(super) fn evaluate(
        &self,
        assembly: &AssemblyReceipt,
        qualification: &SewingQualification,
        budget: &mut Budget,
    ) -> Result<Receipt, Refusal> {
        let hash = qualification_sha256(qualification)?;
        let admitted = self
            .qualifications
            .iter()
            .find(|reference| reference.id == qualification.id && reference.sha256 == hash)
            .ok_or_else(|| {
                bad("planar access does not cover the exact admitted process qualification")
            })?;
        let needle = &assembly.needle_mm;
        if assembly.frame != "operation_local_mm"
            || needle.len() < 2
            || needle.iter().any(|&point| !bounded(point))
        {
            return Err(bad(
                "planar access needs the bounded explicit operation-local assembly path",
            ));
        }
        let vertices: usize = self.fixtures.iter().map(|f| f.outline_mm.len()).sum();
        // Two endpoint containment tests plus complete edge distances, reserved before evaluation.
        budget.charge(
            (needle.len() - 1)
                .saturating_mul(vertices)
                .saturating_mul(3),
        )?;
        let required = self.tool_radius_mm + self.minimum_clearance_mm;
        let mut best: Option<(f64, Witness)> = None;
        for (segment, chord) in needle.windows(2).enumerate() {
            let [a, b] = [chord[0], chord[1]];
            for fixture in &self.fixtures {
                let ring = &fixture.outline_mm;
                if point_in_polygon(a, ring) || point_in_polygon(b, ring) {
                    return Err(bad(format!(
                        "planar access: needle segment {segment} enters fixture {}",
                        fixture.id
                    )));
                }
                for (edge, c, d) in edges(ring) {
                    if segments_touch(a, b, c, d) {
                        return Err(bad(format!(
                            "planar access: needle segment {segment} intersects fixture {} edge {edge}",
                            fixture.id
                        )));
                    }
                    let (squared, on_needle, on_fixture) = closest(a, b, c, d);
                    if squared <= self.tool_radius_mm * self.tool_radius_mm
                        || squared < required * required
                    {
                        return Err(bad(format!(
                            "planar access: tool sweep at needle segment {segment} lacks clearance from fixture {} edge {edge}",
                            fixture.id
                        )));
                    }
                    if best
                        .as_ref()
                        .is_none_or(|(distance, _)| squared < *distance)
                    {
                        best = Some((
                            squared,
                            Witness {
                                fixture: fixture.id.clone(),
                                needle_segment: [segment, segment + 1],
                                fixture_edge: [edge, (edge + 1) % ring.len()],
                                needle_mm: on_needle,
                                fixture_mm: on_fixture,
                            },
                        ));
                    }
                }
            }
        }
        let (squared, witness) = best.expect("validated nonempty path and fixture edges");
        let distance = (witness.needle_mm - witness.fixture_mm).norm();
        debug_assert!(squared.is_finite() && distance.is_finite());
        Ok(Receipt {
            schema: "ftx-sewing-access/1",
            model: "planar_disk_sweep/1",
            frame: "operation_local_mm",
            machine_sha256: self.machine_sha256.clone(),
            qualification: admitted.clone(),
            tool_radius_mm: self.tool_radius_mm,
            required_clearance_mm: self.minimum_clearance_mm,
            minimum_clearance_mm: distance - self.tool_radius_mm,
            minimum_centerline_distance_mm: distance,
            checked_segments: needle.len() - 1,
            fixture_count: self.fixtures.len(),
            witness,
            physical_access_validated: false,
        })
    }
}

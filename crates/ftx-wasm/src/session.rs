//! A posing session for the page: one garment sewn and draped on a baked synthetic body, kept in memory,
//! and re-draped as the body's arms are moved.
//!
//! The solver is the production one throughout. [`Session::start`] calls [`ftx_drape::run::drape`] once and
//! keeps its scene; [`Session::advance`] calls [`ftx_drape::motion::advance_body`], the checked
//! moving-body step, towards the body posed at the requested arm angles. Nothing is approximated for
//! speed: a move the solver cannot complete is refused and leaves the session where it was.
//!
//! The body is a [`PosableBody`]: frames, landmarks, section radii, the collision and display surfaces and
//! each arm sampled at a list of angles, baked natively by `showcase/scripts/bake-posable-body.rs` so that
//! the page needs no converted body model. Between two sampled angles a surface is interpolated linearly,
//! which is also the straight vertex path the solver moves the body along. Positions are single precision
//! and the section radii are tabulated per millimetre, so a session is not a bit-for-bit copy of a native
//! `ftx drape` of the same inputs. No claim: a pose reached here is a cloth state the solver accepted on a
//! synthetic body, not evidence of fit or comfort in that pose.

use std::collections::BTreeMap;

use ftx_core::Vec3;
use ftx_drape::motion::{MotionOptions, MotionReport, advance_body, advance_body_carried};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::{PartFrames, Scene};
use ftx_drape::stepping::SubdivisionOptions;
use ftx_drape::{
    AdaptiveBarrierPolicy, BendingModel, CompressionModel, ConstructionMechanicsOptions,
    DampingOptions, MeshOptions, NewtonStrategy, PartFrame, SelfRepulsionOptions,
    StickSlipFriction, TimeIntegrator,
};
use ftx_pattern::Pattern;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA: &str = "ftx-posable-body/1";
/// Sew + drape + settle steps a session's opening drape may ask for, as the page's fold-up.
const MAX_OPENING_STEPS: usize = 512;
/// Longest move one call may ask for (s). A page sends many short moves, not one long one.
const MAX_MOVE_S: f64 = 2.0;

#[derive(Deserialize)]
struct FrameDoc {
    origin: [f64; 3],
    up: [f64; 3],
    front: [f64; 3],
    lat: [f64; 3],
}

#[derive(Deserialize)]
struct ArrayDoc {
    dtype: String,
    offset: usize,
    count: usize,
}

/// One arm's frame and landmark heights with that arm at one sampled angle.
#[derive(Deserialize)]
struct ArmFrameDoc {
    origin: [f64; 3],
    up: [f64; 3],
    front: [f64; 3],
    lat: [f64; 3],
    landmarks: BTreeMap<String, f64>,
}

#[derive(Deserialize)]
struct Header {
    schema: String,
    base_arm_deg: f64,
    arm_angles_deg: Vec<f64>,
    radius_h0_mm: f64,
    frames: BTreeMap<String, FrameDoc>,
    landmarks: BTreeMap<String, BTreeMap<String, f64>>,
    /// `arm_L` and `arm_R`, one entry per sampled angle. Without them a drape can only start in the base pose.
    #[serde(default)]
    arm_frames: BTreeMap<String, Vec<ArmFrameDoc>>,
    arrays: BTreeMap<String, ArrayDoc>,
}

/// One arm's displacement of a surface from the base pose: the vertices it moves, and where each is at
/// every sampled angle.
#[derive(Debug)]
struct Arm {
    indices: Vec<usize>,
    /// `offsets[sample][k]` is the displacement of vertex `indices[k]` (m).
    offsets: Vec<Vec<Vec3>>,
}

#[derive(Debug)]
struct Surface {
    base: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    left: Arm,
    right: Arm,
}

/// One arm at one sampled angle: its frame and its landmark heights (mm).
type ArmSample = (PartFrame, BTreeMap<String, f64>);

/// A baked synthetic body (`ftx-posable-body/1`): what a drape places pieces by, collides with and shows,
/// with each arm sampled at a list of abduction angles.
#[derive(Debug)]
pub struct PosableBody {
    base_deg: f64,
    angles: Vec<f64>,
    frames: BTreeMap<String, PartFrame>,
    landmarks: BTreeMap<String, BTreeMap<String, f64>>,
    /// `arm_L` / `arm_R` → per sampled angle, that arm's frame and landmark heights.
    arm_frames: BTreeMap<String, Vec<ArmSample>>,
    radius: BTreeMap<String, Vec<f64>>,
    radius_h0: f64,
    collision: Surface,
    display: Surface,
}

/// The frames a drape starts from with each arm at a sampled angle: the base pose's, with each arm's own
/// frame and landmarks taken from its sample.
struct PosedFrames<'a> {
    body: &'a PosableBody,
    /// Sample index of the left and right arm.
    arms: [usize; 2],
}

impl PosedFrames<'_> {
    fn arm(&self, part: &str) -> Option<&ArmSample> {
        let side = match part {
            "arm_L" => 0,
            "arm_R" => 1,
            _ => return None,
        };
        self.body.arm_frames.get(part)?.get(self.arms[side])
    }
}

impl PartFrames for PosedFrames<'_> {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        self.arm(part)
            .map(|(frame, _)| *frame)
            .or_else(|| self.body.frame(part))
    }

    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        match self.arm(part) {
            Some((_, heights)) => heights.get(name).copied(),
            None => self.body.landmark_h(part, name),
        }
    }

    // An arm's section radius is its upper arm's, which does not change as the arm turns.
    fn section_radius(&self, part: &str, h: f64) -> f64 {
        self.body.section_radius(part, h)
    }

    // Each arm is at its own sampled angle, so the mirrored sleeve goes round the left arm itself.
    fn independent_limbs(&self) -> bool {
        true
    }
}

fn bytes_of<'a>(
    header: &Header,
    data: &'a [u8],
    name: &str,
    dtype: &str,
) -> Result<&'a [u8], String> {
    let entry = header
        .arrays
        .get(name)
        .ok_or_else(|| format!("posable body: array {name} is missing"))?;
    if entry.dtype != dtype {
        return Err(format!(
            "posable body: array {name} is {}, not {dtype}",
            entry.dtype
        ));
    }
    entry
        .count
        .checked_mul(4)
        .and_then(|length| entry.offset.checked_add(length))
        .and_then(|end| data.get(entry.offset..end))
        .ok_or_else(|| format!("posable body: array {name} runs past the end of the data"))
}

fn floats(header: &Header, data: &[u8], name: &str) -> Result<Vec<f64>, String> {
    let values: Vec<f64> = bytes_of(header, data, name, "f32")?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f64::from(f32::from_le_bytes(b)))
        .collect();
    if values.iter().all(|v| v.is_finite()) {
        Ok(values)
    } else {
        Err(format!(
            "posable body: array {name} holds a non-finite value"
        ))
    }
}

fn indices(header: &Header, data: &[u8], name: &str, below: usize) -> Result<Vec<usize>, String> {
    bytes_of(header, data, name, "u32")?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| {
            let index = u32::from_le_bytes(b) as usize;
            (index < below).then_some(index).ok_or_else(|| {
                format!("posable body: array {name} indexes vertex {index} of {below}")
            })
        })
        .collect()
}

fn points(values: &[f64], name: &str) -> Result<Vec<Vec3>, String> {
    if !values.len().is_multiple_of(3) {
        return Err(format!(
            "posable body: array {name} is not a list of xyz triples"
        ));
    }
    Ok(values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect())
}

fn arm(
    header: &Header,
    data: &[u8],
    stem: &str,
    vertices: usize,
    samples: usize,
) -> Result<Arm, String> {
    let moved = indices(header, data, &format!("{stem}_indices"), vertices)?;
    let name = format!("{stem}_offsets");
    let all = points(&floats(header, data, &name)?, &name)?;
    if moved.len().checked_mul(samples) != Some(all.len()) {
        return Err(format!(
            "posable body: array {name} holds {} points, expected {} vertices at {samples} angles",
            all.len(),
            moved.len()
        ));
    }
    let offsets = if moved.is_empty() {
        vec![Vec::new(); samples]
    } else {
        all.chunks_exact(moved.len())
            .map(<[Vec3]>::to_vec)
            .collect()
    };
    Ok(Arm {
        indices: moved,
        offsets,
    })
}

fn surface(header: &Header, data: &[u8], stem: &str, samples: usize) -> Result<Surface, String> {
    let name = format!("{stem}_vertices");
    let base = points(&floats(header, data, &name)?, &name)?;
    let name = format!("{stem}_triangles");
    let corners = indices(header, data, &name, base.len())?;
    if base.is_empty() || corners.is_empty() || !corners.len().is_multiple_of(3) {
        return Err(format!(
            "posable body: the {stem} surface is empty or its triangles are incomplete"
        ));
    }
    Ok(Surface {
        triangles: corners.as_chunks::<3>().0.to_vec(),
        left: arm(header, data, &format!("left_{stem}"), base.len(), samples)?,
        right: arm(header, data, &format!("right_{stem}"), base.len(), samples)?,
        base,
    })
}

impl PosableBody {
    /// Reads a baked body from its header (JSON text) and its array data.
    ///
    /// # Errors
    /// Another schema, a missing or misdeclared array, an index out of range, a non-finite number, or arm
    /// angles that are not increasing or do not include the base angle.
    pub fn parse(header: &str, data: &[u8]) -> Result<Self, String> {
        let header: Header =
            serde_json::from_str(header).map_err(|e| format!("posable body header: {e}"))?;
        if header.schema != SCHEMA {
            return Err(format!(
                "posable body: schema {} is not {SCHEMA}",
                header.schema
            ));
        }
        let angles = header.arm_angles_deg.clone();
        let increasing = angles.windows(2).all(|pair| pair[0] < pair[1]);
        if angles.len() < 2
            || !increasing
            || angles.iter().any(|a| !(0.0..=180.0).contains(a))
            || !angles.contains(&header.base_arm_deg)
        {
            return Err(
                "posable body: arm angles must increase within 0–180° and include the base angle"
                    .into(),
            );
        }
        let vector = |v: [f64; 3]| Vec3::new(v[0], v[1], v[2]);
        let mut frames = BTreeMap::new();
        let mut radius = BTreeMap::new();
        for (part, frame) in &header.frames {
            let frame = PartFrame {
                origin: vector(frame.origin),
                up: vector(frame.up),
                front: vector(frame.front),
                lat: vector(frame.lat),
            };
            if ![frame.origin, frame.up, frame.front, frame.lat]
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(format!("posable body: the {part} frame is not finite"));
            }
            frames.insert(part.clone(), frame);
            radius.insert(
                part.clone(),
                floats(&header, data, &format!("radius_{part}"))?,
            );
        }
        if !header.radius_h0_mm.is_finite()
            || header
                .landmarks
                .values()
                .flat_map(BTreeMap::values)
                .any(|h| !h.is_finite())
        {
            return Err("posable body: a landmark height is not finite".into());
        }
        let mut arm_frames = BTreeMap::new();
        for (part, docs) in &header.arm_frames {
            if !matches!(part.as_str(), "arm_L" | "arm_R") || docs.len() != angles.len() {
                return Err(format!(
                    "posable body: arm frames {part} must be arm_L or arm_R with one per sampled angle"
                ));
            }
            let mut samples = Vec::with_capacity(docs.len());
            for doc in docs {
                let frame = PartFrame {
                    origin: vector(doc.origin),
                    up: vector(doc.up),
                    front: vector(doc.front),
                    lat: vector(doc.lat),
                };
                if ![frame.origin, frame.up, frame.front, frame.lat]
                    .iter()
                    .all(|v| v.is_finite())
                    || doc.landmarks.values().any(|h| !h.is_finite())
                {
                    return Err(format!("posable body: an {part} frame is not finite"));
                }
                samples.push((frame, doc.landmarks.clone()));
            }
            arm_frames.insert(part.clone(), samples);
        }
        Ok(Self {
            base_deg: header.base_arm_deg,
            radius_h0: header.radius_h0_mm,
            collision: surface(&header, data, "collision", angles.len())?,
            display: surface(&header, data, "display", angles.len())?,
            angles,
            frames,
            landmarks: header.landmarks,
            arm_frames,
            radius,
        })
    }

    /// The angles each arm was sampled at (degrees from vertical). A drape can start with either arm at
    /// any of them; moves go anywhere in between.
    #[must_use]
    pub fn arm_angles(&self) -> &[f64] {
        &self.angles
    }

    // The frames for a drape that starts with the arms at these sampled angles.
    fn frames_at(&self, left: f64, right: f64) -> Result<PosedFrames<'_>, String> {
        let sample = |angle: f64| {
            self.angles
                .iter()
                .position(|&a| (a - angle).abs() < 1e-9)
                .ok_or_else(|| {
                    format!("a drape starts with each arm at a sampled angle, not {angle}°")
                })
        };
        let arms = [sample(left)?, sample(right)?];
        let at_base = |side: usize| self.angles[arms[side]] == self.base_deg;
        for (side, part) in [(0, "arm_L"), (1, "arm_R")] {
            if !at_base(side) && !self.arm_frames.contains_key(part) {
                return Err(format!(
                    "this body has no baked {part} frames, so a drape can only start in its base pose"
                ));
            }
        }
        Ok(PosedFrames { body: self, arms })
    }

    /// The smallest and largest arm angle the body was sampled at (degrees from vertical).
    #[must_use]
    pub fn arm_range(&self) -> (f64, f64) {
        (self.angles[0], self.angles[self.angles.len() - 1])
    }

    /// The arm angle of the pose the body was baked in, which the opening drape uses.
    #[must_use]
    pub fn base_arm_deg(&self) -> f64 {
        self.base_deg
    }

    // The two samples an angle lies between and its fraction of the way from the first to the second.
    fn bracket(&self, angle: f64) -> (usize, f64) {
        let (low, high) = self.arm_range();
        let angle = if angle.is_finite() {
            angle.clamp(low, high)
        } else {
            self.base_deg
        };
        let upper = self
            .angles
            .iter()
            .position(|&a| a >= angle)
            .unwrap_or(self.angles.len() - 1)
            .max(1);
        let (a, b) = (self.angles[upper - 1], self.angles[upper]);
        (upper - 1, (angle - a) / (b - a))
    }

    fn posed(&self, surface: &Surface, left: f64, right: f64) -> Vec<Vec3> {
        let mut points = surface.base.clone();
        for (arm, angle) in [(&surface.left, left), (&surface.right, right)] {
            let (sample, t) = self.bracket(angle);
            let (from, to) = (&arm.offsets[sample], &arm.offsets[sample + 1]);
            for (k, &vertex) in arm.indices.iter().enumerate() {
                points[vertex] = points[vertex] + from[k] * (1.0 - t) + to[k] * t;
            }
        }
        points
    }

    // Angles vary linearly in time, but the baked surface is only linear between its samples.
    // The union of both arms' sample crossings partitions the displayed path into the exact
    // vertex chords the moving-body solver checks. Include the final endpoint exactly once.
    fn motion_fractions(&self, from: [f64; 2], to: [f64; 2]) -> Vec<f64> {
        let mut fractions = vec![1.0];
        for (from, to) in from.into_iter().zip(to) {
            if from == to {
                continue;
            }
            for &angle in &self.angles {
                if angle > from.min(to) && angle < from.max(to) {
                    fractions.push((angle - from) / (to - from));
                }
            }
        }
        fractions.sort_unstable_by(f64::total_cmp);
        fractions.dedup();
        fractions
    }

    /// The collision surface with the arms at these angles (m), clamped to the sampled range.
    #[must_use]
    pub fn collision_at(&self, left: f64, right: f64) -> Vec<Vec3> {
        self.posed(&self.collision, left, right)
    }

    /// The displayed surface with the arms at these angles (m), clamped to the sampled range.
    #[must_use]
    pub fn display_at(&self, left: f64, right: f64) -> Vec<Vec3> {
        self.posed(&self.display, left, right)
    }
}

impl PartFrames for PosableBody {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        self.frames.get(part).copied()
    }

    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        self.landmarks.get(part)?.get(name).copied()
    }

    // The baked table holds the radius at whole millimetres of the part's height coordinate; beyond its
    // ends there is no body.
    fn section_radius(&self, part: &str, h: f64) -> f64 {
        let Some(table) = self.radius.get(part) else {
            return 0.0;
        };
        let at = (h - self.radius_h0).round();
        if at.is_finite() && at >= 0.0 && (at as usize) < table.len() {
            table[at as usize]
        } else {
            0.0
        }
    }
}

/// The opening drape's settings. Absent fields take the production defaults.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionSettings {
    /// Mesh spacing (mm).
    pub spacing_mm: f64,
    /// Sewing steps.
    pub sew_steps: usize,
    /// Steps under gravity.
    pub drape_steps: usize,
    /// Further gravity steps until the cloth is quiet.
    pub max_settle_steps: usize,
    /// Bisect a step that does not converge.
    pub time_subdivision: bool,
    /// Cloth–cloth contact thickness (mm); zero leaves cloth self-contact unsimulated.
    pub self_contact_mm: f64,
    /// Nonlinear globalization and linear-solve strategy (default: `ProjectedNewton`).
    pub newton_strategy: NewtonStrategy,
    /// Structural time-integration scheme (default: `ImplicitEuler`).
    pub integrator: TimeIntegrator,
    /// Hinge bending formulation.
    pub bending_model: BendingModel,
    /// Membrane compression-softening formulation.
    pub compression_model: CompressionModel,
    /// Compressive stiffness as a fraction of the tensile (`ftx drape --compression`; 1 is the
    /// reference's cloth, as stiff in compression as in tension). A numerical model choice, not a
    /// measured fabric property.
    pub compression: f64,
    /// Opt-in aerodynamic drag with a retained uniform world-space air velocity, and
    /// SE(3)-invariant Rayleigh internal damping.
    pub damping: DampingOptions,
    /// Opt-in Li et al. (2020) adaptive IPC barrier stiffness scaling.
    pub adaptive_barrier: Option<AdaptiveBarrierPolicy>,
    /// Opt-in smooth static/kinetic stick-slip friction for cloth–body contact.
    pub stick_slip: Option<StickSlipFriction>,
    /// Opt-in smooth static/kinetic stick-slip friction for cloth–cloth self-contact.
    pub self_stick_slip: Option<StickSlipFriction>,
    /// Opt-in curvature/gather-adaptive meshing and bounded interior Lloyd CVT smoothing.
    pub mesh: MeshOptions,
    /// Opt-in folded-hem and sewn seam-allowance mass and bending multipliers.
    pub construction: ConstructionMechanicsOptions,
    /// Opt-in spatial-hash cloth–cloth self-repulsion during start-state relaxation.
    pub start_self_repulsion: Option<SelfRepulsionOptions>,
    /// Longest body-motion substep of a move (s).
    pub motion_step_s: f64,
    /// Carry radius of an arm move (mm, [`ftx_drape::motion::advance_body_carried`]): each
    /// substep's solve starts with the cloth within this distance moved along with the body. Zero
    /// starts from the cloth at rest ([`advance_body`]).
    pub motion_carry_mm: f64,
    /// Newton tolerance (m/s, above 0 and at most 0.1) of every move substep but the last
    /// ([`MotionOptions::intermediate_tol`]); absent keeps the drape's 1e-3 m/s throughout. The
    /// last substep of every move always converges to the drape's tolerance.
    pub motion_intermediate_tol_m_s: Option<f64>,
}

impl Default for SessionSettings {
    fn default() -> Self {
        let drape = DrapeOptions::default();
        Self {
            spacing_mm: drape.spacing,
            sew_steps: drape.sew_steps,
            drape_steps: drape.drape_steps,
            max_settle_steps: 0,
            time_subdivision: true,
            self_contact_mm: 0.0,
            newton_strategy: drape.newton_strategy,
            integrator: drape.integrator,
            bending_model: drape.bending_model,
            compression_model: drape.compression_model,
            compression: drape.compression,
            damping: drape.damping,
            adaptive_barrier: drape.adaptive_barrier,
            stick_slip: drape.stick_slip,
            self_stick_slip: drape.self_stick_slip,
            mesh: drape.mesh,
            construction: drape.construction,
            start_self_repulsion: drape.start_self_repulsion,
            motion_step_s: MotionOptions::default().max_step_s,
            motion_carry_mm: 0.0,
            motion_intermediate_tol_m_s: None,
        }
    }
}

/// A garment draped on a posable body, and the pose it has reached.
pub struct Session {
    body: PosableBody,
    scene: Scene,
    arms: [f64; 2],
    replay: Vec<f32>,
    summary: Value,
    motion: MotionOptions,
    /// Carry radius of an arm move (m); `None` for no carry.
    carry_m: Option<f64>,
    /// The last accepted move's work: trials (including rejected ones), accepted substeps and each
    /// substep's Newton iterations; null before the first move.
    last_move: Value,
}

fn flat(points: &[Vec3]) -> impl Iterator<Item = f32> + '_ {
    points
        .iter()
        .flat_map(|p| [p.x as f32, p.y as f32, p.z as f32])
}

impl Session {
    /// Sews and drapes `pattern` on `body` in its base pose, calling `progress` with a line per stage.
    ///
    /// # Errors
    /// Settings out of range or over the page's work budget, and every refusal of the drape itself.
    pub fn start(
        body: PosableBody,
        pattern: &Value,
        fit_spec: &Value,
        settings: SessionSettings,
        progress: impl FnMut(&str),
    ) -> Result<Self, String> {
        let base = body.base_arm_deg();
        Self::start_posed(body, pattern, fit_spec, settings, base, base, progress)
    }

    /// Sews and drapes `pattern` on `body` with the left and right arm at these sampled angles (degrees
    /// from vertical): the pieces are placed by the arms' frames in that pose, as `ftx drape --arm-deg`
    /// places them, and sewn and hung there.
    ///
    /// # Errors
    /// An angle that is not one of the body's samples, a body without baked arm frames for a pose other
    /// than its base, settings out of range or over the page's work budget, and every refusal of the drape.
    pub fn start_posed(
        body: PosableBody,
        pattern: &Value,
        fit_spec: &Value,
        settings: SessionSettings,
        left_deg: f64,
        right_deg: f64,
        mut progress: impl FnMut(&str),
    ) -> Result<Self, String> {
        let SessionSettings {
            spacing_mm,
            sew_steps,
            drape_steps,
            max_settle_steps,
            time_subdivision,
            self_contact_mm,
            newton_strategy,
            integrator,
            bending_model,
            compression_model,
            compression,
            damping,
            adaptive_barrier,
            stick_slip,
            self_stick_slip,
            mesh,
            construction,
            start_self_repulsion,
            motion_step_s,
            motion_carry_mm,
            motion_intermediate_tol_m_s,
        } = settings;
        if !self_contact_mm.is_finite() || self_contact_mm < 0.0 {
            return Err("self-contact thickness must be finite and nonnegative".into());
        }
        if !motion_carry_mm.is_finite() || !(0.0..=100.0).contains(&motion_carry_mm) {
            return Err("the motion carry radius must lie between 0 and 100 mm".into());
        }
        if !motion_step_s.is_finite() || !(1e-4..=0.1).contains(&motion_step_s) {
            return Err("the motion step must lie between 0.0001 and 0.1 s".into());
        }
        if motion_intermediate_tol_m_s
            .is_some_and(|tol| !(tol.is_finite() && tol > 0.0 && tol <= 0.1))
        {
            return Err(
                "the intermediate move tolerance must lie above 0 and at most 0.1 m/s".into(),
            );
        }
        if sew_steps
            .saturating_add(drape_steps)
            .saturating_add(max_settle_steps)
            > MAX_OPENING_STEPS
        {
            return Err(format!(
                "the opening drape exceeds the {MAX_OPENING_STEPS}-step browser work budget (sew + drape + settle)"
            ));
        }
        let strategy_within_budget = match newton_strategy {
            NewtonStrategy::ProjectedNewton => true,
            NewtonStrategy::AdaptiveIlu {
                pcg_iters,
                max_shifts,
            } => pcg_iters <= 256 && max_shifts <= 16,
            NewtonStrategy::TrustRegionSteihaug { cg_iters, .. } => cg_iters <= 4096,
        };
        if !strategy_within_budget {
            return Err("configured newton_strategy exceeds the browser solver work budget".into());
        }
        let defaults = DrapeOptions::default();
        let mut options = DrapeOptions {
            spacing: spacing_mm,
            sew_steps,
            drape_steps,
            max_settle_steps,
            time_subdivision: time_subdivision.then(SubdivisionOptions::default),
            newton_strategy,
            integrator,
            bending_model,
            compression_model,
            compression,
            damping,
            adaptive_barrier,
            stick_slip,
            self_stick_slip,
            mesh,
            construction,
            start_self_repulsion,
            ..defaults
        };
        if self_contact_mm > 0.0 {
            options.self_contact = Some(ftx_drape::contact::ContactParams {
                thickness: self_contact_mm * 1e-3,
                ..options.contact
            });
        }
        options.validate().map_err(|refusal| refusal.to_string())?;
        let pattern = Pattern::new(pattern.clone());
        let base = body.base_arm_deg();
        let collision = body.collision_at(left_deg, right_deg);
        let display = body.display_at(left_deg, right_deg);
        let run = {
            let frames = body.frames_at(left_deg, right_deg)?;
            drape(
                &pattern,
                fit_spec,
                &DrapeBody {
                    frames: &frames,
                    collision: (&collision, &body.collision.triangles),
                    display: (&display, &body.display.triangles),
                    // Retain the legacy common angle for equal arms, and the exact pair when they differ.
                    arm_deg: left_deg.max(right_deg),
                    arm_pose: (left_deg != right_deg).then_some(ftx_body::drape::ArmPose {
                        left_abduction_deg: left_deg,
                        right_abduction_deg: right_deg,
                    }),
                },
                &options,
                &mut progress,
            )
            .map_err(|refusal| refusal.to_string())?
        };
        let mesh = &run.scene.mesh;
        let pieces: Vec<Value> = mesh
            .instances
            .iter()
            .zip(&mesh.inst_range)
            .map(|(id, &(first, end))| json!({"id": id, "first": first, "end": end}))
            .collect();
        let seams: Vec<Value> = mesh
            .seams
            .iter()
            .map(|seam| json!({"id": seam.id, "side_a": seam.side_a, "side_b": seam.side_b}))
            .collect();
        let (low, high) = body.arm_range();
        let summary = json!({
            "schema": "ftx-drape-session/1",
            "cloth_vertices": mesh.verts2d.len(),
            "cloth_triangles": mesh.faces.len(),
            "solver_vertices": run.scene.solver.x.len(),
            "pieces": pieces,
            "seams": seams,
            "frames": {"count": run.fold.frames.len(), "fold": run.fold.fold, "zip": run.fold.zip,
                       "sew": run.fold.sew, "drape": run.fold.drape},
            "base_arm_deg": base,
            "arm_range_deg": [low, high],
            "arm_angles_deg": body.arm_angles(),
            "arms_deg": [left_deg, right_deg],
            "report": run.report,
        });
        let replay = run
            .fold
            .frames
            .iter()
            .flat_map(|frame| flat(frame))
            .collect();
        Ok(Self {
            body,
            scene: run.scene,
            arms: [left_deg, right_deg],
            replay,
            summary,
            motion: MotionOptions {
                max_step_s: motion_step_s,
                intermediate_tol: motion_intermediate_tol_m_s,
                ..MotionOptions::default()
            },
            carry_m: (motion_carry_mm > 0.0).then_some(motion_carry_mm * 1e-3),
            last_move: Value::Null,
        })
    }

    /// Counts, the pieces' vertex ranges, each seam's two vertex runs, the opening drape's phases and its
    /// report (`ftx-drape-session/1`).
    #[must_use]
    pub fn summary(&self) -> &Value {
        &self.summary
    }

    /// Every frame of the opening fold-up, frame-major, as mesh-vertex xyz (m).
    #[must_use]
    pub fn replay(&self) -> &[f32] {
        &self.replay
    }

    /// The cloth as it is now: mesh-vertex xyz (m).
    #[must_use]
    pub fn cloth(&self) -> Vec<f32> {
        flat(&self.scene.positions()).collect()
    }

    /// The cloth's triangles, as mesh-vertex indices.
    #[must_use]
    pub fn cloth_triangles(&self) -> Vec<u32> {
        self.scene
            .mesh
            .faces
            .iter()
            .flatten()
            .map(|&v| v as u32)
            .collect()
    }

    /// Each mesh vertex's place on its flat pattern piece (mm).
    #[must_use]
    pub fn cloth_pattern_mm(&self) -> Vec<f32> {
        self.scene
            .mesh
            .verts2d
            .iter()
            .flat_map(|p| [p.x as f32, p.y as f32])
            .collect()
    }

    /// The displayed body with the arms at these angles: vertex xyz (m).
    #[must_use]
    pub fn display(&self, left_deg: f64, right_deg: f64) -> Vec<f32> {
        flat(&self.body.display_at(left_deg, right_deg)).collect()
    }

    /// The displayed body's triangles.
    #[must_use]
    pub fn display_triangles(&self) -> Vec<u32> {
        self.body
            .display
            .triangles
            .iter()
            .flatten()
            .map(|&v| v as u32)
            .collect()
    }

    /// The arm angles the session has reached, left then right (degrees).
    #[must_use]
    pub fn arms(&self) -> [f64; 2] {
        self.arms
    }

    /// The last accepted move's work: `trials` and `substeps`, and per accepted substep its solver
    /// report's `newton_iterations`, `ccd_limited`, `halvings`, `line_search_failures`, `factorizations`
    /// and `pcg_iterations`; null before the first. Moves crossing baked samples additionally retain
    /// their shared path boundaries in `body_sample_times_s`.
    #[must_use]
    pub fn last_move(&self) -> &Value {
        &self.last_move
    }

    /// Moves the arms to these angles over `seconds` of cloth time and returns every substep the solver
    /// accepted: for each, its time since the move began (s) and then the cloth's mesh-vertex xyz (m).
    /// Angles are clamped to the body's sampled range. The same angles as now let the cloth settle.
    /// Crossing either arm's baked samples follows those samples in order, with one shared trial and
    /// output budget. Every segment must succeed before the session or its last-move receipt changes.
    ///
    /// # Errors
    /// Non-finite angles, a duration outside (0, 2] s, and every refusal of the moving-body step. On any
    /// error the session is exactly where it was.
    pub fn advance(
        &mut self,
        left_deg: f64,
        right_deg: f64,
        seconds: f64,
    ) -> Result<Vec<f32>, String> {
        if !left_deg.is_finite() || !right_deg.is_finite() {
            return Err("arm angles must be finite".into());
        }
        if !seconds.is_finite() || seconds <= 0.0 || seconds > MAX_MOVE_S {
            return Err(format!(
                "a move lasts more than 0 and at most {MAX_MOVE_S} s"
            ));
        }
        let (low, high) = self.body.arm_range();
        let (left, right) = (left_deg.clamp(low, high), right_deg.clamp(low, high));
        let fractions = self.body.motion_fractions(self.arms, [left, right]);
        let report = if fractions.len() == 1 {
            // Retain the established arithmetic, tolerances and receipts within one sampled cell.
            let target = self.body.collision_at(left, right);
            match self.carry_m {
                Some(radius) => advance_body_carried(
                    &mut self.scene.solver,
                    &target,
                    seconds,
                    self.motion,
                    radius,
                ),
                None => advance_body(&mut self.scene.solver, &target, seconds, self.motion),
            }
            .map_err(|refusal| refusal.to_string())?
        } else {
            let (solver, report) = self.advance_sampled([left, right], seconds, &fractions)?;
            self.scene.solver = solver;
            report
        };
        self.arms = [left, right];
        let per_substep = |field: fn(&ftx_drape::SolveReport) -> usize| {
            report
                .samples
                .iter()
                .map(|s| field(&s.solve))
                .collect::<Vec<_>>()
        };
        self.last_move = json!({
            "trials": report.trials,
            "substeps": report.samples.len(),
            "rejected_before_solve": report.rejected_before_solve,
            "rejected_unsolved": report.rejected_unsolved,
            "rejected_after_solve": report.rejected_after_solve,
            "rejected_newton_iterations": report.rejected_newton_iterations,
            "newton_iterations": per_substep(|r| r.iterations),
            "ccd_limited": per_substep(|r| r.ccd_limited),
            "halvings": per_substep(|r| r.halvings),
            "line_search_failures": per_substep(|r| r.line_search_failures),
            "factorizations": per_substep(|r| r.factorizations),
            "pcg_iterations": per_substep(|r| r.pcg_iterations),
        });
        if fractions.len() > 1 {
            self.last_move["body_sample_times_s"] =
                json!(fractions.iter().map(|f| seconds * f).collect::<Vec<_>>());
        }
        let mut samples =
            Vec::with_capacity(report.samples.len() * (1 + 3 * self.scene.weld.len()));
        for sample in &report.samples {
            samples.push(sample.time_s as f32);
            for &welded in &self.scene.weld {
                let p = sample.cloth[welded];
                samples.extend([p.x as f32, p.y as f32, p.z as f32]);
            }
        }
        Ok(samples)
    }

    fn advance_sampled(
        &self,
        to: [f64; 2],
        seconds: f64,
        fractions: &[f64],
    ) -> Result<(ftx_drape::Solver, MotionReport), String> {
        let count = self
            .body
            .collision
            .base
            .len()
            .checked_add(self.scene.solver.x.len())
            .ok_or("body motion sample size overflow")?;
        // Every sampled chord needs at least one trial and one complete retained sample. Refuse
        // before cloning the solver when even that lower bound exceeds the original allowance.
        if fractions.len() > self.motion.max_trials {
            return Err(
                "body motion trial budget exceeded by sampled path; state unchanged".into(),
            );
        }
        if count == 0 || fractions.len() > self.motion.max_sample_vertices / count {
            return Err(
                "body motion sample budget exceeded by sampled path; state unchanged".into(),
            );
        }
        let mut work = self.scene.solver.clone();
        let mut report = MotionReport {
            trials: 0,
            pairs_tested: 0,
            samples: Vec::new(),
            rejected_before_solve: 0,
            rejected_unsolved: 0,
            rejected_after_solve: 0,
            rejected_newton_iterations: 0,
        };
        let mut time = 0.0;
        let mut replay_time = 0.0_f32;
        for &fraction in fractions {
            let end = seconds * fraction;
            let duration = end - time;
            if !duration.is_finite() || duration <= 0.0 {
                return Err(
                    "body sample crossings require distinct positive times; state unchanged".into(),
                );
            }
            if report.trials >= self.motion.max_trials {
                return Err(
                    "body motion trial budget exceeded across sampled path; state unchanged".into(),
                );
            }
            let used = report.samples.len() * count;
            if used >= self.motion.max_sample_vertices {
                return Err(
                    "body motion sample budget exceeded across sampled path; state unchanged"
                        .into(),
                );
            }
            let angles = if fraction == 1.0 {
                to
            } else {
                std::array::from_fn(|side| {
                    self.arms[side] + (to[side] - self.arms[side]) * fraction
                })
            };
            let target = self.body.collision_at(angles[0], angles[1]);
            let options = MotionOptions {
                max_trials: self.motion.max_trials - report.trials,
                max_sample_vertices: self.motion.max_sample_vertices - used,
                ..self.motion
            };
            // A crossed source sample is still an intermediate point of this move. Preserve
            // the caller's intermediate tolerance there; only the requested endpoint uses
            // the session's final tolerance.
            let final_tolerance = work.tol;
            if fraction < 1.0
                && let Some(tolerance) = options.intermediate_tol
            {
                work.tol = tolerance;
            }
            let segment = match self.carry_m {
                Some(radius) => advance_body_carried(&mut work, &target, duration, options, radius),
                None => advance_body(&mut work, &target, duration, options),
            };
            work.tol = final_tolerance;
            let mut segment = segment.map_err(|refusal| refusal.to_string())?;
            for sample in &mut segment.samples {
                sample.time_s = if sample.time_s == duration {
                    end
                } else {
                    time + sample.time_s
                };
                let visible_time = sample.time_s as f32;
                if !visible_time.is_finite() || visible_time <= replay_time {
                    return Err(
                        "body motion samples have indistinguishable replay times; state unchanged"
                            .into(),
                    );
                }
                replay_time = visible_time;
            }
            report.trials += segment.trials;
            report.pairs_tested += segment.pairs_tested;
            report.rejected_before_solve += segment.rejected_before_solve;
            report.rejected_unsolved += segment.rejected_unsolved;
            report.rejected_after_solve += segment.rejected_after_solve;
            report.rejected_newton_iterations += segment.rejected_newton_iterations;
            report.samples.append(&mut segment.samples);
            time = end;
        }
        Ok((work, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::det;

    const PATTERN: &str =
        include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json");
    const RING: usize = 16;

    // A cylinder of radius 30 mm for a body. Its "left arm" is the whole cylinder, carried `shift_m` along x
    // between the two sampled angles; the "right arm" moves nothing.
    fn baked(shift_m: f64) -> (String, Vec<u8>) {
        let mut vertices = Vec::new();
        for z in [-0.02, 0.12] {
            for i in 0..RING {
                let (s, c) = det::sin_cos(i as f64 * std::f64::consts::TAU / RING as f64);
                vertices.push([0.03 * c, 0.03 * s, z]);
            }
        }
        let mut triangles = Vec::new();
        for i in 0..RING {
            let j = (i + 1) % RING;
            triangles.extend([i, j, j + RING, i, j + RING, i + RING]);
        }
        let mut data = Vec::new();
        let mut arrays = serde_json::Map::new();
        let mut put_floats = |name: &str, values: &[f64], data: &mut Vec<u8>| {
            arrays.insert(
                name.into(),
                json!({"dtype": "f32", "offset": data.len(), "count": values.len()}),
            );
            for &v in values {
                data.extend((v as f32).to_le_bytes());
            }
        };
        let xyz: Vec<f64> = vertices.iter().flatten().copied().collect();
        // Two samples: no displacement at the base angle, then the shift.
        let mut left = vec![0.0; 3 * 2 * RING];
        left.extend((0..2 * RING).flat_map(|_| [shift_m, 0.0, 0.0]));
        for stem in ["collision", "display"] {
            put_floats(&format!("{stem}_vertices"), &xyz, &mut data);
            put_floats(&format!("left_{stem}_offsets"), &left, &mut data);
            put_floats(&format!("right_{stem}_offsets"), &[], &mut data);
        }
        put_floats("radius_torso", &[30.0; 400], &mut data);
        let mut put_indices = |name: &str, values: &[usize], data: &mut Vec<u8>| {
            arrays.insert(
                name.into(),
                json!({"dtype": "u32", "offset": data.len(), "count": values.len()}),
            );
            for &v in values {
                data.extend((v as u32).to_le_bytes());
            }
        };
        let moved: Vec<usize> = (0..2 * RING).collect();
        for stem in ["collision", "display"] {
            put_indices(&format!("{stem}_triangles"), &triangles, &mut data);
            put_indices(&format!("left_{stem}_indices"), &moved, &mut data);
            put_indices(&format!("right_{stem}_indices"), &[], &mut data);
        }
        let header = json!({
            "schema": SCHEMA, "base_arm_deg": 35.0, "arm_angles_deg": [35.0, 45.0], "radius_h0_mm": -200.0,
            "frames": {"torso": {"origin": [0, 0, 0], "up": [0, 0, 1], "front": [0, -1, 0], "lat": [1, 0, 0]}},
            "landmarks": {"torso": {"waist": 20.0}},
            "arrays": arrays,
        });
        (header.to_string(), data)
    }

    // The band is sewn round the cylinder with no gravity phase, and gravity is then switched off: nothing
    // holds a loose band up on a bare cylinder, and the test is about the body pushing the cloth.
    fn session(shift_m: f64) -> Session {
        let (header, data) = baked(shift_m);
        let body = PosableBody::parse(&header, &data).unwrap();
        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let settings = SessionSettings {
            sew_steps: 24,
            drape_steps: 0,
            ..SessionSettings::default()
        };
        let mut session = Session::start(body, &pattern, &json!({}), settings, |_| {}).unwrap();
        session.scene.solver.gravity = Vec3::ZERO;
        session
    }

    // An L-shaped sampled path: the cylinder first pushes right, then moves forward. A direct
    // endpoint chord misses the intermediate 15 mm excursion and cannot describe the same cloth.
    fn turning_session() -> Session {
        let mut session = session(0.015);
        session.body.angles = vec![35.0, 40.0, 45.0];
        for surface in [&mut session.body.collision, &mut session.body.display] {
            surface
                .left
                .offsets
                .push(vec![Vec3::new(0.0, 0.015, 0.0); 2 * RING]);
            surface.right.offsets.push(Vec::new());
        }
        session.motion.max_step_s = 0.025;
        session
    }

    #[test]
    fn motion_partitions_both_arms_at_every_crossed_sample_in_either_direction() {
        let (header, data) = baked(0.0);
        let mut body = PosableBody::parse(&header, &data).unwrap();
        body.angles = vec![0.0, 10.0, 20.0, 30.0];
        assert_eq!(
            body.motion_fractions([0.0, 30.0], [30.0, 10.0]),
            vec![1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0]
        );
        assert_eq!(
            body.motion_fractions([30.0, 30.0], [0.0, 0.0]),
            vec![1.0 / 3.0, 2.0 / 3.0, 1.0],
            "shared crossings do not create zero-duration segments"
        );
        assert_eq!(body.motion_fractions([10.0, 20.0], [20.0, 10.0]), vec![1.0]);
        assert_eq!(body.motion_fractions([15.0, 15.0], [15.0, 15.0]), vec![1.0]);
    }

    #[test]
    fn a_sampled_move_checks_the_rendered_path_and_matches_separate_checked_segments() {
        for (carry, tolerance) in [(None, None), (Some(0.03), None), (Some(0.03), Some(5e-3))] {
            let mut session = turning_session();
            session.carry_m = carry;
            session.motion.intermediate_tol = tolerance;
            let original = session.scene.solver.clone();
            let fractions = session.body.motion_fractions(session.arms, [45.0, 35.0]);
            let (expected, report) = session
                .advance_sampled([45.0, 35.0], 0.25, &fractions)
                .unwrap();
            assert!(report.samples.iter().any(|sample| sample.time_s == 0.125));
            for sample in &report.samples {
                let displayed = session
                    .body
                    .collision_at(35.0 + 10.0 * sample.time_s / 0.25, 35.0);
                assert!(
                    sample
                        .body
                        .iter()
                        .zip(displayed)
                        .all(|(a, b)| (*a - b).norm() < 1e-14)
                );
            }
            let mut independent = original.clone();
            let mut trials = 0;
            let mut samples = 0;
            let count = original.x.len() + session.body.collision.base.len();
            for angle in [40.0, 45.0] {
                let target = session.body.collision_at(angle, 35.0);
                let options = MotionOptions {
                    max_trials: session.motion.max_trials - trials,
                    max_sample_vertices: session.motion.max_sample_vertices - samples * count,
                    ..session.motion
                };
                independent.tol = if angle == 40.0 {
                    tolerance.unwrap_or(original.tol)
                } else {
                    original.tol
                };
                let segment = match carry {
                    Some(radius) => {
                        advance_body_carried(&mut independent, &target, 0.125, options, radius)
                    }
                    None => advance_body(&mut independent, &target, 0.125, options),
                }
                .unwrap();
                independent.tol = original.tol;
                trials += segment.trials;
                samples += segment.samples.len();
            }
            assert_eq!(format!("{expected:?}"), format!("{independent:?}"));
            assert_eq!(report.trials, trials);
            assert_eq!(report.samples.len(), samples);
            let actual = session.advance(45.0, 35.0, 0.25).unwrap();
            let stride = 1 + session.cloth().len();
            let times: Vec<f32> = actual
                .chunks_exact(stride)
                .map(|sample| sample[0])
                .collect();
            assert_eq!(
                times,
                report
                    .samples
                    .iter()
                    .map(|sample| sample.time_s as f32)
                    .collect::<Vec<_>>()
            );
            assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
            assert_eq!(times.last(), Some(&0.25));
            assert_eq!(
                session.last_move()["body_sample_times_s"],
                json!([0.125, 0.25])
            );
            assert_eq!(session.last_move()["trials"], json!(trials));
            assert_eq!(
                format!("{:?}", session.scene.solver),
                format!("{independent:?}")
            );
            assert_eq!(session.arms(), [45.0, 35.0]);
            assert_eq!(
                &actual[actual.len() - stride + 1..],
                session.cloth().as_slice()
            );
        }
    }

    #[test]
    fn late_sampled_move_budget_failures_restore_the_entire_session_and_can_be_replayed() {
        let mut session = turning_session();
        session.carry_m = Some(0.03);
        session.advance(35.0, 35.0, 0.01).unwrap();
        let original = session.scene.solver.clone();
        let before_receipt = session.last_move().clone();
        let before_replay = session.replay().to_vec();
        let before_summary = session.summary().clone();
        let original_options = session.motion;
        let count = original.x.len() + session.body.collision.base.len();
        let mut first = original.clone();
        let first_report = advance_body_carried(
            &mut first,
            &session.body.collision_at(40.0, 35.0),
            0.125,
            session.motion,
            0.03,
        )
        .unwrap();
        assert!(first_report.samples.len() > 1 && first_report.trials > 1);
        for trial_budget in [true, false] {
            session.motion = if trial_budget {
                MotionOptions {
                    max_trials: first_report.trials + 1,
                    ..original_options
                }
            } else {
                MotionOptions {
                    max_sample_vertices: (first_report.samples.len() + 1) * count,
                    ..original_options
                }
            };
            let error = session.advance(45.0, 35.0, 0.25).unwrap_err();
            assert!(
                error.contains(if trial_budget {
                    "trial budget"
                } else {
                    "sample budget"
                }),
                "{error}"
            );
            assert_eq!(
                format!("{:?}", session.scene.solver),
                format!("{original:?}")
            );
            assert_eq!(session.arms(), [35.0, 35.0]);
            assert_eq!(session.last_move(), &before_receipt);
            assert_eq!(session.replay(), before_replay);
            assert_eq!(session.summary(), &before_summary);
        }
        session.motion = original_options;
        let fractions = session.body.motion_fractions(session.arms, [45.0, 35.0]);
        let (expected, _) = session
            .advance_sampled([45.0, 35.0], 0.25, &fractions)
            .unwrap();
        session.advance(45.0, 35.0, 0.25).unwrap();
        assert_eq!(
            format!("{:?}", session.scene.solver),
            format!("{expected:?}")
        );
    }

    #[test]
    fn a_baked_body_interpolates_each_arm_between_its_samples_and_refuses_bad_data() {
        let (header, data) = baked(0.004);
        let body = PosableBody::parse(&header, &data).unwrap();
        assert_eq!(body.arm_range(), (35.0, 45.0));
        let base = body.collision_at(35.0, 35.0);
        let half = body.collision_at(40.0, 35.0);
        let full = body.collision_at(90.0, 35.0); // clamped to the last sample
        assert!((half[RING].x - base[RING].x - 0.002).abs() < 1e-7);
        assert!((full[RING].x - base[RING].x - 0.004).abs() < 1e-7);
        assert!((full[0].y - base[0].y).abs() < 1e-12 && (full[0].z - base[0].z).abs() < 1e-12);
        assert_eq!(
            body.collision_at(35.0, 45.0),
            base,
            "the right arm moves nothing here"
        );
        assert_eq!(
            body.display_at(40.0, 35.0),
            half,
            "both surfaces are baked alike here"
        );
        assert_eq!(
            body.display_at(f64::NAN, 35.0),
            base,
            "a non-finite display angle falls back to the base pose instead of NaN vertices"
        );
        assert_eq!(body.section_radius("torso", 20.4), 30.0);
        assert_eq!(body.section_radius("torso", 5000.0), 0.0);
        assert_eq!(body.section_radius("arm_R", 20.0), 0.0);
        assert_eq!(body.landmark_h("torso", "waist"), Some(20.0));
        assert_eq!(body.landmark_h("torso", "knee"), None);

        assert!(
            PosableBody::parse(&header, &data[..data.len() - 8]).is_err(),
            "truncated data"
        );
        let other = header.replace(SCHEMA, "ftx-posable-body/2");
        assert!(
            PosableBody::parse(&other, &data)
                .unwrap_err()
                .contains("schema")
        );
        let unordered = header.replace("[35.0,45.0]", "[45.0,35.0]");
        assert!(
            PosableBody::parse(&unordered, &data)
                .unwrap_err()
                .contains("arm angles")
        );
    }

    #[test]
    fn a_drape_can_start_with_the_arms_at_any_sampled_pose_that_has_its_frames() {
        let (header, data) = baked(0.004);
        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let settings = SessionSettings {
            sew_steps: 24,
            drape_steps: 0,
            ..SessionSettings::default()
        };
        let start = |header: &str, left: f64| {
            let body = PosableBody::parse(header, &data).unwrap();
            Session::start_posed(body, &pattern, &json!({}), settings, left, 35.0, |_| {})
        };
        // Without baked arm frames only the base pose can start, and only sampled angles ever can.
        assert!(
            start(&header, 45.0)
                .err()
                .unwrap()
                .contains("no baked arm_L frames")
        );
        assert!(
            start(&header, 40.0)
                .err()
                .unwrap()
                .contains("sampled angle")
        );

        // With them, a drape starts on the body posed at the sample: here the cylinder carried 4 mm.
        let mut doc: Value = serde_json::from_str(&header).unwrap();
        let frame = |x: f64| {
            json!({"origin": [x, 0, 0], "up": [0, 0, 1], "front": [0, -1, 0], "lat": [1, 0, 0],
                   "landmarks": {"shoulder_tip": 100.0}})
        };
        doc["arm_frames"] =
            json!({"arm_L": [frame(0.0), frame(4.0)], "arm_R": [frame(0.0), frame(0.0)]});
        let posed = start(&doc.to_string(), 45.0).unwrap();
        assert_eq!(posed.arms(), [45.0, 35.0]);
        assert_eq!(posed.summary()["arms_deg"], json!([45.0, 35.0]));
        assert_eq!(posed.summary()["arm_angles_deg"], json!([35.0, 45.0]));
        let shifted = PosableBody::parse(&doc.to_string(), &data)
            .unwrap()
            .display_at(45.0, 35.0);
        let shown: Vec<f32> = shifted
            .iter()
            .flat_map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
        assert_eq!(posed.display(45.0, 35.0), shown);
        let inradius = 0.03 * det::cos(std::f64::consts::PI / RING as f64);
        assert!(
            posed
                .cloth()
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| (f64::from(p[0]) - 0.004).hypot(f64::from(p[1])) > inradius),
            "the band was sewn round the carried cylinder"
        );
        // A frame list that does not match the samples is refused.
        doc["arm_frames"]["arm_L"] = json!([frame(0.0)]);
        assert!(
            PosableBody::parse(&doc.to_string(), &data)
                .unwrap_err()
                .contains("one per sampled angle")
        );
    }

    #[test]
    fn a_session_moves_the_body_under_the_draped_cloth_with_the_checked_solver_step() {
        // The band's radius is 40 mm, so a 30 mm cylinder carried 15 mm sideways must push it.
        let mut session = session(0.015);
        let summary = session.summary().clone();
        let count = summary["cloth_vertices"].as_u64().unwrap() as usize;
        assert_eq!(summary["pieces"][0]["id"], "band");
        assert_eq!(summary["arm_range_deg"], json!([35.0, 45.0]));
        let frames = summary["frames"]["count"].as_u64().unwrap() as usize;
        assert_eq!(session.replay().len(), frames * count * 3);
        assert_eq!(session.cloth_pattern_mm().len(), count * 2);
        let before = session.cloth();
        assert_eq!(before.len(), count * 3);
        assert_eq!(
            &session.replay()[(frames - 1) * count * 3..],
            before.as_slice()
        );

        // A refused request leaves the session exactly where it was.
        assert!(session.advance(f64::NAN, 35.0, 0.1).is_err());
        assert!(session.advance(45.0, 35.0, 0.0).is_err());
        assert_eq!(session.cloth(), before);
        assert_eq!(session.arms(), [35.0, 35.0]);

        let samples = session.advance(45.0, 35.0, 0.25).unwrap();
        let stride = 1 + count * 3;
        assert!(!samples.is_empty() && samples.len().is_multiple_of(stride));
        let last = &samples[samples.len() - stride..];
        assert!(
            (last[0] - 0.25).abs() < 1e-6,
            "the last accepted substep ends the move"
        );
        assert_eq!(&last[1..], session.cloth().as_slice());
        assert_eq!(session.arms(), [45.0, 35.0]);
        let mean_x = |cloth: &[f32]| {
            cloth
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| f64::from(p[0]))
                .sum::<f64>()
                / count as f64
        };
        let pushed = mean_x(&session.cloth()) - mean_x(&before);
        assert!(
            pushed > 0.003,
            "the cylinder carried the band with it: {pushed} m"
        );
        // The cloth stays outside the moved cylinder (a 16-sided prism: its faces lie inside the circle).
        let inradius = 0.03 * det::cos(std::f64::consts::PI / RING as f64);
        let outside = session
            .cloth()
            .as_chunks::<3>()
            .0
            .iter()
            .all(|p| (f64::from(p[0]) - 0.015).hypot(f64::from(p[1])) > inradius);
        assert!(outside);
    }

    #[test]
    fn a_carried_move_reaches_the_same_pose_in_fewer_checked_substeps() {
        let move_samples = |carry_m: Option<f64>| {
            let mut session = session(0.015);
            session.carry_m = carry_m;
            let count = session.summary()["cloth_vertices"].as_u64().unwrap() as usize;
            let samples = session.advance(45.0, 35.0, 0.25).unwrap();
            let inradius = 0.03 * det::cos(std::f64::consts::PI / RING as f64);
            assert!(
                session
                    .cloth()
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .all(|p| (f64::from(p[0]) - 0.015).hypot(f64::from(p[1])) > inradius),
                "the cloth stays outside the moved cylinder (carry {carry_m:?})"
            );
            assert_eq!(session.arms(), [45.0, 35.0]);
            // Every trial was accepted or rejected for exactly one recorded reason.
            let work = session.last_move();
            let field = |name: &str| work[name].as_u64().unwrap();
            assert_eq!(
                field("trials"),
                field("substeps")
                    + field("rejected_before_solve")
                    + field("rejected_unsolved")
                    + field("rejected_after_solve"),
                "{work}"
            );
            assert_eq!(
                work["ccd_limited"].as_array().unwrap().len() as u64,
                field("substeps")
            );
            samples.len() / (1 + count * 3)
        };
        let (plain, carried) = (move_samples(None), move_samples(Some(0.02)));
        assert!(
            carried < plain,
            "carrying the band took {carried} substeps, at rest {plain}"
        );
    }

    #[test]
    fn a_looser_intermediate_move_tolerance_is_opt_in_bounded_and_leaves_the_solver_alone() {
        assert_eq!(SessionSettings::default().motion_intermediate_tol_m_s, None);
        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let (header, data) = baked(0.015);
        for tol in [0.0, -1e-3, 0.2] {
            let settings: SessionSettings =
                serde_json::from_value(json!({ "motion_intermediate_tol_m_s": tol })).unwrap();
            let body = PosableBody::parse(&header, &data).unwrap();
            let err = Session::start(body, &pattern, &json!({}), settings, |_| {})
                .err()
                .expect("an out-of-range intermediate tolerance is refused");
            assert!(err.contains("intermediate move tolerance"), "{err}");
        }
        // The setting reaches the move policy (as `session()` builds its session, plus the tolerance).
        let settings = SessionSettings {
            sew_steps: 24,
            drape_steps: 0,
            motion_intermediate_tol_m_s: Some(3e-3),
            ..SessionSettings::default()
        };
        let body = PosableBody::parse(&header, &data).unwrap();
        let mut session = Session::start(body, &pattern, &json!({}), settings, |_| {}).unwrap();
        session.scene.solver.gravity = Vec3::ZERO;
        assert_eq!(session.motion.intermediate_tol, Some(3e-3));
        let tol = session.scene.solver.tol;
        session.advance(45.0, 35.0, 0.25).unwrap();
        assert_eq!(session.arms(), [45.0, 35.0]);
        assert_eq!(session.scene.solver.tol, tol);
    }

    #[test]
    fn session_settings_accepts_opt_in_realism_options_while_preserving_defaults() {
        let def: SessionSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(def.newton_strategy, NewtonStrategy::ProjectedNewton);
        assert_eq!(def.integrator, TimeIntegrator::ImplicitEuler);
        assert_eq!(def.bending_model, BendingModel::Quadratic);
        assert_eq!(def.compression_model, CompressionModel::AxisAligned);
        assert_eq!(def.compression, DrapeOptions::default().compression);
        assert!(def.damping.is_default());
        assert!(def.adaptive_barrier.is_none());
        assert!(def.stick_slip.is_none());
        assert!(def.self_stick_slip.is_none());
        assert!(def.mesh.is_default());
        assert!(def.construction.is_default());
        assert!(def.start_self_repulsion.is_none());

        let settings: SessionSettings = serde_json::from_value(json!({
            "spacing_mm": 12.0,
            "sew_steps": 24,
            "drape_steps": 0,
            "self_contact_mm": 2.0,
            "newton_strategy": { "kind": "adaptive_ilu", "pcg_iters": 40, "max_shifts": 6 },
            "integrator": { "kind": "generalized_alpha", "rho_inf": 0.25 },
            "bending_model": "discrete_shells",
            "compression_model": "principal",
            "compression": 0.05,
            "damping": {
                "air_drag": { "rho_air_kg_m3": 1.225, "c_normal": 1.2, "c_tangent": 0.05, "eps_v_m_s": 1e-3, "air_velocity_m_s": [0.5, -1.5, 0.25] },
                "rayleigh_membrane_s": 1e-4,
                "rayleigh_bending_s": 2e-4
            },
            "adaptive_barrier": {
                "target_gap_fraction": 0.5,
                "min_gap_fraction": 0.05,
                "growth_factor": 2.0,
                "membrane_floor_scale": 1e-5,
                "kappa_min": 1e-3,
                "kappa_max": 1e5
            },
            "stick_slip": { "mu_s": 0.5, "mu_k": 0.3, "transition_v_m_s": 0.02 },
            "self_stick_slip": { "mu_s": 0.45, "mu_k": 0.25, "transition_v_m_s": 0.02 },
            "mesh": { "min_spacing_mm": 8.0, "cvt_iterations": 2 },
            "construction": {
                "hem_width_mm": 15.0,
                "hem_mass_multiplier": 2.0,
                "hem_bending_multiplier": 4.0,
                "seam_width_mm": 8.0,
                "seam_mass_multiplier": 1.5,
                "seam_bending_multiplier": 2.5
            },
            "start_self_repulsion": { "min_separation_m": 0.004, "stiffness": 0.5, "period": 5 }
        }))
        .unwrap();
        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let mut uncharacterized = pattern.clone();
        uncharacterized.as_object_mut().unwrap().remove("fabric");
        let (header, data) = baked(0.015);

        // Characterized material records refuse uniform stick-slip / construction overrides fail-closed.
        let body_char = PosableBody::parse(&header, &data).unwrap();
        let err = Session::start(body_char, &pattern, &json!({}), settings, |_| {})
            .err()
            .expect("characterized material record must refuse uniform stick-slip override");
        assert!(err.contains("characterized material record"), "{err}");

        // Uncharacterized pattern exercises all opt-in FTX-27 options live through start + advance.
        let body = PosableBody::parse(&header, &data).unwrap();
        let mut live =
            Session::start(body, &uncharacterized, &json!({}), settings, |_| {}).unwrap();
        assert_eq!(
            live.summary()["report"]["numerical"]["damping"]["air_drag"]["air_velocity_m_s"],
            json!([0.5, -1.5, 0.25])
        );
        assert_eq!(
            live.scene.solver.damping.air_drag.unwrap().air_velocity_m_s,
            [0.5, -1.5, 0.25]
        );
        live.scene.solver.gravity = Vec3::ZERO;
        let samples = live.advance(40.0, 35.0, 0.15).unwrap();
        assert!(!samples.is_empty());
        assert_eq!(live.arms(), [40.0, 35.0]);
        assert_eq!(
            live.scene.solver.damping.air_drag.unwrap().air_velocity_m_s,
            [0.5, -1.5, 0.25]
        );

        // Fail-closed: self_stick_slip without self_contact_mm > 0 must be refused.
        let bad: SessionSettings = serde_json::from_value(json!({
            "sew_steps": 12,
            "drape_steps": 0,
            "self_contact_mm": 0.0,
            "self_stick_slip": { "mu_s": 0.5, "mu_k": 0.3, "transition_v_m_s": 0.02 }
        }))
        .unwrap();
        let body2 = PosableBody::parse(&header, &data).unwrap();
        assert!(Session::start(body2, &uncharacterized, &json!({}), bad, |_| {}).is_err());

        // Fail-closed: overflowing ambient wind must refuse before meshing/stepping.
        let bad_wind: SessionSettings = serde_json::from_value(json!({
            "sew_steps": 12,
            "drape_steps": 0,
            "damping": {
                "air_drag": {
                    "rho_air_kg_m3": 1.225,
                    "c_normal": 1.2,
                    "c_tangent": 0.05,
                    "eps_v_m_s": 1e-3,
                    "air_velocity_m_s": [1e120, 0.0, 0.0]
                }
            }
        }))
        .unwrap();
        let body_wind = PosableBody::parse(&header, &data).unwrap();
        assert!(Session::start(body_wind, &uncharacterized, &json!({}), bad_wind, |_| {}).is_err());

        // Fail-closed: excessive browser solver work budgets must be refused before meshing.
        let over_budget: SessionSettings = serde_json::from_value(json!({
            "sew_steps": 12,
            "drape_steps": 0,
            "newton_strategy": { "kind": "adaptive_ilu", "pcg_iters": 512, "max_shifts": 6 }
        }))
        .unwrap();
        let body3 = PosableBody::parse(&header, &data).unwrap();
        let budget_err = Session::start(body3, &uncharacterized, &json!({}), over_budget, |_| {})
            .err()
            .expect("over-budget adaptive_ilu pcg_iters must be refused");
        assert!(
            budget_err.contains("browser solver work budget"),
            "{budget_err}"
        );
    }
}

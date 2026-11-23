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
use ftx_drape::motion::{MotionOptions, advance_body};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape};
use ftx_drape::scene::{PartFrames, Scene};
use ftx_drape::stepping::SubdivisionOptions;
use ftx_drape::{BendingModel, CompressionModel, PartFrame};
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

#[derive(Deserialize)]
struct Header {
    schema: String,
    base_arm_deg: f64,
    arm_angles_deg: Vec<f64>,
    radius_h0_mm: f64,
    frames: BTreeMap<String, FrameDoc>,
    landmarks: BTreeMap<String, BTreeMap<String, f64>>,
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

/// A baked synthetic body (`ftx-posable-body/1`): what a drape places pieces by, collides with and shows,
/// with each arm sampled at a list of abduction angles.
#[derive(Debug)]
pub struct PosableBody {
    base_deg: f64,
    angles: Vec<f64>,
    frames: BTreeMap<String, PartFrame>,
    landmarks: BTreeMap<String, BTreeMap<String, f64>>,
    radius: BTreeMap<String, Vec<f64>>,
    radius_h0: f64,
    collision: Surface,
    display: Surface,
}

fn bytes_of<'a>(header: &Header, data: &'a [u8], name: &str, dtype: &str) -> Result<&'a [u8], String> {
    let entry = header
        .arrays
        .get(name)
        .ok_or_else(|| format!("posable body: array {name} is missing"))?;
    if entry.dtype != dtype {
        return Err(format!("posable body: array {name} is {}, not {dtype}", entry.dtype));
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
        .chunks_exact(4)
        .map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
        .collect();
    if values.iter().all(|v| v.is_finite()) {
        Ok(values)
    } else {
        Err(format!("posable body: array {name} holds a non-finite value"))
    }
}

fn indices(header: &Header, data: &[u8], name: &str, below: usize) -> Result<Vec<usize>, String> {
    bytes_of(header, data, name, "u32")?
        .chunks_exact(4)
        .map(|b| {
            let index = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
            (index < below)
                .then_some(index)
                .ok_or_else(|| format!("posable body: array {name} indexes vertex {index} of {below}"))
        })
        .collect()
}

fn points(values: &[f64], name: &str) -> Result<Vec<Vec3>, String> {
    if values.len() % 3 != 0 {
        return Err(format!("posable body: array {name} is not a list of xyz triples"));
    }
    Ok(values
        .chunks_exact(3)
        .map(|p| Vec3::new(p[0], p[1], p[2]))
        .collect())
}

fn arm(header: &Header, data: &[u8], stem: &str, vertices: usize, samples: usize) -> Result<Arm, String> {
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
        all.chunks_exact(moved.len()).map(<[Vec3]>::to_vec).collect()
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
    if base.is_empty() || corners.is_empty() || corners.len() % 3 != 0 {
        return Err(format!("posable body: the {stem} surface is empty or its triangles are incomplete"));
    }
    Ok(Surface {
        triangles: corners.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
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
        let header: Header = serde_json::from_str(header).map_err(|e| format!("posable body header: {e}"))?;
        if header.schema != SCHEMA {
            return Err(format!("posable body: schema {} is not {SCHEMA}", header.schema));
        }
        let angles = header.arm_angles_deg.clone();
        let increasing = angles.windows(2).all(|pair| pair[0] < pair[1]);
        if angles.len() < 2
            || !increasing
            || angles.iter().any(|a| !(0.0..=180.0).contains(a))
            || !angles.contains(&header.base_arm_deg)
        {
            return Err("posable body: arm angles must increase within 0–180° and include the base angle".into());
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
            radius.insert(part.clone(), floats(&header, data, &format!("radius_{part}"))?);
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
        Ok(Self {
            base_deg: header.base_arm_deg,
            radius_h0: header.radius_h0_mm,
            collision: surface(&header, data, "collision", angles.len())?,
            display: surface(&header, data, "display", angles.len())?,
            angles,
            frames,
            landmarks: header.landmarks,
            radius,
        })
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
    /// Hinge bending formulation.
    pub bending_model: BendingModel,
    /// Membrane compression-softening formulation.
    pub compression_model: CompressionModel,
    /// Longest body-motion substep of a move (s).
    pub motion_step_s: f64,
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
            bending_model: drape.bending_model,
            compression_model: drape.compression_model,
            motion_step_s: MotionOptions::default().max_step_s,
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
}

fn flat(points: &[Vec3]) -> impl Iterator<Item = f32> + '_ {
    points.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32])
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
        mut progress: impl FnMut(&str),
    ) -> Result<Self, String> {
        let SessionSettings {
            spacing_mm,
            sew_steps,
            drape_steps,
            max_settle_steps,
            time_subdivision,
            self_contact_mm,
            bending_model,
            compression_model,
            motion_step_s,
        } = settings;
        if !self_contact_mm.is_finite() || self_contact_mm < 0.0 {
            return Err("self-contact thickness must be finite and nonnegative".into());
        }
        if !motion_step_s.is_finite() || !(1e-4..=0.1).contains(&motion_step_s) {
            return Err("the motion step must lie between 0.0001 and 0.1 s".into());
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
        let defaults = DrapeOptions::default();
        let mut options = DrapeOptions {
            spacing: spacing_mm,
            sew_steps,
            drape_steps,
            max_settle_steps,
            time_subdivision: time_subdivision.then(SubdivisionOptions::default),
            bending_model,
            compression_model,
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
        let collision = body.collision_at(base, base);
        let display = body.display_at(base, base);
        let run = drape(
            &pattern,
            fit_spec,
            &DrapeBody {
                frames: &body,
                collision: (&collision, &body.collision.triangles),
                display: (&display, &body.display.triangles),
                arm_deg: base,
            },
            &options,
            &mut progress,
        )
        .map_err(|refusal| refusal.to_string())?;
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
            "report": run.report,
        });
        let replay = run.fold.frames.iter().flat_map(|frame| flat(frame)).collect();
        Ok(Self {
            body,
            scene: run.scene,
            arms: [base, base],
            replay,
            summary,
            motion: MotionOptions {
                max_step_s: motion_step_s,
                ..MotionOptions::default()
            },
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

    /// Moves the arms to these angles over `seconds` of cloth time and returns every substep the solver
    /// accepted: for each, its time since the move began (s) and then the cloth's mesh-vertex xyz (m).
    /// Angles are clamped to the body's sampled range. The same angles as now let the cloth settle.
    ///
    /// # Errors
    /// Non-finite angles, a duration outside (0, 2] s, and every refusal of the moving-body step. On any
    /// error the session is exactly where it was.
    pub fn advance(&mut self, left_deg: f64, right_deg: f64, seconds: f64) -> Result<Vec<f32>, String> {
        if !left_deg.is_finite() || !right_deg.is_finite() {
            return Err("arm angles must be finite".into());
        }
        if !seconds.is_finite() || seconds <= 0.0 || seconds > MAX_MOVE_S {
            return Err(format!("a move lasts more than 0 and at most {MAX_MOVE_S} s"));
        }
        let (low, high) = self.body.arm_range();
        let (left, right) = (left_deg.clamp(low, high), right_deg.clamp(low, high));
        let target = self.body.collision_at(left, right);
        let report = advance_body(&mut self.scene.solver, &target, seconds, self.motion)
            .map_err(|refusal| refusal.to_string())?;
        self.arms = [left, right];
        let mut samples = Vec::with_capacity(report.samples.len() * (1 + 3 * self.scene.weld.len()));
        for sample in &report.samples {
            samples.push(sample.time_s as f32);
            for &welded in &self.scene.weld {
                let p = sample.cloth[welded];
                samples.extend([p.x as f32, p.y as f32, p.z as f32]);
            }
        }
        Ok(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::det;

    const PATTERN: &str = include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json");
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
            arrays.insert(name.into(), json!({"dtype": "f32", "offset": data.len(), "count": values.len()}));
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
            arrays.insert(name.into(), json!({"dtype": "u32", "offset": data.len(), "count": values.len()}));
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
        assert_eq!(body.collision_at(35.0, 45.0), base, "the right arm moves nothing here");
        assert_eq!(body.display_at(40.0, 35.0), half, "both surfaces are baked alike here");
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

        assert!(PosableBody::parse(&header, &data[..data.len() - 8]).is_err(), "truncated data");
        let other = header.replace(SCHEMA, "ftx-posable-body/2");
        assert!(PosableBody::parse(&other, &data).unwrap_err().contains("schema"));
        let unordered = header.replace("[35.0,45.0]", "[45.0,35.0]");
        assert!(PosableBody::parse(&unordered, &data).unwrap_err().contains("arm angles"));
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
        assert_eq!(&session.replay()[(frames - 1) * count * 3..], before.as_slice());

        // A refused request leaves the session exactly where it was.
        assert!(session.advance(f64::NAN, 35.0, 0.1).is_err());
        assert!(session.advance(45.0, 35.0, 0.0).is_err());
        assert_eq!(session.cloth(), before);
        assert_eq!(session.arms(), [35.0, 35.0]);

        let samples = session.advance(45.0, 35.0, 0.25).unwrap();
        let stride = 1 + count * 3;
        assert!(!samples.is_empty() && samples.len() % stride == 0);
        let last = &samples[samples.len() - stride..];
        assert!((last[0] - 0.25).abs() < 1e-6, "the last accepted substep ends the move");
        assert_eq!(&last[1..], session.cloth().as_slice());
        assert_eq!(session.arms(), [45.0, 35.0]);
        let mean_x = |cloth: &[f32]| cloth.chunks_exact(3).map(|p| f64::from(p[0])).sum::<f64>() / count as f64;
        let pushed = mean_x(&session.cloth()) - mean_x(&before);
        assert!(pushed > 0.003, "the cylinder carried the band with it: {pushed} m");
        // The cloth stays outside the moved cylinder (a 16-sided prism: its faces lie inside the circle).
        let inradius = 0.03 * det::cos(std::f64::consts::PI / RING as f64);
        let outside = session
            .cloth()
            .chunks_exact(3)
            .all(|p| (f64::from(p[0]) - 0.015).hypot(f64::from(p[1])) > inradius);
        assert!(outside);
    }
}

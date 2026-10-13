//! The garment scene (plan §11.3–11.4): a pattern meshed, placed around a body, pre-sewn and relaxed,
//! its sewing groups welded into single vertices, and handed to the contact cloth solver over the body's
//! skin; then the reference's schedule — sew at zero gravity, drape under gravity.
//!
//! Rest coordinates are each piece's flat 2D mesh in metres, rotated so the piece's grainline is +u (the
//! membrane's warp). Welding keeps every triangle's own rest coordinates (`ClothModel::from_triangles`),
//! so a seam joining sides of different lengths (ease, gathers) is strained, not rewritten.

use std::collections::BTreeMap;

use ftx_body::BodyFrames;
use ftx_core::np::linspace;
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det};
use ftx_pattern::{Pattern, truthy};
use serde_json::Value;

use crate::cloth::{ClothModel, SolveReport, Solver};
use crate::contact::{Body, Contact, ContactParams, Intersection};
use crate::elements::Membrane;
use crate::garment::GarmentMesh;
use crate::nearest::NearestTree;
use crate::start::{PartFrame, StartState, WrapPlacement, start_state, wrap_points};

/// Fabric parameters (spec 04 §5.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fabric {
    /// Areal density (kg/m²).
    pub areal_kg_m2: f64,
    /// Tensile stiffness (N/m), warp and weft.
    pub tensile_n_m: f64,
    /// Bending rigidity (N·m).
    pub bending_nm: f64,
}

impl Default for Fabric {
    /// The reference's defaults: 200 g/m², 8000 N/m, 3e-5 N·m.
    fn default() -> Self {
        Self {
            areal_kg_m2: 0.2,
            tensile_n_m: 8000.0,
            bending_nm: 3e-5,
        }
    }
}

impl Fabric {
    /// `pattern.fabric` (`areal_g_m2`, `tensile_N_m`, `bending_Nm`) over the defaults.
    #[must_use]
    pub fn of(pattern: &Pattern) -> Self {
        let f = &pattern.raw()["fabric"];
        let d = Self::default();
        Self {
            areal_kg_m2: f["areal_g_m2"]
                .as_f64()
                .map_or(d.areal_kg_m2, |g| g / 1000.0),
            tensile_n_m: f["tensile_N_m"].as_f64().unwrap_or(d.tensile_n_m),
            bending_nm: f["bending_Nm"].as_f64().unwrap_or(d.bending_nm),
        }
    }

    /// The membrane: equal warp and weft stiffness, no Poisson coupling, bias shear 0.2·E (the
    /// reference's shear springs), compression softened to `compression` of tension (cloth buckles).
    #[must_use]
    pub fn membrane(&self, compression: f64) -> Membrane {
        let e = self.tensile_n_m;
        Membrane::orthotropic(e, e, 0.0, 0.2 * e, compression)
    }
}

/// Body frames, landmarks and section radii in the form the wrap needs.
pub trait PartFrames {
    /// The frame of `part` (`torso`, `arm_R`, `leg_R`).
    fn frame(&self, part: &str) -> Option<PartFrame>;
    /// A landmark's height coordinate in a part frame (mm).
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64>;
    /// Largest distance from the part axis to the body near `h` (mm).
    fn section_radius(&self, part: &str, h: f64) -> f64;
}

impl PartFrames for BodyFrames {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        BodyFrames::frame(self, part).map(|f| PartFrame {
            origin: f.origin,
            up: f.up,
            front: f.front,
            lat: f.lat,
        })
    }
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        BodyFrames::landmark_h(self, part, name)
    }
    fn section_radius(&self, part: &str, h: f64) -> f64 {
        BodyFrames::section_radius(self, part, h)
    }
}

fn bad(m: String) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, m)
}

/// The wrap placement of each simulated instance (spec 04 §3.1–3.4) from its piece's `placement` and
/// the body's frames: anchor height `landmark_h − y0`, radius `radius_mm` or the largest section radius
/// over nine heights of the piece plus `clearance_mm` (25).
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for a missing or malformed placement, an unknown part or landmark.
pub fn placements(
    pattern: &Pattern,
    mesh: &GarmentMesh,
    frames: &dyn PartFrames,
) -> Result<Vec<WrapPlacement>, Refusal> {
    let mut out = Vec::new();
    for (k, id) in mesh.instances.iter().enumerate() {
        let inst = pattern
            .instance(id)
            .ok_or_else(|| bad(format!("no instance {id}")))?;
        let piece = &pattern.raw()["pieces"][inst.piece_index];
        let pl = &piece["placement"];
        let num = |key: &str, default: f64| pl.get(key).and_then(Value::as_f64).unwrap_or(default);
        let (mut x0, mut y0) = (num("x0", 0.0), num("y0", 0.0));
        if let Some(sc) = piece.get("sim_scale").filter(|v| truthy(v)) {
            x0 *= sc[0].as_f64().unwrap_or(1.0);
            y0 *= sc[1].as_f64().unwrap_or(1.0);
        }
        let part = pl["part"]
            .as_str()
            .ok_or_else(|| bad(format!("{id}: placement has no part")))?;
        let key = if part == "torso" {
            "torso".to_string()
        } else {
            format!("{part}_R")
        };
        let frame = frames
            .frame(&key)
            .ok_or_else(|| bad(format!("{id}: unknown part {part}")))?;
        let landmark = pl
            .get("y0_landmark")
            .and_then(Value::as_str)
            .unwrap_or("waist");
        let h0 = frames
            .landmark_h(&key, landmark)
            .ok_or_else(|| bad(format!("{id}: no landmark {landmark} on {key}")))?
            - y0;
        let radius = if let Some(r) = pl.get("radius_mm").and_then(Value::as_f64) {
            r
        } else {
            let (lo, hi) = mesh.inst_range[k];
            let ys = mesh.verts2d[lo..hi].iter().map(|p| h0 + p.y);
            let (hmin, hmax) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| {
                (a.min(h), b.max(h))
            });
            linspace(hmin, hmax, 9)
                .into_iter()
                .map(|h| frames.section_radius(&key, h))
                .fold(0.0, f64::max)
                + num("clearance_mm", 25.0)
        };
        out.push(WrapPlacement {
            frame,
            h0,
            radius,
            theta_deg: num("theta_deg", 0.0),
            x0,
            mirrored: inst.mirrored,
            own_side: matches!(inst.side.as_str(), "R" | "L") && part == "leg",
        });
    }
    Ok(out)
}

/// Wrapped start positions (m) of every mesh vertex.
#[must_use]
pub fn wrapped_positions(mesh: &GarmentMesh, placements: &[WrapPlacement]) -> Vec<Vec3> {
    let mut x = vec![Vec3::ZERO; mesh.verts2d.len()];
    for (k, p) in placements.iter().enumerate() {
        let (lo, hi) = mesh.inst_range[k];
        x[lo..hi].copy_from_slice(&wrap_points(&mesh.verts2d[lo..hi], p));
    }
    x
}

/// Each instance's rest coordinates in metres with its grainline along +u (a rotation; `sim_scale`
/// and mirroring are already in the mesh, so the grain is scaled and mirrored the same way).
fn grain_rest(pattern: &Pattern, mesh: &GarmentMesh) -> Vec<Vec2> {
    let mut rest = vec![Vec2::ZERO; mesh.verts2d.len()];
    for (k, id) in mesh.instances.iter().enumerate() {
        let Some(inst) = pattern.instance(id) else {
            continue;
        };
        let piece = &pattern.raw()["pieces"][inst.piece_index];
        let g = &piece["grain"];
        let mut d = Vec2::new(
            g[1][0].as_f64().unwrap_or(0.0) - g[0][0].as_f64().unwrap_or(0.0),
            g[1][1].as_f64().unwrap_or(1.0) - g[0][1].as_f64().unwrap_or(0.0),
        );
        if let Some(sc) = piece.get("sim_scale").filter(|v| truthy(v)) {
            d = Vec2::new(
                d.x * sc[0].as_f64().unwrap_or(1.0),
                d.y * sc[1].as_f64().unwrap_or(1.0),
            );
        }
        if inst.mirrored {
            d.x = -d.x;
        }
        let (s, c) = if d.norm() > 0.0 {
            det::sin_cos(-det::atan2(d.y, d.x))
        } else {
            (0.0, 1.0)
        };
        let (lo, hi) = mesh.inst_range[k];
        for i in lo..hi {
            let p = mesh.verts2d[i] * 1e-3;
            rest[i] = Vec2::new(c * p.x - s * p.y, s * p.x + c * p.y);
        }
    }
    rest
}

/// A garment ready to drape: the mesh, its start state, the weld map and the solver.
pub struct Scene {
    /// The garment mesh.
    pub mesh: GarmentMesh,
    /// The kinematic start state.
    pub start: StartState,
    /// Welded index of each mesh vertex.
    pub weld: Vec<usize>,
    /// Triangles dropped because welding collapsed them.
    pub collapsed: usize,
    /// Start crossings untangled, per round.
    pub untangled: Vec<usize>,
    /// The cloth offset the contact ramps up to (m).
    pub target_thickness: f64,
    /// The solver (positions are the welded vertices).
    pub solver: Solver,
}

impl Scene {
    /// Positions of every mesh vertex (m).
    #[must_use]
    pub fn positions(&self) -> Vec<Vec3> {
        self.weld.iter().map(|&w| self.solver.x[w]).collect()
    }
}

/// Builds the scene: mesh → placements → start state against `skin` → weld → cloth model → solver with
/// contact over the skin (no gravity yet).
///
/// # Errors
/// Any refusal from placement, the start state, the cloth model or the contact set-up
/// (`intersecting_start` when the relaxed start still touches the body).
#[allow(clippy::too_many_arguments)]
pub fn build_scene(
    pattern: &Pattern,
    mesh: GarmentMesh,
    frames: &dyn PartFrames,
    skin_vertices: &[Vec3],
    skin_triangles: &[[usize; 3]],
    fabric: Fabric,
    compression: f64,
    contact: ContactParams,
) -> Result<Scene, Refusal> {
    let places = placements(pattern, &mesh, frames)?;
    let wrapped = wrapped_positions(&mesh, &places);
    let tree = NearestTree::new(skin_vertices.to_vec(), skin_triangles.to_vec())?;
    let start = start_state(&mesh, wrapped, &tree);
    // Weld each sewing group into one vertex.
    let mut weld = vec![usize::MAX; mesh.verts2d.len()];
    let mut next = 0;
    for g in &start.groups {
        for &v in g {
            weld[v] = next;
        }
        next += 1;
    }
    for w in &mut weld {
        if *w == usize::MAX {
            *w = next;
            next += 1;
        }
    }
    let mut x = vec![Vec3::ZERO; next];
    for (v, &w) in weld.iter().enumerate() {
        x[w] = start.relaxed[v];
    }
    let rest = grain_rest(pattern, &mesh);
    let (mut tris, mut tri_rest, mut collapsed) = (Vec::new(), Vec::new(), 0);
    for f in &mesh.faces {
        let t = f.map(|v| weld[v]);
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            collapsed += 1;
            continue;
        }
        tris.push(t);
        tri_rest.push(f.map(|v| rest[v]));
    }
    let model = ClothModel::from_triangles(
        next,
        &tris,
        &tri_rest,
        fabric.membrane(compression),
        fabric.bending_nm,
        fabric.areal_kg_m2,
    )?;
    let body = Body::new(skin_vertices.to_vec(), skin_triangles.to_vec(), 5e-3)?;
    let untangled = untangle(&mut x, &tris, &body, &tree)?;
    // The relaxed start keeps every vertex 4.8 mm out, but an edge between two of them can pass close to
    // a sharp body feature (a thumb by a sleeve hem). Start the barrier at half the smallest distance
    // when that is less than the cloth offset; `run_phase` raises it back as the cloth moves out.
    let probe = Contact::new(
        body.clone(),
        ContactParams {
            thickness: 0.0,
            ..contact
        },
        tris.clone(),
        |_, _| 1e-3,
    );
    let d_min = probe
        .min_gap(&x, contact.thickness + contact.dhat)
        .unwrap_or(f64::INFINITY);
    let start_contact = if d_min > 0.0 {
        ContactParams {
            thickness: contact.thickness.min(0.5 * d_min),
            ..contact
        }
    } else {
        contact // refused below, with the location
    };
    let solver = Solver::new(model, x, &vec![[false; 3]; next], Vec3::ZERO)
        .with_contact(body, start_contact)?;
    Ok(Scene {
        mesh,
        start,
        weld,
        collapsed,
        untangled,
        target_thickness: contact.thickness,
        solver,
    })
}

/// Clearance the untangling moves cloth to above a crossed body triangle or ridge (m).
pub const UNTANGLE_CLEARANCE: f64 = 2e-3;

/// Removes the start's remaining crossings found by the exact audit (an edge of either mesh through a
/// triangle of the other), one at a time: a cloth edge through a body triangle has both ends moved
/// [`UNTANGLE_CLEARANCE`] above that triangle's plane; a body edge through a cloth triangle has the
/// triangle moved that far above the ridge along the body's normal there. Returns the crossings it
/// removed per round.
///
/// # Errors
/// [`RefusalCode::IntersectingStart`] for a piece wholly inside the body, or crossings left after 50
/// rounds.
fn untangle(
    x: &mut [Vec3],
    tris: &[[usize; 3]],
    body: &Body,
    tree: &NearestTree,
) -> Result<Vec<usize>, Refusal> {
    let audit = Contact::new(
        body.clone(),
        ContactParams {
            dhat: 1e-3,
            thickness: 0.0,
            kappa: 0.0,
            mu: 0.0,
            eps_v: 1e-3,
        },
        tris.to_vec(),
        |_, _| 1e-3,
    );
    let lift = |p: &mut Vec3, origin: Vec3, n: Vec3| {
        let s = (*p - origin).dot(n);
        if s < UNTANGLE_CLEARANCE {
            *p += n * (UNTANGLE_CLEARANCE - s);
        }
    };
    let mut counts = Vec::new();
    for _ in 0..50 {
        let found = audit.crossings(x, usize::MAX);
        if found.is_empty() {
            if let Some(Intersection::Inside { cv }) = audit.first_intersection(x) {
                return Err(Refusal::new(
                    RefusalCode::IntersectingStart,
                    format!("a garment piece starts inside the body (vertex {cv})"),
                ));
            }
            return Ok(counts);
        }
        counts.push(found.len());
        for i in found {
            match i {
                Intersection::ClothEdge { ce, bt } => {
                    let t = body.triangles[bt].map(|v| body.vertices[v]);
                    let n = (t[1] - t[0]).cross(t[2] - t[0]).normalized();
                    for v in audit.cloth.edges[ce] {
                        lift(&mut x[v], t[0], n);
                    }
                }
                Intersection::BodyEdge { ct, be } => {
                    let [a, b] = body.edges[be].map(|v| body.vertices[v]);
                    let mid = (a + b) * 0.5;
                    let n = tree.nearest(mid).normal;
                    for v in tris[ct] {
                        lift(&mut x[v], mid, n);
                    }
                }
                Intersection::Inside { .. } => {}
            }
        }
    }
    Err(Refusal::new(
        RefusalCode::IntersectingStart,
        format!(
            "the start still crosses the body after 50 untangling rounds (crossings per round {counts:?})"
        ),
    ))
}

/// How much the cloth offset may grow per step while it is below its target (m).
pub const THICKNESS_RAMP: f64 = 0.25e-3;

/// Raises the contact offset toward its target by at most [`THICKNESS_RAMP`], unless that would leave a
/// gap below a tenth of ĝ (then it waits for the barrier to push the cloth out).
fn ramp_thickness(scene: &mut Scene) {
    let x = &scene.solver.x;
    if let Some(c) = scene.solver.contact.as_mut()
        && c.params.thickness < scene.target_thickness
    {
        let old = c.params.thickness;
        c.params.thickness = (old + THICKNESS_RAMP).min(scene.target_thickness);
        if c.min_gap(x, c.params.dhat)
            .is_some_and(|g| g <= 0.1 * c.params.dhat)
        {
            c.params.thickness = old;
        }
    }
}

/// One phase's per-step reports.
#[derive(Clone, Debug, Default)]
pub struct PhaseReport {
    /// Steps taken.
    pub steps: usize,
    /// Newton iterations, summed.
    pub iterations: usize,
    /// Steps that did not converge.
    pub unconverged: usize,
    /// Largest vertex speed at the end (m/s).
    pub max_speed: f64,
    /// The cloth offset at the end (m).
    pub thickness: f64,
}

/// Runs `steps` implicit steps of `h` seconds under `gravity` (m/s², along −z), calling `frame` after
/// each with the step index and the mesh-vertex positions.
///
/// # Errors
/// A refusal from the solver (a non-finite state).
pub fn run_phase(
    scene: &mut Scene,
    steps: usize,
    h: f64,
    gravity: f64,
    mut frame: impl FnMut(usize, &[Vec3]),
) -> Result<PhaseReport, Refusal> {
    scene.solver.gravity = Vec3::new(0.0, 0.0, -gravity);
    let mut rep = PhaseReport::default();
    for k in 0..steps {
        ramp_thickness(scene);
        let r: SolveReport = scene.solver.step(h)?;
        rep.steps += 1;
        rep.iterations += r.iterations;
        rep.unconverged += usize::from(!r.converged);
        frame(k, &scene.positions());
    }
    rep.max_speed = scene.solver.v.iter().map(|v| v.norm()).fold(0.0, f64::max);
    rep.thickness = scene
        .solver
        .contact
        .as_ref()
        .map_or(0.0, |c| c.params.thickness);
    Ok(rep)
}

/// Mesh-vertex index ranges per instance id (for reports).
#[must_use]
pub fn instance_ranges(mesh: &GarmentMesh) -> BTreeMap<String, (usize, usize)> {
    mesh.instances
        .iter()
        .cloned()
        .zip(mesh.inst_range.iter().copied())
        .collect()
}

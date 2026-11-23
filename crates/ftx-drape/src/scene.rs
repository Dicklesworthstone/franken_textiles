//! The garment scene (plan §11.3–11.4): a pattern meshed, placed around a body, pre-sewn and relaxed,
//! its sewing groups welded into single vertices, and handed to the contact cloth solver over the body's
//! skin; then the reference's schedule — sew at zero gravity, drape under gravity.
//!
//! Rest coordinates are each piece's flat 2D mesh in metres, rotated so the piece's grainline is +u (the
//! membrane's warp). Welding keeps every triangle's own rest coordinates (`ClothModel::from_triangles`),
//! so a seam joining sides of different lengths (ease, gathers) is strained, not rewritten.

use std::collections::{BTreeMap, BTreeSet};

use ftx_body::BodyFrames;
use ftx_core::np::linspace;
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det};
use ftx_pattern::{Pattern, Side, instance::physical_side, material_elastic::ElasticBand, truthy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cloth::{ClothModel, SheetMaterial, SolveReport, Solver};
use crate::contact::{Body, Contact, ContactParams, Intersection};
use crate::elements::ElasticSegment;
use crate::garment::{ConstructionField, ConstructionMechanicsOptions, GarmentMesh};
use crate::nearest::NearestTree;
use crate::start::{
    GATHER_COMPRESSION, MM, PartFrame, SelfRepulsionOptions, StartState, WrapPlacement,
    start_state, wrap_points,
};
use crate::stepping::{AcceptedSubstep, SubdivisionOptions};

pub use crate::material::Fabric;

/// Body frames, landmarks and section radii in the form the wrap needs.
pub trait PartFrames {
    /// The frame of `part` (`torso`, `arm_R`, `leg_R`, or explicitly selected `arm_L`/`leg_L`).
    fn frame(&self, part: &str) -> Option<PartFrame>;
    /// A landmark's height coordinate in a part frame (mm).
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64>;
    /// Largest distance from the part axis to the body near `h` (mm).
    fn section_radius(&self, part: &str, h: f64) -> f64;
    /// True for a body whose left and right limbs are posed independently (one arm raised, the other
    /// down). A generated mirror pair's mirrored instance then wraps the left limb itself, not the
    /// reflection of the right limb's wrap, which is the reference convention and assumes a symmetric
    /// pose. The two agree exactly when the left frame is the right frame's reflection.
    fn independent_limbs(&self) -> bool {
        false
    }
}

/// A frame reflected through the sagittal plane `x = 0`, `lat` negated so it stays right-handed: the
/// left limb's frame written in the right limb's convention, which a mirrored instance's wrap uses.
fn reflected(f: PartFrame) -> PartFrame {
    let r = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    PartFrame {
        origin: r(f.origin),
        up: r(f.up),
        front: r(f.front),
        lat: -r(f.lat),
    }
}

// Physical panels select their actual posed limb. Generated mirror pairs still select the right
// frame in placements() and reflect their completed wrap according to the reference convention.
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

fn placement_vector(value: &Value, key: &str, id: &str) -> Result<Vec3, Refusal> {
    let components = value
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| bad(format!("{id}: placement {key} must be [front, lat, up]")))?;
    let mut vector = [0.; 3];
    for (output, input) in vector.iter_mut().zip(components) {
        *output = input
            .as_f64()
            .ok_or_else(|| bad(format!("{id}: placement {key} components must be numbers")))?;
        if !output.is_finite() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("{id}: placement {key} components must be finite"),
            ));
        }
    }
    Ok(vector.into())
}

/// Rigid authored placement is about the selected landmark's centerline, not the world origin or
/// the cloth's anchor surface. Translation uses the original part axes, after the rotation.
fn adjusted_frame(
    frame: PartFrame,
    landmark_h: f64,
    placement: &Value,
    id: &str,
) -> Result<PartFrame, Refusal> {
    let offset = placement
        .get("frame_offset_mm")
        .map(|value| placement_vector(value, "frame_offset_mm", id))
        .transpose()?
        .unwrap_or(Vec3::ZERO);
    let rotation = placement
        .get("frame_rotation")
        .map(|value| {
            let fields = value.as_object().ok_or_else(|| {
                bad(format!(
                    "{id}: placement frame_rotation must contain axis and angle_deg"
                ))
            })?;
            if fields.len() != 2 || !fields.contains_key("axis") || !fields.contains_key("angle_deg")
            {
                return Err(bad(format!(
                    "{id}: placement frame_rotation must contain only axis and angle_deg"
                )));
            }
            let axis = placement_vector(&fields["axis"], "frame_rotation.axis", id)?;
            // Scaling before normalization admits arbitrary finite direction magnitudes without
            // overflow or underflow in the squared norm; the zero direction remains a refusal.
            let scale = axis.x.abs().max(axis.y.abs()).max(axis.z.abs());
            if scale == 0. {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{id}: placement frame_rotation.axis must be nonzero"),
                ));
            }
            let scaled = axis / scale;
            let axis = scaled / scaled.norm();
            let angle = fields["angle_deg"].as_f64().ok_or_else(|| {
                bad(format!(
                    "{id}: placement frame_rotation.angle_deg must be a number"
                ))
            })?;
            if !angle.is_finite() || !(-180. ..=180.).contains(&angle) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "{id}: placement frame_rotation.angle_deg must be within -180 to 180 degrees"
                    ),
                ));
            }
            Ok((axis, angle))
        })
        .transpose()?;
    if offset == Vec3::ZERO && rotation.is_none_or(|(_, angle)| angle == 0.) {
        // Preserve the reference's exact arithmetic, including signed zeros, for absent and
        // explicit identity transforms. Malformed declarations above never become defaults.
        return Ok(frame);
    }
    let axes = [frame.front, frame.lat, frame.up];
    if !frame.origin.is_finite()
        || !landmark_h.is_finite()
        || axes
            .iter()
            .any(|axis| !axis.is_finite() || (axis.norm2() - 1.).abs() > 1e-10)
        || frame.front.dot(frame.lat).abs() > 1e-10
        || frame.front.dot(frame.up).abs() > 1e-10
        || frame.lat.dot(frame.up).abs() > 1e-10
        || (frame.up.cross(frame.front) - frame.lat).norm() > 1e-10
    {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            format!(
                "{id}: a rigid placement requires a finite, right-handed orthonormal body frame"
            ),
        ));
    }
    let local_vector = |v: Vec3| frame.front * v.x + frame.lat * v.y + frame.up * v.z;
    let mut result = frame;
    if let Some((axis, angle)) = rotation.filter(|(_, angle)| *angle != 0.) {
        let axis = local_vector(axis).normalized();
        let rotate = ftx_core::Mat3::rotation(axis, angle.to_radians());
        result.front = rotate.mul_vec(frame.front);
        result.lat = rotate.mul_vec(frame.lat);
        result.up = rotate.mul_vec(frame.up);
        // origin + up*h is the pivot. This equivalent form avoids subtracting two translated
        // positions and keeps a small adjustment accurate on a distant part-frame origin.
        result.origin = frame.origin + (frame.up - result.up) * landmark_h;
    }
    if offset != Vec3::ZERO {
        result.origin += local_vector(offset);
    }
    if !result.origin.is_finite()
        || !result.front.is_finite()
        || !result.lat.is_finite()
        || !result.up.is_finite()
    {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("{id}: placement frame adjustment is not representable"),
        )
        .repair("Use frame offsets and rotations representable at the selected landmark."));
    }
    Ok(result)
}

/// The wrap placement of each simulated instance (spec 04 §3.1–3.4) from its piece's `placement` and
/// the body's frames: anchor height `landmark_h − y0`, radius `radius_mm` or the largest section radius
/// over nine heights of the piece plus `clearance_mm` (25). Optional local rigid frame adjustments
/// move that same wrap and roll without changing the flat rest geometry or inferred radius.
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
        let num = |key: &str| {
            pl.get(key)
                .or_else(|| key.strip_suffix("_mm").and_then(|alias| pl.get(alias)))
                .map(|v| {
                    v.as_f64().ok_or_else(|| {
                        bad(format!("{id}: placement {key} must be a number"))
                            .repair(format!("Supply a numeric {key} or omit the field."))
                    })
                })
                .transpose()
        };
        let (mut x0, mut y0) = (num("x0")?.unwrap_or(0.0), num("y0")?.unwrap_or(0.0));
        if let Some(sc) = piece.get("sim_scale").filter(|v| truthy(v)) {
            let pair = sc
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| bad(format!("{id}: sim_scale must be [sx, sy]")))?;
            let (sx, sy) = (
                pair[0]
                    .as_f64()
                    .ok_or_else(|| bad(format!("{id}: sim_scale[0] must be a number")))?,
                pair[1]
                    .as_f64()
                    .ok_or_else(|| bad(format!("{id}: sim_scale[1] must be a number")))?,
            );
            if !(sx.is_finite() && sx > 0.0 && sy.is_finite() && sy > 0.0) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{id}: sim_scale [{sx}, {sy}] must be finite and positive"),
                ));
            }
            x0 *= sx;
            y0 *= sy;
        }
        let part = pl["part"]
            .as_str()
            .ok_or_else(|| bad(format!("{id}: placement has no part")))?;
        let physical = physical_side(piece).map_err(|e| bad(format!("{id}: {}", e.message())))?;
        // On independently posed limbs a mirrored limb instance wraps the left limb: its frame is written
        // in the right-hand convention the wrap and the placement adjustments use, and the completed wrap
        // is reflected back as for every mirrored instance (see `PartFrames::independent_limbs`).
        let own_limb =
            part != "torso" && inst.mirrored && physical.is_none() && frames.independent_limbs();
        let key = if part == "torso" {
            "torso".to_string()
        } else if own_limb {
            format!("{part}_{}", Side::L.as_str())
        } else {
            format!("{part}_{}", physical.unwrap_or(Side::R).as_str())
        };
        let frame = frames
            .frame(&key)
            .ok_or_else(|| bad(format!("{id}: unknown part {part}")))?;
        let frame = if own_limb { reflected(frame) } else { frame };
        let landmark = pl
            .get("y0_landmark")
            .and_then(Value::as_str)
            .unwrap_or("waist");
        let landmark_h = frames
            .landmark_h(&key, landmark)
            .ok_or_else(|| bad(format!("{id}: no landmark {landmark} on {key}")))?;
        let h0 = landmark_h - y0;
        let radius = if let Some(r) = num("radius_mm")? {
            r
        } else {
            let (lo, hi) = mesh.inst_range[k];
            let ys = mesh.verts2d[lo..hi].iter().map(|p| h0 + p.y);
            let (hmin, hmax) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| {
                (a.min(h), b.max(h))
            });
            let clearance = num("clearance_mm")?.unwrap_or(25.0);
            linspace(hmin, hmax, 9)
                .into_iter()
                .map(|h| frames.section_radius(&key, h))
                .fold(0.0, f64::max)
                + clearance
        };
        if !radius.is_finite() || radius <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{id}: placement wrap radius must be finite and positive (got {radius} mm)"
                ),
            )
            .repair(
                "Supply a positive radius_mm or choose clearance_mm that gives a positive radius.",
            ));
        }
        out.push(WrapPlacement {
            frame: adjusted_frame(frame, landmark_h, pl, id)?,
            h0,
            radius,
            theta_deg: num("theta_deg")?.unwrap_or(0.0),
            x0,
            mirrored: inst.mirrored,
            own_side: (physical.is_some() || matches!(inst.side, Side::R | Side::L))
                && part == "leg",
            own_side_left: physical == Some(Side::L) && part == "leg",
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
fn grain_rest(pattern: &Pattern, mesh: &GarmentMesh, required: bool) -> Result<Vec<Vec2>, Refusal> {
    let mut rest = vec![Vec2::ZERO; mesh.verts2d.len()];
    for (k, id) in mesh.instances.iter().enumerate() {
        let inst = pattern
            .instance(id)
            .ok_or_else(|| bad(format!("mesh refers to missing pattern instance {id}")))?;
        let piece = &pattern.raw()["pieces"][inst.piece_index];
        let mut d = crate::material::grain_direction(piece, required)?;
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
    Ok(rest)
}

/// A garment ready to drape: the mesh, its start state, the weld map and the solver.
pub struct Scene {
    /// The garment mesh.
    pub mesh: GarmentMesh,
    /// Each instance's wrap placement.
    pub placements: Vec<WrapPlacement>,
    /// The kinematic start state.
    pub start: StartState,
    /// Welded index of each mesh vertex.
    pub weld: Vec<usize>,
    /// Triangles dropped because welding collapsed them.
    pub collapsed: usize,
    /// Source instance of each retained solver triangle (indexes `mesh.instances`), after collapse.
    pub triangle_owner: Vec<usize>,
    /// Start crossings untangled, per round.
    pub untangled: Vec<usize>,
    /// Rejected final relaxation count when an earlier contact-clear start was selected.
    /// `start.relax_iterations` and `start.block_p95` describe the accepted checkpoint.
    pub start_backtracked_from: Option<usize>,
    /// Own-side leg start repair, as moved welded vertices and maximum displacement in mm.
    pub start_leg_separation: Option<(usize, f64)>,
    /// The cloth offset the contact ramps up to (m).
    pub target_thickness: f64,
    /// The solver (positions are the welded vertices).
    pub solver: Solver,
    /// The casing elastic's rings (fold, seam line) as ranges of the model's `elastics`; empty without one.
    pub elastic_rings: Vec<std::ops::Range<usize>>,
    /// Independent casing ownership and load domains; ring ranges index `elastic_rings`.
    pub elastic_groups: Vec<ElasticGroup>,
    /// Classified folded-hem and seam-allowance construction field when opt-in construction
    /// mechanics (FTX-27d) were applied; `None` for default single-ply scenes.
    pub construction_field: Option<ConstructionField>,
    /// Number of solver hinges with non-zero authored rest dihedral angles `θ₀` from pattern
    /// `crease_angle_deg` annotations (`0` when no creases are declared).
    pub authored_crease_hinges: usize,
}

#[derive(Clone, Debug)]
/// Independent ring ranges and the physical mass domain used only for the casing's grip estimate.
pub struct ElasticGroup {
    /// Resolved source instance and relaxed length.
    pub casing: ElasticCasing,
    /// This casing's consecutive ranges in `Scene::elastic_rings`.
    pub rings: std::ops::Range<usize>,
    /// Unique welded vertices touched by the load domain, in fixed solver order. A seam vertex can
    /// also carry other instances' mass; summing its entire mass is NOT this domain's weight.
    pub loaded_vertices: Vec<usize>,
    /// Retained source-triangle mass in the declared load domain, kg, excluding accessory shares.
    pub loaded_cloth_mass_kg: f64,
    /// Accessory mass allocated to declared source instances, kg, even at shared seam vertices.
    pub loaded_attached_mass_kg: f64,
    /// Sum of this domain's actual nodal mass contributions, kg. Welding does not expand the scope.
    pub loaded_mass_kg: f64,
}

impl Scene {
    /// Positions of every mesh vertex (m).
    #[must_use]
    pub fn positions(&self) -> Vec<Vec3> {
        self.weld.iter().map(|&w| self.solver.x[w]).collect()
    }
}

/// Axial stiffness of a casing elastic (N): a soft knit elastic, 10 % stretch under 1.5 N. Chosen, not
/// measured; KAT 7's ladder separates gripping from slack elastics with it.
pub const ELASTIC_EA: f64 = 15.0;

/// An elastic in a casing (plan §11.2 "Elastic"): the casing instance, the relaxed elastic length (m) and
/// its axial stiffness EA (N).
#[derive(Clone, Debug, PartialEq)]
pub struct ElasticCasing {
    /// The casing instance (its fold, the `top` edge, and its seam line carry the elastic).
    pub instance: String,
    /// Relaxed elastic length (m).
    pub length: f64,
    /// Axial stiffness EA (N).
    pub ea: f64,
    /// Independent finished-band evidence, when selected; absent retains caller-specified EA.
    pub material: Option<ElasticBand>,
    /// Physical instances whose simulated inertia is compared with this casing's ideal-ring grip.
    /// This comparison does not infer load transfer between independent garments or casings.
    pub load_instances: Vec<String>,
}

/// Compatibility helper returning exactly one resolved casing, or None on absence/refusal.
/// Production consumers must use [`elastic_casings`] to retain structured evidence refusals.
#[must_use]
pub fn elastic_casing(pattern: &Pattern, ea: f64) -> Option<ElasticCasing> {
    let mut casings = elastic_casings(pattern, ea).ok()?;
    (casings.len() == 1).then(|| casings.remove(0))
}

// Only the historical geometry convention; never resolves or defaults an explicitly selected band.
fn legacy_casing(pattern: &Pattern, ea: f64) -> Option<ElasticCasing> {
    let raw = pattern.raw();
    let cut = raw["meta"]["solved"]["elastic_cut_mm"].as_f64()?;
    let overlap = raw["meta"]["style"]["elastic_overlap"]
        .as_f64()
        .unwrap_or(0.0);
    let length = (cut - overlap) * MM;
    if !(length > 0.0 && ea > 0.0 && ea.is_finite()) {
        return None;
    }
    let piece = raw["pieces"]
        .as_array()?
        .iter()
        .find(|p| p["role"] == "waistband")?;
    let mut casings = pattern
        .instances
        .iter()
        .filter(|i| piece["id"] == i.piece.as_str());
    let instance = casings.next()?.id.clone();
    if casings.next().is_some() {
        return None;
    }
    Some(ElasticCasing {
        instance,
        length,
        ea,
        material: None,
        load_instances: pattern.instances.iter().map(|i| i.id.clone()).collect(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredCasing {
    instance: String,
    relaxed_m: f64,
    load_instances: Vec<String>,
    #[serde(default)]
    elastic_material: Option<ftx_pattern::material_catalog::Selection>,
}

fn band(raw: &Value) -> Result<ElasticBand, Refusal> {
    let selection = serde_json::from_value(raw.clone())
        .map_err(|e| bad(format!("elastic_material selection: {e}")))?;
    ElasticBand::resolve(&ftx_pattern::textile_import::catalog()?, &selection)
}

/// Resolve source-preserving, explicitly scoped casings or the original single-waistband convention.
/// A selected finished band supplies its own EA; positive caller EA is not a multiplier or override.
/// Explicit declarations are validated even when caller EA is zero, which disables all casing rings.
///
/// # Errors
/// Ambiguous legacy/explicit declarations, duplicate or inactive casings, invalid lengths or load scopes.
pub fn elastic_casings(pattern: &Pattern, ea: f64) -> Result<Vec<ElasticCasing>, Refusal> {
    let solved = &pattern.raw()["meta"]["solved"];
    let Some(raw) = solved.get("elastic_casings") else {
        let Some(selected) = solved.get("elastic_material") else {
            return Ok(legacy_casing(pattern, ea).into_iter().collect());
        };
        Refusal::finite("elastic stiffness switch", ea)?;
        if ea < 0.0 {
            return Err(bad("elastic stiffness switch must be nonnegative".into()));
        }
        let material = band(selected)?;
        let mut casing = legacy_casing(pattern, material.axial_stiffness_n()).ok_or_else(|| {
            bad("selected elastic band needs a valid single legacy casing".into())
        })?;
        let instance = pattern
            .instance(&casing.instance)
            .expect("legacy casing instance");
        let piece = &pattern.raw()["pieces"][instance.piece_index];
        if piece["sim"] == false || !truthy(&piece["placement"]) {
            return Err(bad(
                "selected elastic band needs an active placed casing".into()
            ));
        }
        casing.material = Some(material);
        return Ok(if ea == 0.0 { Vec::new() } else { vec![casing] });
    };
    if solved.get("elastic_cut_mm").is_some() || solved.get("elastic_material").is_some() {
        return Err(bad(
            "explicit casings cannot also declare legacy elastic_cut_mm or elastic_material".into(),
        ));
    }
    let entries = raw
        .as_array()
        .ok_or_else(|| bad("elastic_casings must be a list".into()))?;
    if entries.is_empty() || entries.len() > 64 {
        return Err(bad("elastic_casings needs 1..64 independent casings".into()));
    }
    Refusal::finite("elastic stiffness", ea)?;
    if ea < 0.0 {
        return Err(bad("elastic stiffness must be nonnegative".into()));
    }
    let mut seen = BTreeSet::new();
    let mut casings = Vec::new();
    for entry in entries {
        if entry.get("elastic_material").is_some_and(Value::is_null) {
            return Err(bad("elastic_material must be a selection, not null".into()));
        }
        let entry: DeclaredCasing =
            serde_json::from_value(entry.clone()).map_err(|e| bad(e.to_string()))?;
        let instance = pattern
            .instance(&entry.instance)
            .ok_or_else(|| bad(format!("unknown casing instance {}", entry.instance)))?;
        let piece = &pattern.raw()["pieces"][instance.piece_index];
        if !seen.insert(entry.instance.clone())
            || piece["sim"] == false
            || !truthy(&piece["placement"])
        {
            return Err(bad(
                "casings must name distinct active placed instances".into()
            ));
        }
        if !entry.relaxed_m.is_finite() || entry.relaxed_m <= 0.0 {
            return Err(bad("casing relaxed_m must be finite and positive".into()));
        }
        let mut loads = BTreeSet::new();
        if entry.load_instances.is_empty() || entry.load_instances.len() > pattern.instances.len() {
            return Err(bad(
                "casing needs a bounded nonempty load_instances list".into()
            ));
        }
        for id in &entry.load_instances {
            let target = pattern
                .instance(id)
                .ok_or_else(|| bad("casing load_instances must be known and unique".into()))?;
            if !loads.insert(id) {
                return Err(bad("casing load_instances must be known and unique".into()));
            }
            let piece = &pattern.raw()["pieces"][target.piece_index];
            if piece["sim"] == false || !truthy(&piece["placement"]) {
                return Err(bad(
                    "explicit casing loads must name active placed instances".into(),
                ));
            }
        }
        if !loads.contains(&entry.instance) {
            return Err(bad(
                "casing load_instances must include its own instance".into()
            ));
        }
        // Resolve even when disabled; the global switch cannot authorize stale or missing evidence.
        let material = entry
            .elastic_material
            .as_ref()
            .map(|selection| {
                ElasticBand::resolve(&ftx_pattern::textile_import::catalog()?, selection)
            })
            .transpose()?;
        if ea > 0.0 {
            casings.push(ElasticCasing {
                instance: entry.instance,
                length: entry.relaxed_m,
                ea: material.as_ref().map_or(ea, ElasticBand::axial_stiffness_n),
                material,
                load_instances: entry.load_instances,
            });
        }
    }
    Ok(casings)
}

/// `pattern` with the casing piece simulated at its cut length (its `sim_scale` along the band set to 1):
/// the elastic, not a shrunken casing, draws it to the waist.
#[must_use]
pub fn casing_at_cut_length(pattern: &Pattern, casing: &ElasticCasing) -> Pattern {
    casings_at_cut_length(pattern, std::slice::from_ref(casing))
}

/// Restore every declared casing's cut geometry without changing other simulated piece scales.
#[must_use]
pub fn casings_at_cut_length(pattern: &Pattern, casings: &[ElasticCasing]) -> Pattern {
    let mut raw = pattern.raw().clone();
    for casing in casings {
        let piece = pattern.instance(&casing.instance).map(|i| i.piece_index);
        if let Some(sc) = piece.and_then(|p| raw["pieces"][p].get_mut("sim_scale"))
            && let Some(x) = sc.get_mut(0)
        {
            *x = Value::from(1.0);
        }
    }
    Pattern::new(raw)
}

/// The casing's elastic as two rings of tension-only segments, along its fold (the `top` edge) and along
/// its seam line (every other edge but the `end_*` ones), each with half the stiffness: an elastic in a
/// narrow channel gathers the whole channel. A ring on the fold alone left the seam line near its cut
/// length (on the 92 cm trousers a cone 100 mm wider at the seam, the trousers hanging 12 mm lower). Each
/// ring's rest is its flat length scaled to the elastic's.
fn elastic_rings_of(
    mesh: &GarmentMesh,
    weld: &[usize],
    casing: &ElasticCasing,
) -> Result<Vec<Vec<ElasticSegment>>, Refusal> {
    let edges: Vec<(&str, &Vec<usize>)> = mesh
        .index
        .iter()
        .filter(|((inst, _), _)| *inst == casing.instance)
        .map(|((_, edge), path)| (edge.as_str(), path))
        .collect();
    let fold: Vec<&Vec<usize>> = edges
        .iter()
        .filter(|(e, _)| *e == "top")
        .map(|(_, p)| *p)
        .collect();
    let seam: Vec<&Vec<usize>> = edges
        .iter()
        .filter(|(e, _)| *e != "top" && !e.starts_with("end"))
        .map(|(_, p)| *p)
        .collect();
    [("top edge", fold), ("seam line", seam)]
        .into_iter()
        .map(|(what, paths)| elastic_ring(mesh, weld, casing, what, &paths))
        .collect()
}

fn elastic_ring(
    mesh: &GarmentMesh,
    weld: &[usize],
    casing: &ElasticCasing,
    what: &str,
    paths: &[&Vec<usize>],
) -> Result<Vec<ElasticSegment>, Refusal> {
    let mut segments = Vec::new();
    for path in paths {
        for w in path.windows(2) {
            let (a, b) = (weld[w[0]], weld[w[1]]);
            if a != b {
                segments.push((a, b, (mesh.verts2d[w[1]] - mesh.verts2d[w[0]]).norm() * MM));
            }
        }
    }
    // a closed ring: every welded vertex on exactly two segments
    let mut degree: BTreeMap<usize, usize> = BTreeMap::new();
    for &(a, b, _) in &segments {
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
    }
    if segments.len() < 3 || degree.values().any(|&d| d != 2) {
        return Err(bad(format!(
            "the casing {}'s {what} does not close into a ring (are its ends sewn?)",
            casing.instance
        )));
    }
    let total_length: f64 = segments.iter().map(|s| s.2).sum();
    if !total_length.is_finite() || total_length <= 0.0 {
        return Err(bad(format!(
            "the casing {}'s {what} has zero or non-finite perimeter",
            casing.instance
        )));
    }
    let scale = casing.length / total_length;
    if !scale.is_finite() {
        return Err(bad(format!(
            "the casing {}'s {what} elastic length scale overflows",
            casing.instance
        )));
    }
    Ok(segments
        .into_iter()
        .map(|(a, b, l)| ElasticSegment {
            v: [a, b],
            rest: l * scale,
            ea: casing.ea / 2.0,
        })
        .collect())
}

/// Opt-in scene-assembly mechanics and start-state options (FTX-27d, FTX-27g).
///
/// Default leaves every legacy mesh, start state, cloth mass, and hinge bending bit-identical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SceneBuildOptions {
    /// Opt-in folded-hem and sewn seam-allowance mass and bending multipliers (FTX-27d).
    #[serde(
        default,
        skip_serializing_if = "ConstructionMechanicsOptions::is_default"
    )]
    pub construction: ConstructionMechanicsOptions,
    /// Opt-in spatial-hash cloth–cloth self-repulsion during start-state relaxation (FTX-27g).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_self_repulsion: Option<SelfRepulsionOptions>,
}

impl SceneBuildOptions {
    /// True when all fields equal the legacy no-op default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.construction.is_default() && self.start_self_repulsion.is_none()
    }

    /// Validates all configured scene-build options fail-closed.
    ///
    /// # Errors
    /// As [`ConstructionMechanicsOptions::validate`] and [`SelfRepulsionOptions::validate`].
    pub fn validate(&self) -> Result<(), Refusal> {
        self.construction.validate()?;
        if let Some(repulsion) = self.start_self_repulsion {
            repulsion.validate()?;
        }
        Ok(())
    }
}

/// Builds the scene: mesh → placements → start state against `skin` → weld → cloth model → solver with
/// contact over the skin (no gravity yet).
///
/// # Errors
/// Any refusal from body-surface admission, placement, the start state, the cloth model or contact
/// set-up. Before placement, the body must pass an exact self-intersection audit within 1,000,000
/// overlapping triangle pairs. This admits valid open surfaces but does not certify anatomy.
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
    elastic: Option<&ElasticCasing>,
) -> Result<Scene, Refusal> {
    build_scene_with_options(
        pattern,
        mesh,
        frames,
        skin_vertices,
        skin_triangles,
        fabric,
        compression,
        contact,
        elastic,
        SceneBuildOptions::default(),
    )
}

/// Builds the scene with opt-in construction mechanics and/or start-state self-repulsion.
/// Default [`SceneBuildOptions::default`] is bit-identical to [`build_scene`].
///
/// # Errors
/// As [`build_scene`], plus [`SceneBuildOptions::validate`] and construction-mechanics refusals.
#[allow(clippy::too_many_arguments)]
pub fn build_scene_with_options(
    pattern: &Pattern,
    mesh: GarmentMesh,
    frames: &dyn PartFrames,
    skin_vertices: &[Vec3],
    skin_triangles: &[[usize; 3]],
    fabric: Fabric,
    compression: f64,
    contact: ContactParams,
    elastic: Option<&ElasticCasing>,
    options: SceneBuildOptions,
) -> Result<Scene, Refusal> {
    if pattern.raw()["fabric"].get("material").is_some()
        || pattern.raw()["fabric"].get("material_id").is_some()
        || pattern.raw()["fabric"].get("interfaces").is_some()
        || pattern.raw()["pieces"]
            .as_array()
            .is_some_and(|ps| ps.iter().any(|p| p.get("fabric").is_some()))
    {
        return Err(bad(
            "characterized/per-piece fabrics require the resolved drape pipeline, not a uniform Fabric argument"
                .into(),
        ));
    }
    build_scene_inner(
        pattern,
        mesh,
        frames,
        skin_vertices,
        skin_triangles,
        fabric,
        compression,
        contact,
        elastic.map(std::slice::from_ref).unwrap_or(&[]),
        None,
        None,
        None,
        None,
        None,
        options,
    )
}

/// Reapply the wrap's own-side placement only where every source of a weld belongs to one leg.
fn separated_leg_start(
    scene: &Scene,
    clearance_m: f64,
    spacing_m: f64,
    plain: &[(usize, usize)],
) -> Option<(StartState, usize, f64)> {
    let mut owners = vec![[false; 3]; scene.solver.x.len()];
    let mut legs = [false; 2];
    for (&owner, &weld) in scene.mesh.owner.iter().zip(&scene.weld) {
        let place = &scene.placements[owner];
        let side = if place.own_side {
            let side = usize::from(place.own_side_left || place.mirrored);
            legs[side] = true;
            side
        } else {
            2
        };
        owners[weld][side] = true;
    }
    if legs != [true, true] {
        return None;
    }
    let mut positions = scene.solver.x.clone();
    let (mut moved, mut max_move) = (0, 0.0_f64);
    let mut crossed = false;
    for (p, owner) in positions.iter_mut().zip(owners) {
        let sign = match owner {
            [true, false, false] => -1.0,
            [false, true, false] => 1.0,
            _ => continue, // Includes the seam joining left to right and attachments to the torso.
        };
        let on_side = sign * p.x;
        crossed |= on_side < 0.0;
        let delta = clearance_m * 0.5 - on_side;
        if delta > spacing_m {
            return None;
        }
        if delta > 0.0 {
            p.x = sign * clearance_m * 0.5;
            moved += 1;
            max_move = max_move.max(delta);
        }
    }
    if !crossed {
        return None;
    }
    let mut start = scene.start.clone();
    start.relaxed = scene.weld.iter().map(|&w| positions[w]).collect();
    let strain: Vec<_> = crate::start::edge_strain(&start.relaxed, &scene.mesh.verts2d, plain)
        .into_iter()
        .map(f64::abs)
        .collect();
    *start.block_p95.last_mut()? = if strain.is_empty() {
        0.0
    } else {
        ftx_core::np::percentile(&strain, 95.0)
    };
    Some((start, moved, max_move / MM))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_scene_resolved_with_options(
    pattern: &Pattern,
    mesh: GarmentMesh,
    frames: &dyn PartFrames,
    collision: (&[Vec3], &[[usize; 3]]),
    material: &crate::material::MaterialInputs,
    elastics: &[ElasticCasing],
    mut display_contact: Option<&mut crate::animation::ReplayContact>,
    options: SceneBuildOptions,
) -> Result<Scene, Refusal> {
    options.validate()?;
    let (skin_vertices, skin_triangles) = collision;
    let build = |mesh, start| {
        build_scene_inner(
            pattern,
            mesh,
            frames,
            skin_vertices,
            skin_triangles,
            material.fabric,
            material.options.compression,
            material.options.contact,
            elastics,
            material.piece_fabrics.as_deref(),
            material.response.as_ref(),
            material.piece_responses.as_deref(),
            material.contact_materials.as_ref(),
            start,
            options,
        )
    };
    let mut scene = build(mesh, None)?;
    if let Some(params) = material.options.self_contact {
        let mut admit = |candidate: &Scene| -> Result<(), Refusal> {
            candidate
                .solver
                .contact
                .as_ref()
                .expect("body contact exists")
                .clone()
                .with_self_contact(&candidate.solver.x, params)?;
            if let Some(display) = display_contact.as_deref_mut() {
                display.check(
                    &candidate.positions(),
                    &candidate.mesh.faces,
                    crate::animation::FOLD_FRAMES + crate::animation::ZIP_FRAMES - 1,
                )?;
            }
            Ok(())
        };
        match admit(&scene) {
            Err(e)
                if matches!(
                    e.code,
                    RefusalCode::IntersectingStart | RefusalCode::IntersectingTrajectory
                ) =>
            {
                // Replay at existing push-out boundaries, preserving the original relaxation path.
                // Rebuilding rechecks body clearance, material domains and source-owned contact
                // before any constitutive history is initialized. The displayed body can retain
                // hands or feet omitted by the collider; every candidate must clear both surfaces.
                let original = scene.start.relax_iterations;
                let tree = NearestTree::new(skin_vertices.to_vec(), skin_triangles.to_vec())?;
                let mut replacement = None;
                let plain: Vec<_> = crate::start::face_edges(&scene.mesh.faces)
                    .into_iter()
                    .filter(|&(a, b)| {
                        scene.start.gather_weights[a] >= 1.0 && scene.start.gather_weights[b] >= 1.0
                    })
                    .collect();
                for stride in [
                    crate::start::RELAX_ITERS,
                    crate::start::RELAX_PUSH_OUT_ITERS,
                ] {
                    let mut positions = scene.start.presewn.clone();
                    for iterations in (stride..original).step_by(stride) {
                        positions = match options.start_self_repulsion {
                            Some(repulsion) => crate::start::relax_with_repulsion(
                                &scene.mesh,
                                &positions,
                                &scene.start.groups,
                                &tree,
                                stride,
                                crate::start::RELAX_CLEARANCE,
                                &scene.start.gather_weights,
                                Some(repulsion),
                            )?,
                            None => crate::start::relax(
                                &scene.mesh,
                                &positions,
                                &scene.start.groups,
                                &tree,
                                stride,
                                crate::start::RELAX_CLEARANCE,
                                &scene.start.gather_weights,
                            ),
                        };
                        let complete_block = iterations % crate::start::RELAX_ITERS == 0;
                        let coarse_equivalent = options.start_self_repulsion.is_none_or(|r| {
                            crate::start::RELAX_PUSH_OUT_ITERS.is_multiple_of(r.period)
                        });
                        if stride == crate::start::RELAX_PUSH_OUT_ITERS
                            && complete_block
                            && coarse_equivalent
                        {
                            continue; // Bit-identical to an already refused coarse checkpoint.
                        }
                        let mut start = scene.start.clone();
                        start.relaxed = positions.clone();
                        start.relax_iterations = iterations;
                        start
                            .block_p95
                            .truncate(iterations / crate::start::RELAX_ITERS);
                        if !complete_block {
                            let strain: Vec<_> =
                                crate::start::edge_strain(&positions, &scene.mesh.verts2d, &plain)
                                    .into_iter()
                                    .map(f64::abs)
                                    .collect();
                            start.block_p95.push(if strain.is_empty() {
                                0.0
                            } else {
                                ftx_core::np::percentile(&strain, 95.0)
                            });
                        }
                        if let Ok(mut candidate) = build(scene.mesh.clone(), Some(start))
                            && admit(&candidate).is_ok()
                        {
                            candidate.start_backtracked_from = Some(original);
                            replacement = Some(candidate);
                        }
                    }
                    if replacement.is_some() {
                        break;
                    }
                }
                if let Some(candidate) = replacement {
                    scene = candidate;
                } else if let Some((start, moved, max_mm)) = separated_leg_start(
                    &scene,
                    params.thickness + params.dhat,
                    material.options.spacing * MM,
                    &plain,
                ) && let Ok(mut candidate) = build(scene.mesh.clone(), Some(start))
                    && admit(&candidate).is_ok()
                {
                    candidate.start_leg_separation = Some((moved, max_mm));
                    scene = candidate;
                }
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
    }
    if material.relaxation.is_some()
        || material
            .piece_relaxations
            .as_ref()
            .is_some_and(|v| v.iter().any(Option::is_some))
        || material.membrane_plasticity.is_some()
        || material
            .piece_membrane_plasticities
            .as_ref()
            .is_some_and(|v| v.iter().any(Option::is_some))
    {
        let relaxation = material_triangle_field(
            pattern,
            &scene,
            &material.relaxation,
            material.piece_relaxations.as_deref(),
        )?;
        let plasticity = material_triangle_field(
            pattern,
            &scene,
            &material.membrane_plasticity,
            material.piece_membrane_plasticities.as_deref(),
        )?;
        // The solver checks the actual retained grain/gather mapping. Never overwrite a caller's
        // compression factor or a gathered zone to make a permanent-set law appear compatible.
        scene.solver = scene
            .solver
            .with_membrane_histories(&relaxation, &plasticity)?;
    }
    if material.plasticity.is_some()
        || material
            .piece_plasticities
            .as_ref()
            .is_some_and(|v| v.iter().any(Option::is_some))
    {
        let laws = material_triangle_field(
            pattern,
            &scene,
            &material.plasticity,
            material.piece_plasticities.as_deref(),
        )?;
        scene.solver = scene.solver.with_bending_plasticity(&laws)?;
    }
    Ok(scene)
}

// All material history binds to the SAME retained triangle ownership and rotated rest frame.
// Welded vertices never merge materials; explicit None entries do not inherit a default law.
fn material_triangle_field<T: Clone>(
    pattern: &Pattern,
    scene: &Scene,
    uniform: &Option<T>,
    pieces: Option<&[Option<T>]>,
) -> Result<Vec<Option<T>>, Refusal> {
    scene
        .triangle_owner
        .iter()
        .map(|owner| {
            let id = &scene.mesh.instances[*owner];
            let instance = pattern
                .instance(id)
                .ok_or_else(|| bad(format!("missing instance {id}")))?;
            match pieces {
                Some(field) => field
                    .get(instance.piece_index)
                    .cloned()
                    .ok_or_else(|| bad("missing per-piece material-history selector".into())),
                None => Ok(uniform.clone()),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_scene_inner(
    pattern: &Pattern,
    mesh: GarmentMesh,
    frames: &dyn PartFrames,
    skin_vertices: &[Vec3],
    skin_triangles: &[[usize; 3]],
    fabric: Fabric,
    compression: f64,
    contact: ContactParams,
    elastics: &[ElasticCasing],
    piece_fabrics: Option<&[Option<Fabric>]>,
    response: Option<&ftx_pattern::material_response::CompiledMembrane>,
    piece_responses: Option<&[Option<ftx_pattern::material_response::CompiledMembrane>]>,
    contact_materials: Option<&crate::material::ContactMaterials>,
    start_override: Option<StartState>,
    options: SceneBuildOptions,
) -> Result<Scene, Refusal> {
    options.validate()?;
    fabric.validate()?;
    fabric.membrane(compression).validate()?;
    crate::collider::audit_surface(
        skin_vertices,
        skin_triangles,
        crate::collider::STATIC_SURFACE_PAIR_BUDGET,
    )?;
    let rest = grain_rest(pattern, &mesh, fabric.requires_grain())?;
    let instance_fabrics: Option<Vec<Fabric>> = piece_fabrics
        .map(|materials| {
            mesh.instances
                .iter()
                .map(|id| {
                    let instance = pattern
                        .instance(id)
                        .ok_or_else(|| bad(format!("no instance {id}")))?;
                    materials
                        .get(instance.piece_index)
                        .copied()
                        .flatten()
                        .ok_or_else(|| bad(format!("no active material for instance {id}")))
                })
                .collect::<Result<_, Refusal>>()
        })
        .transpose()?;
    let instance_responses: Option<Vec<Option<ftx_pattern::material_response::CompiledMembrane>>> =
        piece_responses
            .map(|responses| {
                mesh.instances
                    .iter()
                    .map(|id| {
                        let instance = pattern
                            .instance(id)
                            .ok_or_else(|| bad(format!("no instance {id}")))?;
                        responses
                            .get(instance.piece_index)
                            .cloned()
                            .ok_or_else(|| bad(format!("no response slot for instance {id}")))
                    })
                    .collect::<Result<_, Refusal>>()
            })
            .transpose()?;
    let mut casing_instances = BTreeSet::new();
    for casing in elastics {
        if !casing.length.is_finite()
            || casing.length <= 0.0
            || !casing.ea.is_finite()
            || casing.ea <= 0.0
        {
            return Err(bad(
                "active casing needs positive finite relaxed length and stiffness".into(),
            ));
        }
        let loads: BTreeSet<_> = casing.load_instances.iter().collect();
        if loads.len() != casing.load_instances.len()
            || !loads.contains(&casing.instance)
            || loads.iter().any(|id| pattern.instance(id).is_none())
        {
            return Err(bad(
                "casing load scope must contain its target and unique known instances".into(),
            ));
        }
        let owner = mesh
            .instances
            .iter()
            .position(|i| *i == casing.instance)
            .ok_or_else(|| bad(format!("casing {} has no simulated mesh", casing.instance)))?;
        if !casing_instances.insert(owner) {
            return Err(bad("duplicate casing mesh instance".into()));
        }
    }
    let places = placements(pattern, &mesh, frames)?;
    let wrapped = wrapped_positions(&mesh, &places);
    if let Some(v) = wrapped.iter().position(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "{}: placement produces a non-finite wrapped position",
                mesh.instances[mesh.owner[v]]
            ),
        )
        .repair(
            "Check the wrap radius, angle, anchor and body frame against the pattern dimensions.",
        ));
    }
    let tree = NearestTree::new(skin_vertices.to_vec(), skin_triangles.to_vec())?;
    let start = match start_override {
        Some(start) => start,
        None => match options.start_self_repulsion {
            Some(repulsion) => {
                crate::start::start_state_with_repulsion(&mesh, wrapped, &tree, Some(repulsion))?
            }
            None => start_state(&mesh, wrapped, &tree),
        },
    };
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
    let (mut tris, mut tri_rest, mut collapsed) = (Vec::new(), Vec::new(), 0);
    // Gather zones (the reference's vertex-weighted structural stiffness): compression drops from the
    // cloth's factor to GATHER_COMPRESSION at a gathered seam, linearly in the element's mean weight.
    let mut tri_compression = Vec::new();
    let mut tri_materials = Vec::new();
    let mut tri_responses = Vec::new();
    let mut tri_domains = Vec::new();
    let mut triangle_owner = Vec::new();
    let mut source_triangles = Vec::new();
    let mut tri_bending = Vec::new();
    let low = GATHER_COMPRESSION.min(compression);
    // A gathered seam that declares its own band of pleats replaces the weights and the floor (spec 04 §5.4).
    let relief = crate::start::gather_relief(&mesh);
    for f in &mesh.faces {
        let t = f.map(|v| weld[v]);
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            collapsed += 1;
            continue;
        }
        tris.push(t);
        source_triangles.push(*f);
        tri_rest.push(f.map(|v| rest[v]));
        let owner = mesh.owner[f[0]];
        triangle_owner.push(owner);
        let sheet = match &instance_fabrics {
            Some(field) => field
                .get(owner)
                .ok_or_else(|| bad("invalid bending triangle owner".into()))?,
            None => &fabric,
        };
        tri_bending.push(sheet.bending_response());
        tri_domains.push(sheet.strain_domain());
        if let Some(field) = &instance_fabrics {
            if f.iter().any(|&v| mesh.owner[v] != owner) {
                return Err(bad(
                    "a rest triangle cannot mix source-piece identities".into()
                ));
            }
            let m = field
                .get(owner)
                .ok_or_else(|| bad("invalid triangle owner".into()))?;
            tri_materials.push(SheetMaterial {
                membrane: m.membrane(compression),
                areal_kg_m2: m.areal_kg_m2,
                bending_nm: m.bending_nm,
            });
        }
        if let Some(field) = &instance_responses {
            tri_responses.push(
                field
                    .get(owner)
                    .cloned()
                    .ok_or_else(|| bad("invalid nonlinear triangle owner".into()))?,
            );
        } else if let Some(law) = response {
            tri_responses.push(Some(law.clone()));
        }
        if casing_instances.contains(&mesh.owner[f[0]]) {
            // a casing at its cut length, drawn up by its elastic: gathered all through
            tri_compression.push(low);
        } else if let Some((weights, floors)) = &relief {
            let w = f.iter().map(|&v| weights[v]).sum::<f64>() / 3.0;
            let floor = f
                .iter()
                .map(|&v| floors[v])
                .fold(f64::INFINITY, f64::min)
                .min(compression);
            tri_compression.push(floor + w * (compression - floor));
        } else {
            let w = f.iter().map(|&v| start.gather_weights[v]).sum::<f64>() / 3.0;
            tri_compression.push(low + w * (compression - low));
        }
    }
    let mut model = if instance_fabrics.is_some() {
        ClothModel::from_material_triangles(next, &tris, &tri_rest, &tri_materials)?
    } else {
        ClothModel::from_triangles(
            next,
            &tris,
            &tri_rest,
            fabric.membrane(compression),
            fabric.bending_nm,
            fabric.areal_kg_m2,
        )?
    };
    model.set_compression(&tri_compression)?;
    model.set_membrane_domains(&tri_domains)?;
    if tri_bending.iter().any(Option::is_some) {
        model.set_panel_bending(&rest, &source_triangles, &weld, &tri_bending)?;
    }
    if !tri_responses.is_empty() {
        model.set_membrane_responses(&tri_responses)?;
    }
    model.check_membrane_domains(&x)?;
    let mut crease_map: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let mut register_crease = |wa: usize, wb: usize, angle: f64| -> Result<(), Refusal> {
        if wa == wb || angle == 0.0 {
            return Ok(());
        }
        let key = (wa.min(wb), wa.max(wb));
        if let Some(&prev) = crease_map.get(&key) {
            if (prev - angle).abs() > 1e-9 {
                return Err(bad(format!(
                    "conflicting authored crease angles ({prev} vs {angle} rad) on welded edge {key:?}"
                )));
            }
        } else {
            crease_map.insert(key, angle);
        }
        Ok(())
    };
    for c in &mesh.creases {
        register_crease(weld[c.verts[0]], weld[c.verts[1]], c.rest_angle_rad)?;
    }
    for s in &mesh.seams {
        if let Some(angle) = s.crease_angle_rad {
            for w in s.side_a.windows(2) {
                register_crease(weld[w[0]], weld[w[1]], angle)?;
            }
        }
    }
    let mut authored_crease_hinges = 0usize;
    if !crease_map.is_empty() {
        let mut angles = vec![0.0_f64; model.hinges.len()];
        for (i, h) in model.hinges.iter().enumerate() {
            let key = (h.v[0].min(h.v[1]), h.v[0].max(h.v[1]));
            if let Some(&theta) = crease_map.get(&key) {
                angles[i] = theta;
                authored_crease_hinges += 1;
            }
        }
        if authored_crease_hinges > 0 {
            if !model.panel_bending_patches().is_empty() {
                return Err(bad(
                    "authored crease/pleat rest angles cannot be combined with characterized panel bending"
                        .into(),
                ));
            }
            model.set_hinge_rest_angles(&angles)?;
        }
    }
    let construction_field = if options.construction.is_active() {
        let base_areal: Vec<f64> = triangle_owner
            .iter()
            .map(|&owner| {
                instance_fabrics
                    .as_ref()
                    .map_or(fabric.areal_kg_m2, |field| field[owner].areal_kg_m2)
            })
            .collect();
        let field = model.apply_construction_mechanics(
            &rest,
            &source_triangles,
            &triangle_owner,
            &weld,
            &base_areal,
            options.construction,
        )?;
        Some(field)
    } else {
        None
    };
    let tri_mass_mult = construction_field
        .as_ref()
        .map(|field| field.triangle_mass_multiplier.as_slice());
    crate::attached_mass::bind(pattern, &mesh, &weld, &mut model)?;
    let mut elastic_rings = Vec::new();
    let mut elastic_groups = Vec::new();
    for e in elastics {
        let first = elastic_rings.len();
        for ring in elastic_rings_of(&mesh, &weld, e)? {
            let at = model.elastics.len();
            model.elastics.extend(ring);
            elastic_rings.push(at..model.elastics.len());
        }
        let owners: BTreeSet<_> = e.load_instances.iter().collect();
        let loaded_vertices: BTreeSet<_> = mesh
            .owner
            .iter()
            .enumerate()
            .filter(|(_, owner)| owners.contains(&mesh.instances[**owner]))
            .map(|(v, _)| weld[v])
            .collect();
        // Reassemble ONLY the admitted sources into nodal order, with exactly the same elementary
        // mass shares as the solver. Summing whole welded vertices would borrow neighboring panels
        // and accessories. Retained elements exclude any triangles collapsed by welding.
        let mut loaded_mass = vec![0.0; model.mass.len()];
        for (t, element) in model.membranes.iter().enumerate() {
            let owner = triangle_owner[t];
            if owners.contains(&mesh.instances[owner]) {
                let density = instance_fabrics
                    .as_ref()
                    .map_or(fabric.areal_kg_m2, |field| field[owner].areal_kg_m2);
                let share = if let Some(mults) = &tri_mass_mult {
                    (density * mults[t]) * element.area / 3.0
                } else {
                    density * element.area / 3.0
                };
                for &v in &element.v {
                    loaded_mass[v] += share;
                }
            }
        }
        let loaded_cloth_mass_kg = loaded_mass.iter().sum();
        let mut loaded_attached_mass_kg = 0.0;
        if let Some(attachments) = model.attached_mass_report()? {
            for group in &attachments.groups {
                for target in &group.targets {
                    if owners.contains(&target.instance) {
                        loaded_mass[target.solver_vertex] += target.mass_kg;
                        loaded_attached_mass_kg += target.mass_kg;
                    }
                }
            }
        }
        let loaded_mass_kg: f64 = loaded_mass.iter().sum();
        if !loaded_mass_kg.is_finite() || loaded_mass_kg <= 0.0 {
            return Err(bad(
                "casing load has no positive finite simulated mass".into()
            ));
        }
        elastic_groups.push(ElasticGroup {
            casing: e.clone(),
            rings: first..elastic_rings.len(),
            loaded_vertices: loaded_vertices.into_iter().collect(),
            loaded_cloth_mass_kg,
            loaded_attached_mass_kg,
            loaded_mass_kg,
        });
    }
    let body = Body::new(skin_vertices.to_vec(), skin_triangles.to_vec(), 5e-3)?;
    let untangled = untangle(&mut x, &tris, &body, &tree)?;
    model.check_membrane_domains(&x)?;
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
    // Admit geometry with zero offset only long enough to bind the actual per-feature material.
    // The binding audits full physical offsets before any scene escapes or solver step runs. Using
    // a uniform maximum for this first admission would incorrectly reject a clear thin panel.
    let admission = if contact_materials.is_some() {
        ContactParams {
            thickness: 0.0,
            ..contact
        }
    } else {
        start_contact
    };
    let mut solver =
        Solver::new(model, x, &vec![[false; 3]; next], Vec3::ZERO).with_contact(body, admission)?;
    if let Some(field) = contact_materials {
        let ids: Vec<_> = triangle_owner
            .iter()
            .map(|&owner| {
                let piece = pattern
                    .instance(&mesh.instances[owner])
                    .expect("resolved instance")
                    .piece_index;
                field
                    .piece_ids
                    .get(piece)
                    .copied()
                    .flatten()
                    .ok_or_else(|| bad("retained triangle has no active contact material".into()))
            })
            .collect::<Result<_, _>>()?;
        let bound = solver
            .contact
            .take()
            .expect("contact was created")
            .with_surface_materials(&solver.x, &ids, &field.surfaces, field.friction.as_deref())?;
        solver.contact = Some(bound);
    }
    Ok(Scene {
        mesh,
        placements: places,
        start,
        weld,
        collapsed,
        triangle_owner,
        untangled,
        start_backtracked_from: None,
        start_leg_separation: None,
        target_thickness: contact.thickness,
        solver,
        elastic_rings,
        elastic_groups,
        construction_field,
        authored_crease_hinges,
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

/// Raises the contact offset toward its target by at most [`THICKNESS_RAMP`], retaining more than a
/// tenth of every current gap, as CCD does. An absolute activation-band margin can exceed a loaded
/// contact's equilibrium gap and prevent further offset growth even when the target is clear.
fn ramp_thickness(c: &mut Contact, x: &[Vec3], target: f64) {
    if c.params.thickness < target {
        let old = c.params.thickness;
        let remaining = target - old;
        let mut increment = THICKNESS_RAMP.min(remaining);
        // If every bounded trial fails, wait for the cloth to move. Repeatedly spending a
        // fraction of a stationary panel's last gap drives its barrier force toward infinity.
        for _ in 0..8 {
            let next = if increment == remaining {
                target
            } else {
                (old + increment).min(target)
            };
            if next == old {
                break;
            }
            c.params.thickness = next;
            // g_new = g_old - delta, so g_new > g_old/10 iff g_new > delta/9.
            // Per-material offsets grow by at most this global increment. Query beyond the
            // activation band when necessary so an inactive but unsafe pair cannot be missed.
            let guard = (next - old) / 9.0;
            if c.min_gap(x, c.params.dhat.max(guard))
                .is_none_or(|g| g > guard)
            {
                return;
            }
            increment *= 0.5;
        }
        c.params.thickness = old;
    }
}

/// One nominal interval; the Newton residual is in N, correction in m and potential in J.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseStep {
    /// Newton diagnostics for the final accepted implicit substep. Without subdivision this is
    /// the whole nominal interval. Potentials from different step sizes are never added together.
    pub solve: SolveReport,
    /// Largest instantaneous vertex speed over every accepted endpoint in this nominal interval
    /// (m/s), never a window-mean drift or a coarse displacement divided by the nominal duration.
    pub max_speed_m_s: f64,
    /// Accepted endpoint contact state, absent if contact is disabled.
    pub contact: Option<crate::contact::ContactSample>,
    /// Complete physical partition when explicitly requested; nominal animation timing is retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_subdivision: Option<PhaseSubdivision>,
}

/// Actual accepted time partition and diagnostics for one requested nominal interval.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseSubdivision {
    /// The complete requested physical duration, never shortened after a failure.
    pub duration_s: f64,
    /// All implicit-step attempts, including rejected attempts discarded before bisection.
    pub trials: usize,
    /// Chronological accepted substeps; their endpoints are relative to this nominal interval.
    pub accepted: Vec<PhaseSubstep>,
}

/// Diagnostics from an actual accepted substep; no full intermediate geometry is retained here.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseSubstep {
    /// Exact physical time partition and the unchanged implicit solver's report.
    #[serde(flatten)]
    pub step: AcceptedSubstep,
    /// Largest instantaneous vertex speed at this physical endpoint, m/s.
    pub max_speed_m_s: f64,
    /// Contact diagnostics at this same endpoint, absent when contact is disabled.
    pub contact: Option<crate::contact::ContactSample>,
}

/// One phase's per-step reports.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PhaseReport {
    /// Nominal intervals taken, equal to retained animation-frame count.
    pub steps: usize,
    /// Newton iterations over accepted physical substeps, summed.
    pub iterations: usize,
    /// Steps that did not converge.
    pub unconverged: usize,
    /// Sparse factorizations over accepted physical substeps, summed.
    pub factorizations: usize,
    /// Iterations whose step the CCD bound cut, summed.
    pub ccd_limited: usize,
    /// Line-search halvings, summed.
    pub halvings: usize,
    /// Iterations that projected element Hessians to PSD, summed.
    pub projected_iterations: usize,
    /// Preconditioned conjugate-gradient iterations, summed.
    pub pcg_iterations: usize,
    /// Largest vertex speed at the end (m/s).
    pub max_speed: f64,
    /// The cloth offset at the end (m).
    pub thickness: f64,
    /// Completed nominal intervals in execution order, with an optional actual time partition.
    /// A failed interval produces no sample or animation frame.
    pub samples: Vec<PhaseStep>,
}

impl PhaseReport {
    /// Continue one phase without discarding accepted solver/contact samples or final diagnostics.
    pub(crate) fn append(&mut self, next: Self) {
        self.steps += next.steps;
        self.iterations += next.iterations;
        self.unconverged += next.unconverged;
        self.factorizations += next.factorizations;
        self.ccd_limited += next.ccd_limited;
        self.halvings += next.halvings;
        self.projected_iterations += next.projected_iterations;
        self.pcg_iterations += next.pcg_iterations;
        self.max_speed = next.max_speed;
        self.thickness = next.thickness;
        self.samples.extend(next.samples);
    }
}

/// Runs `steps` implicit steps of `h` seconds under `gravity` (m/s², along −z), calling `frame` after
/// each with the step index and the mesh-vertex positions.
///
/// # Errors
/// A refusal from the solver, including a stalled Newton solve. Accepted earlier steps remain;
/// the failed step emits no frame and does not advance the contact-offset ramp.
pub fn run_phase(
    scene: &mut Scene,
    steps: usize,
    h: f64,
    gravity: f64,
    frame: impl FnMut(usize, &[Vec3]),
) -> Result<PhaseReport, Refusal> {
    run_phase_with_subdivision(scene, steps, h, gravity, None, &mut 0, frame)
}

fn max_speed_or_nan(velocities: &[Vec3]) -> f64 {
    let mut best = 0.0_f64;
    for v in velocities {
        let s = v.norm();
        if s.is_nan() {
            return f64::NAN;
        }
        if s > best {
            best = s;
        }
    }
    best
}

/// Ordinary drape phases share one explicit attempted-step allowance, including a settling tail.
/// The old path remains unchanged when the policy is absent. With subdivision, the thickness ramp,
/// solver caches, friction and material memory commit only after the whole nominal interval succeeds.
pub(crate) fn run_phase_with_subdivision(
    scene: &mut Scene,
    steps: usize,
    h: f64,
    gravity: f64,
    policy: Option<SubdivisionOptions>,
    trials_used: &mut usize,
    mut frame: impl FnMut(usize, &[Vec3]),
) -> Result<PhaseReport, Refusal> {
    if let Some(policy) = policy {
        policy.validate(h)?;
    }
    // Successful empty phases still select their load, as on the ordinary path. A failed
    // subdivided interval must not change gravity or any other part of the accepted state.
    let target_gravity = Vec3::new(0.0, 0.0, -gravity);
    if (policy.is_none() || steps == 0) && scene.solver.gravity != target_gravity {
        scene.solver.gravity = target_gravity;
        scene.solver.reset_acceleration();
    }
    let mut rep = PhaseReport::default();
    for k in 0..steps {
        if let Some(policy) = policy {
            let remaining = policy.max_trials.saturating_sub(*trials_used);
            if remaining == 0 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "shared sewing/draping/settling subdivision trial budget exhausted; nominal interval unchanged",
                ));
            }
            let mut candidate = scene.solver.clone();
            if candidate.gravity != target_gravity {
                candidate.gravity = target_gravity;
                candidate.reset_acceleration();
            }
            if let Some(contact) = candidate.contact.as_mut() {
                contact.adapt_barrier_stiffness(&candidate.x);
                ramp_thickness(contact, &candidate.x, scene.target_thickness);
            }
            let (partition, observations) = candidate.step_subdivided_with(
                h,
                SubdivisionOptions {
                    max_trials: remaining,
                    ..policy
                },
                |solver| {
                    (
                        max_speed_or_nan(&solver.v),
                        solver
                            .contact
                            .as_ref()
                            .map(|c| c.accepted_sample(&solver.x)),
                    )
                },
            )?;
            *trials_used += partition.trials;
            let solve = partition
                .accepted
                .last()
                .expect("complete nonempty interval")
                .solve;
            let mut max_speed_m_s = 0.0_f64;
            let mut accepted = Vec::with_capacity(partition.accepted.len());
            for (step, (speed, contact)) in partition.accepted.into_iter().zip(observations) {
                if !speed.is_finite() {
                    return Err(Refusal::new(
                        RefusalCode::NonFinite,
                        "cloth velocity became non-finite during subdivided drape step",
                    ));
                }
                rep.iterations += step.solve.iterations;
                rep.factorizations += step.solve.factorizations;
                rep.ccd_limited += step.solve.ccd_limited;
                rep.halvings += step.solve.halvings;
                rep.projected_iterations += step.solve.projected_iterations;
                rep.pcg_iterations += step.solve.pcg_iterations;
                max_speed_m_s = max_speed_m_s.max(speed);
                accepted.push(PhaseSubstep {
                    step,
                    max_speed_m_s: speed,
                    contact,
                });
            }
            rep.steps += 1;
            rep.samples.push(PhaseStep {
                solve,
                max_speed_m_s,
                contact: accepted
                    .last()
                    .expect("complete nonempty interval")
                    .contact
                    .clone(),
                time_subdivision: Some(PhaseSubdivision {
                    duration_s: partition.duration_s,
                    trials: partition.trials,
                    accepted,
                }),
            });
            scene.solver = candidate;
            frame(k, &scene.positions());
            continue;
        }
        let saved = scene.solver.contact.as_ref().map(|c| {
            (
                c.params.thickness,
                c.params.kappa,
                c.self_contact_params().map(|p| p.kappa),
            )
        });
        if let Some(contact) = scene.solver.contact.as_mut() {
            contact.adapt_barrier_stiffness(&scene.solver.x);
            ramp_thickness(contact, &scene.solver.x, scene.target_thickness);
        }
        let r: SolveReport = match scene.solver.step(h) {
            Ok(r) => r,
            Err(e) => {
                if let (Some(c), Some((thickness, body_kappa, self_kappa))) =
                    (&mut scene.solver.contact, saved)
                {
                    c.params.thickness = thickness;
                    c.restore_kappas(body_kappa, self_kappa);
                }
                return Err(e);
            }
        };
        rep.steps += 1;
        rep.iterations += r.iterations;
        rep.unconverged += usize::from(!r.converged);
        rep.factorizations += r.factorizations;
        rep.ccd_limited += r.ccd_limited;
        rep.halvings += r.halvings;
        rep.projected_iterations += r.projected_iterations;
        rep.pcg_iterations += r.pcg_iterations;
        let step_speed = max_speed_or_nan(&scene.solver.v);
        if !step_speed.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "cloth velocity became non-finite during drape step",
            ));
        }
        rep.samples.push(PhaseStep {
            solve: r,
            max_speed_m_s: step_speed,
            contact: scene
                .solver
                .contact
                .as_ref()
                .map(|c| c.accepted_sample(&scene.solver.x)),
            time_subdivision: None,
        });
        frame(k, &scene.positions());
    }
    let final_speed = max_speed_or_nan(&scene.solver.v);
    if !final_speed.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "cloth velocity is non-finite at end of drape phase",
        ));
    }
    rep.max_speed = final_speed;
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

#[cfg(test)]
mod grain_tests {
    use super::*;
    use crate::garment::build_garment;
    use crate::start::roll_points;
    use serde_json::json;

    fn subdivision_scene() -> Scene {
        let rest = vec![Vec2::ZERO, Vec2::new(0.1, 0.), Vec2::new(0., 0.1)];
        let faces = vec![[0, 1, 2]];
        let model = ClothModel::new(
            rest.clone(),
            &faces,
            crate::Membrane::orthotropic(1000., 600., 0., 300., 1.),
            0.,
            0.2,
        )
        .unwrap();
        let x = vec![Vec3::ZERO, Vec3::new(0.11, 0., 0.), Vec3::new(0., 0.1, 0.)];
        let mut solver = Solver::new(
            model,
            x.clone(),
            &[[true; 3], [false, true, true], [true; 3]],
            Vec3::new(0., 0., -0.),
        );
        solver.tol = 1e-6;
        solver.pcg_iters = 10;
        Scene {
            mesh: GarmentMesh {
                instances: vec!["synthetic".into()],
                verts2d: rest.iter().map(|p| *p / MM).collect(),
                faces,
                owner: vec![0; 3],
                inst_range: vec![(0, 3)],
                pairs: vec![],
                seams: vec![],
                corner: BTreeMap::new(),
                index: BTreeMap::new(),
                creases: Vec::new(),
            },
            placements: vec![],
            start: StartState {
                wrapped: x.clone(),
                presewn: x.clone(),
                relaxed: x,
                groups: vec![],
                relax_iterations: 0,
                block_p95: vec![],
                gather_weights: vec![1.; 3],
            },
            weld: vec![0, 1, 2],
            collapsed: 0,
            triangle_owner: vec![0],
            untangled: vec![],
            start_backtracked_from: None,
            start_leg_separation: None,
            target_thickness: 0.,
            solver,
            elastic_rings: vec![],
            elastic_groups: vec![],
            construction_field: None,
            authored_crease_hinges: 0,
        }
    }

    #[test]
    fn ordinary_phase_subdivision_retries_real_failures_and_keeps_nominal_frames() {
        let mut scene = subdivision_scene();
        scene.solver.max_newton = 3;
        let original = scene.solver.clone();
        assert_eq!(
            run_phase(&mut scene, 1, 0.04, 0., |_, _| {
                panic!("failed interval emitted a frame")
            })
            .unwrap_err()
            .code,
            RefusalCode::NotConverged
        );
        assert_eq!(format!("{:?}", scene.solver), format!("{original:?}"));
        let mut frames = Vec::new();
        let mut trials = 0;
        let policy = SubdivisionOptions {
            max_halvings: 12,
            max_trials: 4096,
        };
        let report = run_phase_with_subdivision(
            &mut scene,
            1,
            0.04,
            0.,
            Some(policy),
            &mut trials,
            |index, points| frames.push((index, points.to_vec())),
        )
        .unwrap();
        assert_eq!(report.steps, 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], (0, scene.positions()));
        let sample = &report.samples[0];
        let partition = sample.time_subdivision.as_ref().unwrap();
        assert_eq!(partition.duration_s, 0.04);
        assert_eq!(partition.accepted.last().unwrap().step.end_s, 0.04);
        assert!(partition.accepted.len() > 1 && partition.trials > partition.accepted.len());
        assert_eq!(trials, partition.trials);
        assert_eq!(sample.solve, partition.accepted.last().unwrap().step.solve);
        assert_eq!(
            sample.max_speed_m_s,
            partition
                .accepted
                .iter()
                .map(|s| s.max_speed_m_s)
                .fold(0., f64::max)
        );
        assert_eq!(
            report.iterations,
            partition
                .accepted
                .iter()
                .map(|s| s.step.solve.iterations)
                .sum::<usize>()
        );
        let mut replay = original.clone();
        for substep in &partition.accepted {
            assert_eq!(
                replay.step(substep.step.step_s).unwrap(),
                substep.step.solve
            );
            assert_eq!(
                substep.max_speed_m_s,
                replay.v.iter().map(|v| v.norm()).fold(0., f64::max)
            );
        }
        assert_eq!(format!("{:?}", scene.solver), format!("{replay:?}"));

        let accepted_state = format!("{:?}", scene.solver);
        // A new phase cannot reset the allowance or change the accepted load before success.
        let error = run_phase_with_subdivision(
            &mut scene,
            1,
            0.04,
            9.81,
            Some(SubdivisionOptions {
                max_trials: trials,
                ..policy
            }),
            &mut trials,
            |_, _| panic!("exhausted shared budget emitted a frame"),
        )
        .unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        assert_eq!(format!("{:?}", scene.solver), accepted_state);

        scene.solver = original;
        let before = format!("{:?}", scene.solver);
        let mut used = 0;
        let error = run_phase_with_subdivision(
            &mut scene,
            1,
            0.04,
            9.81,
            Some(SubdivisionOptions {
                max_trials: partition.trials - 1,
                ..policy
            }),
            &mut used,
            |_, _| panic!("partial nominal interval emitted a frame"),
        )
        .unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        assert_eq!(format!("{:?}", scene.solver), before);
        assert_eq!(
            used, 0,
            "failed run publishes no successful interval receipt"
        );
    }

    #[test]
    fn subdivision_nominal_success_matches_the_existing_phase_state_and_work() {
        let mut ordinary = subdivision_scene();
        let mut adaptive = subdivision_scene();
        let mut ordinary_frames = Vec::new();
        let baseline = run_phase(&mut ordinary, 2, 0.01, 0., |_, x| {
            ordinary_frames.push(x.to_vec())
        })
        .unwrap();
        let mut adaptive_frames = Vec::new();
        let mut trials = 0;
        let mut report = run_phase_with_subdivision(
            &mut adaptive,
            2,
            0.01,
            0.,
            Some(SubdivisionOptions::default()),
            &mut trials,
            |_, x| adaptive_frames.push(x.to_vec()),
        )
        .unwrap();
        assert_eq!(trials, 2);
        for sample in &mut report.samples {
            assert_eq!(sample.time_subdivision.take().unwrap().accepted.len(), 1);
        }
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::to_value(baseline).unwrap()
        );
        assert_eq!(ordinary_frames, adaptive_frames);
        assert_eq!(
            format!("{:?}", ordinary.solver),
            format!("{:?}", adaptive.solver)
        );
    }

    #[test]
    fn subdivision_empty_phase_preserves_the_existing_gravity_transition() {
        let mut ordinary = subdivision_scene();
        let mut adaptive = subdivision_scene();
        let baseline = run_phase(&mut ordinary, 0, 0.01, 9.81, |_, _| {
            panic!("empty phase emitted a frame")
        })
        .unwrap();
        let policy = SubdivisionOptions::default();
        let mut trials = policy.max_trials;
        let report = run_phase_with_subdivision(
            &mut adaptive,
            0,
            0.01,
            9.81,
            Some(policy),
            &mut trials,
            |_, _| panic!("empty phase emitted a frame"),
        )
        .unwrap();
        assert_eq!(trials, policy.max_trials);
        assert_eq!(adaptive.solver.gravity, Vec3::new(0., 0., -9.81));
        assert_eq!(
            format!("{:?}", ordinary.solver),
            format!("{:?}", adaptive.solver)
        );
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::to_value(baseline).unwrap()
        );
    }

    #[test]
    fn refused_subdivided_phase_restores_the_contact_ramp_and_complete_solver() {
        let mut scene = subdivision_scene();
        for x in &mut scene.solver.x {
            x.z = 0.0005;
        }
        let body = Body::new(
            vec![
                Vec3::new(-1., -1., 0.),
                Vec3::new(1., -1., 0.),
                Vec3::new(0., 1., 0.),
            ],
            vec![[0, 1, 2]],
            0.1,
        )
        .unwrap();
        let params = ContactParams {
            dhat: 0.001,
            thickness: 0.0002,
            kappa: 100.,
            mu: 0.4,
            eps_v: 0.001,
        };
        scene.solver = scene.solver.with_contact(body, params).unwrap();
        scene.solver.step(0.00001).unwrap();
        scene.solver.max_newton = 1;
        scene.target_thickness = 0.0003;
        let mut ramp = scene.solver.contact.clone().unwrap();
        ramp_thickness(&mut ramp, &scene.solver.x, scene.target_thickness);
        assert!(
            ramp.params.thickness > params.thickness,
            "fixture must advance its trial ramp"
        );
        let before = format!("{:?}", scene.solver);
        let refusal = run_phase_with_subdivision(
            &mut scene,
            1,
            0.04,
            9.81,
            Some(SubdivisionOptions {
                max_halvings: 12,
                max_trials: 1,
            }),
            &mut 0,
            |_, _| panic!("failed nominal interval emitted a frame"),
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
        assert_eq!(format!("{:?}", scene.solver), before);
    }

    #[test]
    fn body_offset_ramp_backtracks_without_dropping_its_relative_safety_margin() {
        let body = Body::new(
            vec![
                Vec3::new(-0.01, -0.01, 0.0),
                Vec3::new(0.02, -0.01, 0.0),
                Vec3::new(-0.01, 0.02, 0.0),
            ],
            vec![[0, 1, 2]],
            0.01,
        )
        .unwrap();
        let x = [
            Vec3::new(0.0, 0.0, 8e-4),
            Vec3::new(1e-3, 0.0, 8e-4),
            Vec3::new(0.0, 1e-3, 8e-4),
        ];
        let mut contact = Contact::new(
            body,
            ContactParams {
                dhat: 1e-3,
                thickness: 5e-4,
                kappa: 100.0,
                mu: 0.4,
                eps_v: 1e-3,
            },
            vec![[0, 1, 2]],
            |a, b| (x[a] - x[b]).norm(),
        );
        let old_gap = contact.min_gap(&x, 1e-3).unwrap();
        ramp_thickness(&mut contact, &x, 1.5e-3);
        assert_eq!(contact.params.thickness, 7.5e-4);
        assert!(contact.min_gap(&x, 1e-3).unwrap() > 0.1 * old_gap);
        ramp_thickness(&mut contact, &x, 7.75e-4);
        assert_eq!(contact.params.thickness, 7.75e-4);

        let close = x.map(|p| Vec3::new(p.x, p.y, 8.25e-4));
        let old = contact.params.thickness;
        let old_gap = contact.min_gap(&close, 1e-3).unwrap();
        ramp_thickness(&mut contact, &close, 1.5e-3);
        // A full increment intersects the body; backtracking must preserve a positive gap.
        assert!((contact.params.thickness - (old + THICKNESS_RAMP / 8.)).abs() < 1e-15);
        assert!(contact.min_gap(&close, 1e-3).unwrap() > 0.1 * old_gap);
        contact.params.thickness = x[0].z;
        ramp_thickness(&mut contact, &x, 1.5e-3);
        assert_eq!(contact.params.thickness, x[0].z);

        let loaded = x.map(|p| Vec3::new(p.x, p.y, p.z + 1e-6));
        let gap = contact.min_gap(&loaded, 1e-3).unwrap();
        for _ in 0..120 {
            ramp_thickness(&mut contact, &loaded, 1.5e-3);
        }
        assert_eq!(contact.min_gap(&loaded, 1e-3), Some(gap));
        let lifted = x.map(|p| Vec3::new(p.x, p.y, p.z + 4e-6));
        ramp_thickness(&mut contact, &lifted, 1.5e-3);
        assert!(contact.params.thickness > x[0].z);
    }

    #[test]
    fn body_offset_ramp_finishes_only_with_relative_clearance() {
        for (name, old, target, distance, dhat, expected, finishes) in [
            (
                "one micron of clearance at the loaded offset",
                0.8e-3,
                1.5e-3,
                0.801e-3,
                1e-3,
                0.8e-3,
                false,
            ),
            (
                "safe final step",
                1.413734e-3,
                1.5e-3,
                1.513872e-3,
                1e-3,
                1.5e-3,
                true,
            ),
            (
                "too little clearance",
                1.41e-3,
                1.5e-3,
                1.509e-3,
                1e-3,
                1.455e-3,
                false,
            ),
            ("touching", 1.41e-3, 1.5e-3, 1.5e-3, 1e-3, 1.455e-3, false),
            (
                "penetrating",
                1.41e-3,
                1.5e-3,
                1.499e-3,
                1e-3,
                1.455e-3,
                false,
            ),
            // The unsafe full increment ends outside the activation band, inside the CCD guard.
            (
                "narrow activation band",
                1.41e-3,
                1.5e-3,
                1.509e-3,
                1e-6,
                1.455e-3,
                false,
            ),
            (
                "one nanometre of clearance at the loaded offset",
                1.41e-3,
                1.5e-3,
                1.410001e-3,
                1e-3,
                1.41e-3,
                false,
            ),
            // The target is clear and preserves >10% of the old gap, but exceeds the step cap.
            (
                "large remaining offset",
                5e-4,
                1.5e-3,
                1.7e-3,
                20e-3,
                7.5e-4,
                false,
            ),
        ] {
            let body = Body::new(
                vec![
                    Vec3::new(-0.01, -0.01, 0.0),
                    Vec3::new(0.02, -0.01, 0.0),
                    Vec3::new(-0.01, 0.02, 0.0),
                ],
                vec![[0, 1, 2]],
                0.01,
            )
            .unwrap();
            let x = [
                Vec3::new(0.0, 0.0, distance),
                Vec3::new(1e-3, 0.0, distance),
                Vec3::new(0.0, 1e-3, distance),
            ];
            let mut contact = Contact::new(
                body,
                ContactParams {
                    dhat,
                    thickness: old,
                    kappa: 100.0,
                    mu: 0.4,
                    eps_v: 1e-3,
                },
                vec![[0, 1, 2]],
                |a, b| (x[a] - x[b]).norm(),
            );
            let range = dhat.max(target);
            let old_gap = contact.min_gap(&x, range).unwrap();
            assert!(old_gap > 0.0, "{name}");
            ramp_thickness(&mut contact, &x, target);
            assert!(
                (contact.params.thickness - expected).abs() < 1e-15,
                "{name}"
            );
            assert_eq!(contact.params.thickness == target, finishes, "{name}");
            assert!(contact.params.thickness - old <= THICKNESS_RAMP, "{name}");
            assert!(
                contact.min_gap(&x, range).unwrap() > 0.1 * old_gap,
                "{name}"
            );
        }
    }

    #[test]
    fn body_offset_ramp_progresses_when_loaded_clearance_is_below_the_band_guard() {
        let body = Body::new(
            vec![
                Vec3::new(-0.01, -0.01, 0.0),
                Vec3::new(0.02, -0.01, 0.0),
                Vec3::new(-0.01, 0.02, 0.0),
            ],
            vec![[0, 1, 2]],
            0.01,
        )
        .unwrap();
        let x = [
            Vec3::new(0.0, 0.0, 1.7e-3),
            Vec3::new(1e-3, 0.0, 1.7e-3),
            Vec3::new(0.0, 1e-3, 1.7e-3),
        ];
        let mut contact = Contact::new(
            body,
            ContactParams {
                dhat: 20e-3,
                thickness: 5e-4,
                kappa: 100.0,
                mu: 0.4,
                eps_v: 1e-3,
            },
            vec![[0, 1, 2]],
            |a, b| (x[a] - x[b]).norm(),
        );
        let target = 1.5e-3;
        // Even the starting gap is below dhat / 10. The requested offset is nevertheless clear,
        // and four bounded increments can reach it while retaining >10% of each preceding gap.
        for _ in 0..4 {
            let old = contact.params.thickness;
            let old_gap = contact.min_gap(&x, contact.params.dhat).unwrap();
            assert!(old_gap < 0.1 * contact.params.dhat);
            ramp_thickness(&mut contact, &x, target);
            assert!(contact.params.thickness > old);
            assert!(contact.params.thickness - old <= THICKNESS_RAMP);
            assert!(contact.min_gap(&x, contact.params.dhat).unwrap() > 0.1 * old_gap);
        }
        assert_eq!(contact.params.thickness, target);
    }

    fn physical_panel(id: &str, part: &str, side: Option<&str>) -> Value {
        let mut piece = json!({"id":id,"cut":{"qty":1},
            "grain":{"p0":[13,0],"p1":[15,20]},
            "vertices":{"a":[13,-2],"b":[37,-2],"c":[41,32],"d":[13,28]},
            "edges":[{"id":"e0","from":"a","to":"b"},{"id":"e1","from":"b","to":"c"},
                {"id":"e2","from":"c","to":"d"},{"id":"e3","from":"d","to":"a"}],
            "placement":{"part":part,"y0_landmark":"anchor","x0":17,"y0":5,
                "theta_deg":37,"clearance_mm":7}});
        if let Some(side) = side {
            piece["placement"]["side"] = json!(side);
        }
        piece
    }

    struct AsymmetricFrames;
    impl PartFrames for AsymmetricFrames {
        fn frame(&self, part: &str) -> Option<PartFrame> {
            let (origin, up) = match part {
                "arm_R" => (Vec3::new(-180., 4., 270.), Vec3::new(0., 0., 1.)),
                "arm_L" => (Vec3::new(210., 9., 310.), Vec3::new(0.6, 0., 0.8)),
                "leg_R" => (Vec3::new(-5., 0., 300.), Vec3::new(0., 0., 1.)),
                "leg_L" => (Vec3::new(5., 0., 300.), Vec3::new(0., 0., 1.)),
                _ => return None,
            };
            let front = Vec3::new(0., -1., 0.);
            Some(PartFrame {
                origin,
                up,
                front,
                lat: up.cross(front),
            })
        }
        fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
            if name != "anchor" {
                return None;
            }
            match part {
                "arm_R" | "leg_R" => Some(110.),
                "arm_L" | "leg_L" => Some(155.),
                _ => None,
            }
        }
        fn section_radius(&self, part: &str, h: f64) -> f64 {
            match part {
                "arm_R" | "leg_R" => 40. + 0.1 * h,
                "arm_L" | "leg_L" => 55. + 0.2 * h,
                _ => 0.,
            }
        }
    }

    #[test]
    fn physical_panels_use_selected_frames_without_mirroring_flat_geometry() {
        let mut right = physical_panel("right", "arm", Some("R"));
        let mut left = physical_panel("left", "arm", Some("L"));
        right["sim_scale"] = json!([1.5, 0.75]);
        left["sim_scale"] = json!([1.5, 0.75]);
        let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[right,left],"seams":[]}));
        let mesh = build_garment(&pattern, 12.).unwrap();
        let places = placements(&pattern, &mesh, &AsymmetricFrames).unwrap();
        assert_eq!(mesh.instances, ["right", "left"]);
        let wrapped = wrapped_positions(&mesh, &places);
        let rest = grain_rest(&pattern, &mesh, true).unwrap();
        let grain = Vec2::new(3., 15.).normalized();
        for (k, (p, key)) in places.iter().zip(["arm_R", "arm_L"]).enumerate() {
            assert_eq!(p.frame, AsymmetricFrames.frame(key).unwrap());
            assert_eq!(p.x0, 25.5);
            assert_eq!(p.h0, if k == 0 { 106.25 } else { 151.25 });
            assert_eq!(p.theta_deg, 37.);
            assert!(!p.mirrored && !p.own_side && !p.own_side_left);
            assert!(!pattern.instance(&mesh.instances[k]).unwrap().mirrored);
            let (lo, hi) = mesh.inst_range[k];
            let max_y = mesh.verts2d[lo..hi]
                .iter()
                .map(|q| q.y)
                .fold(f64::NEG_INFINITY, f64::max);
            assert_eq!(
                p.radius,
                AsymmetricFrames.section_radius(key, p.h0 + max_y) + 7.
            );
            for (i, q) in mesh.verts2d.iter().enumerate().take(hi).skip(lo) {
                let theta = 37_f64.to_radians() + (q.x - 25.5) / p.radius;
                let expected = (p.frame.origin
                    + p.frame.up * (p.h0 + q.y)
                    + (p.frame.lat * theta.sin() + p.frame.front * theta.cos()) * p.radius)
                    * MM;
                assert!((wrapped[i] - expected).norm() < 1e-14);
                let expected_rest = Vec2::new(grain.dot(*q), grain.cross(*q)) * MM;
                assert!((rest[i] - expected_rest).norm() < 1e-15);
            }
            for t in mesh.faces.iter().filter(|t| mesh.owner[t[0]] == k) {
                let normal = (wrapped[t[1]] - wrapped[t[0]]).cross(wrapped[t[2]] - wrapped[t[0]]);
                let center = (wrapped[t[0]] + wrapped[t[1]] + wrapped[t[2]]) / 3.;
                let mean_y = t.iter().map(|&i| mesh.verts2d[i].y).sum::<f64>() / 3.;
                let radial = center - (p.frame.origin + p.frame.up * (p.h0 + mean_y)) * MM;
                assert!(normal.dot(radial) > 0., "{key}: public side faces outward");
            }
            // The physical anchor is fixed throughout the roll, including an asymmetric left frame.
            let points = [
                Vec2::new(p.x0, 9.),
                Vec2::new(p.x0 + 12., 9.),
                Vec2::new(p.x0, 17.),
            ];
            let theta = p.theta_deg.to_radians();
            let radial = p.frame.lat * theta.sin() + p.frame.front * theta.cos();
            let expected_anchor =
                (p.frame.origin + p.frame.up * (p.h0 + 9.) + radial * p.radius) * MM;
            for progress in [0., 0.4, 0.9, 1.] {
                let rolled = roll_points(&points, p, progress);
                assert!((rolled[0] - expected_anchor).norm() < 1e-14);
                assert!(
                    (rolled[1] - rolled[0])
                        .cross(rolled[2] - rolled[0])
                        .dot(radial)
                        > 0.
                );
            }
            assert_eq!(roll_points(&points, p, 1.), wrap_points(&points, p));
        }
    }

    #[test]
    fn placement_frame_adjustments_preserve_flat_lengths_and_every_roll_stage() {
        for side in ["R", "L"] {
            let mut piece = physical_panel("sleeve", "arm", Some(side));
            piece["sim_scale"] = json!([1.5, 0.75]);
            let original = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
                "pieces":[piece.clone()],"seams":[]}));
            piece["placement"]["frame_offset_mm"] = json!([11., 23., -7.]);
            piece["placement"]["frame_rotation"] = json!({"axis":[0.,2.,0.],"angle_deg":90.});
            let adjusted = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
                "pieces":[piece],"seams":[]}));
            let mesh = build_garment(&original, 12.).unwrap();
            let adjusted_mesh = build_garment(&adjusted, 12.).unwrap();
            assert_eq!(mesh.verts2d, adjusted_mesh.verts2d);
            assert_eq!(mesh.faces, adjusted_mesh.faces);
            assert_eq!(
                grain_rest(&original, &mesh, true).unwrap(),
                grain_rest(&adjusted, &adjusted_mesh, true).unwrap()
            );
            let base = placements(&original, &mesh, &AsymmetricFrames).unwrap()[0];
            let moved = placements(&adjusted, &adjusted_mesh, &AsymmetricFrames).unwrap()[0];
            assert_eq!(base.h0, moved.h0);
            assert_eq!(base.x0, moved.x0);
            assert_eq!(base.radius, moved.radius);
            assert_eq!(base.theta_deg, moved.theta_deg);
            let landmark = if side == "R" { 110. } else { 155. };
            let pivot = base.frame.origin + base.frame.up * landmark;
            let translation = base.frame.front * 11. + base.frame.lat * 23. - base.frame.up * 7.;
            assert!(
                (moved.frame.origin + moved.frame.up * landmark - pivot - translation).norm()
                    < 1e-12
            );
            assert!((moved.frame.up - base.frame.front).norm() < 1e-14);
            assert!((moved.frame.front + base.frame.up).norm() < 1e-14);
            assert!((moved.frame.lat - base.frame.lat).norm() < 1e-14);
            assert!((moved.frame.up.cross(moved.frame.front) - moved.frame.lat).norm() < 1e-14);
            for progress in [0., 0.35, 0.9, 1.] {
                let before = roll_points(&mesh.verts2d, &base, progress);
                let after = roll_points(&adjusted_mesh.verts2d, &moved, progress);
                for (point, placed) in before.iter().zip(&after) {
                    // Independent 90-degree rotation about the original local lateral axis:
                    // front -> -up, up -> front, with the landmark centerline fixed.
                    let d = *point / MM - pivot;
                    let expected = (pivot
                        + base.frame.front * d.dot(base.frame.up)
                        + base.frame.lat * d.dot(base.frame.lat)
                        - base.frame.up * d.dot(base.frame.front)
                        + translation)
                        * MM;
                    assert!((*placed - expected).norm() < 1e-14, "{side}, {progress}");
                }
                for (a, b) in crate::start::face_edges(&mesh.faces) {
                    assert!(
                        ((after[b] - after[a]).norm() - (before[b] - before[a]).norm()).abs()
                            < 1e-14
                    );
                }
            }
            assert_eq!(
                roll_points(&mesh.verts2d, &moved, 1.),
                wrap_points(&mesh.verts2d, &moved)
            );
        }
    }

    #[test]
    fn placement_frame_identity_and_legacy_mirror_are_exact() {
        let mut piece = physical_panel("sleeve", "arm", None);
        piece["cut"]["qty"] = json!(2);
        let original = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece.clone()],"seams":[]}));
        let mesh = build_garment(&original, 12.).unwrap();
        let base = placements(&original, &mesh, &AsymmetricFrames).unwrap();
        piece["placement"]["frame_offset_mm"] = json!([-0., 0., -0.]);
        piece["placement"]["frame_rotation"] = json!({"axis":[1e308,1e308,1e308],"angle_deg":-0.});
        let identity = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece.clone()],"seams":[]}));
        let same = placements(&identity, &mesh, &AsymmetricFrames).unwrap();
        assert_eq!(base, same);
        for (before, after) in wrapped_positions(&mesh, &base)
            .iter()
            .zip(wrapped_positions(&mesh, &same))
        {
            assert_eq!(
                [before.x.to_bits(), before.y.to_bits(), before.z.to_bits()],
                [after.x.to_bits(), after.y.to_bits(), after.z.to_bits()]
            );
        }
        piece["placement"]["frame_offset_mm"] = json!([5., 8., -12.]);
        piece["placement"]["frame_rotation"] = json!({"axis":[2.,3.,4.],"angle_deg":-37.});
        let adjusted = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece],"seams":[]}));
        let moved = placements(&adjusted, &mesh, &AsymmetricFrames).unwrap();
        assert_eq!(moved[0].frame, moved[1].frame);
        assert!(!moved[0].mirrored && moved[1].mirrored);
        for progress in [0., 0.6, 1.] {
            let right = roll_points(&[Vec2::new(23., 7.)], &moved[0], progress)[0];
            let left = roll_points(&[Vec2::new(-23., 7.)], &moved[1], progress)[0];
            assert_eq!(left, Vec3::new(-right.x, right.y, right.z));
        }
    }

    #[test]
    fn placement_frame_axis_direction_is_scale_independent() {
        let frame = AsymmetricFrames.frame("arm_L").unwrap();
        for angle in [-180., 180., 43.] {
            let expected = adjusted_frame(
                frame,
                155.,
                &json!({"frame_rotation":{"axis":[1.,-1.,1.],"angle_deg":angle}}),
                "left",
            )
            .unwrap();
            for scale in [f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
                let scaled = adjusted_frame(
                    frame,
                    155.,
                    &json!({"frame_rotation":{"axis":[scale,-scale,scale],"angle_deg":angle}}),
                    "left",
                )
                .unwrap();
                assert_eq!(expected, scaled);
            }
        }
    }

    #[test]
    fn placement_frame_adjustments_refuse_malformed_and_unrepresentable_values() {
        let piece = physical_panel("sleeve", "arm", Some("R"));
        let source = json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece],"seams":[]});
        let mesh = build_garment(&Pattern::new(source.clone()), 12.).unwrap();
        for (key, value) in [
            ("frame_offset_mm", json!(null)),
            ("frame_offset_mm", json!(30.)),
            ("frame_offset_mm", json!([1., 2.])),
            ("frame_offset_mm", json!([1., 2., 3., 4.])),
            ("frame_offset_mm", json!([1., "2", 3.])),
            ("frame_rotation", json!(null)),
            ("frame_rotation", json!([0., 0., 90.])),
            ("frame_rotation", json!({"axis":[0.,1.,0.]})),
            ("frame_rotation", json!({"angle_deg":90.})),
            (
                "frame_rotation",
                json!({"axis":[0.,1.,0.],"angle_deg":90.,"pivot":"origin"}),
            ),
            ("frame_rotation", json!({"axis":[0.,0.,0.],"angle_deg":0.})),
            ("frame_rotation", json!({"axis":[0.,1.],"angle_deg":90.})),
            (
                "frame_rotation",
                json!({"axis":[0.,1.,0.],"angle_deg":"90"}),
            ),
            (
                "frame_rotation",
                json!({"axis":[0.,1.,0.],"angle_deg":180.001}),
            ),
            (
                "frame_rotation",
                json!({"axis":[0.,1.,0.],"angle_deg":-180.001}),
            ),
        ] {
            let mut invalid = source.clone();
            invalid["pieces"][0]["placement"][key] = value;
            let refusal = placements(&Pattern::new(invalid), &mesh, &AsymmetricFrames).unwrap_err();
            assert!(refusal.message.contains("sleeve"));
            assert!(refusal.message.contains(key));
        }
        let mut frame = AsymmetricFrames.frame("arm_R").unwrap();
        frame.origin.x = f64::MAX;
        let refusal = adjusted_frame(
            frame,
            110.,
            &json!({"frame_offset_mm":[0.,f64::MAX,0.]}),
            "sleeve",
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::OutOfRange);
        assert!(refusal.message.contains("not representable"));
        frame.up = frame.front;
        let refusal = adjusted_frame(
            frame,
            110.,
            &json!({"frame_offset_mm":[0.,1.,0.]}),
            "sleeve",
        )
        .unwrap_err();
        assert_eq!(refusal.code, RefusalCode::DegenerateGeometry);
    }

    #[test]
    fn placement_frame_offset_clears_a_displayed_obstacle_with_the_same_flat_cut() {
        let mut piece = physical_panel("sleeve", "arm", Some("R"));
        piece["placement"]["radius_mm"] = json!(40.);
        let original = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece.clone()],"seams":[]}));
        let mesh = build_garment(&original, 8.).unwrap();
        let base = placements(&original, &mesh, &AsymmetricFrames).unwrap();
        // A synthetic protrusion crosses the original wrap. This exercises the actual displayed-
        // surface checker without inferring anatomical hand dimensions or a successful drape.
        let obstacle = [
            Vec3::new(-0.150, -0.1, 0.35),
            Vec3::new(-0.150, 0.1, 0.35),
            Vec3::new(-0.150, 0., 0.5),
        ];
        let mut contact = crate::animation::ReplayContact::new(&obstacle, &[[0, 1, 2]]).unwrap();
        let refusal = contact
            .check(&wrapped_positions(&mesh, &base), &mesh.faces, 0)
            .unwrap_err();
        assert!(refusal.message.contains("displayed body triangle"));
        piece["placement"]["frame_offset_mm"] = json!([0., 60., 0.]);
        let adjusted = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[piece],"seams":[]}));
        let adjusted_mesh = build_garment(&adjusted, 8.).unwrap();
        assert_eq!(mesh.verts2d, adjusted_mesh.verts2d);
        assert_eq!(mesh.faces, adjusted_mesh.faces);
        let moved = placements(&adjusted, &adjusted_mesh, &AsymmetricFrames).unwrap();
        contact
            .check(
                &wrapped_positions(&adjusted_mesh, &moved),
                &adjusted_mesh.faces,
                0,
            )
            .unwrap();
    }

    #[test]
    fn imported_two_panel_sleeve_keeps_seams_and_wraps_on_the_selected_limb() {
        const SOURCE: &str =
            include_str!("../../../tests/fixtures/import/synthetic-sleeve.garmentcode.json");
        let annotations: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/import/synthetic-sleeve.annotations.json"
        ))
        .unwrap();
        struct RightOnlyFrames;
        impl PartFrames for RightOnlyFrames {
            fn frame(&self, part: &str) -> Option<PartFrame> {
                AsymmetricFrames
                    .frame(part)
                    .filter(|_| part.ends_with("_R"))
            }
            fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
                AsymmetricFrames.landmark_h(part, name)
            }
            fn section_radius(&self, part: &str, h: f64) -> f64 {
                AsymmetricFrames.section_radius(part, h)
            }
        }
        for side in ["R", "L"] {
            let mut notes = annotations.clone();
            // Two 100 mm cuff edges make a 200 mm circumference; both pieces form one sleeve.
            for (panel, angle) in [("outer", 0.), ("under", 180.)] {
                notes["panels"][panel]["sim"] = json!(true);
                notes["panels"][panel]["placement"] = json!({"part":"arm","side":side,
                    "y0_landmark":"anchor","y0":200.,"x0":0.,"theta_deg":angle,
                    "radius_mm":100. / std::f64::consts::PI});
            }
            let imported = ftx_pattern::garmentcode_import::import_garmentcode_json(
                SOURCE,
                Some(&serde_json::to_string(&notes).unwrap()),
            )
            .unwrap();
            assert_eq!(imported.report["status"], "REVIEW_REQUIRED");
            assert_eq!(imported.report["fit_spec"], annotations["fit"]);
            let pattern = Pattern::new(imported.pattern);
            assert_eq!(pattern.instances.len(), 2);
            assert_eq!(pattern.raw()["meta"]["import"]["status"], "REVIEW_REQUIRED");
            for (id, cuff, sides) in [
                ("p0000", "e0000", ["e0001", "e0003"]),
                ("p0001", "e0003", ["e0000", "e0002"]),
            ] {
                let instance = pattern.instance(id).unwrap();
                assert!(!instance.mirrored);
                assert!((instance.edge_length(cuff).unwrap() - 100.).abs() < 1e-10);
                for edge in sides {
                    assert!((instance.edge_length(edge).unwrap() - 200.).abs() < 1e-10);
                }
            }
            let flat = ftx_pattern::validate(
                &pattern,
                &imported.report["fit_spec"],
                &ftx_pattern::Measurements(json!({"cuff_pass":200.})),
            );
            assert_eq!(flat.status(), "CONVERGED");
            let mesh = build_garment(&pattern, 12.).unwrap();
            assert_eq!(mesh.instances, ["p0000", "p0001"]);
            assert_eq!(mesh.seams.len(), 2);
            assert!(mesh.seams.iter().all(|seam| !seam.pairs.is_empty()));
            assert!(!mesh.pairs.is_empty());
            let places = placements(&pattern, &mesh, &AsymmetricFrames).unwrap();
            let key = format!("arm_{side}");
            for place in &places {
                assert_eq!(place.frame, AsymmetricFrames.frame(&key).unwrap());
                assert!(!place.mirrored);
            }
            let wrapped = wrapped_positions(&mesh, &places);
            assert!(
                wrapped
                    .iter()
                    .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
            );
            assert!(
                wrapped
                    .iter()
                    .all(|p| if side == "R" { p.x < 0. } else { p.x > 0. })
            );
            for &(a, b) in &mesh.pairs {
                assert_ne!(mesh.owner[a], mesh.owner[b]);
                assert!((wrapped[a] - wrapped[b]).norm() < 1e-12);
            }
            for t in &mesh.faces {
                let place = &places[mesh.owner[t[0]]];
                let normal = (wrapped[t[1]] - wrapped[t[0]]).cross(wrapped[t[2]] - wrapped[t[0]]);
                let center = (wrapped[t[0]] + wrapped[t[1]] + wrapped[t[2]]) / 3.;
                let mean_y = t.iter().map(|&i| mesh.verts2d[i].y).sum::<f64>() / 3.;
                let radial =
                    center - (place.frame.origin + place.frame.up * (place.h0 + mean_y)) * MM;
                assert!(normal.dot(radial) > 0.);
            }
            if side == "L" {
                assert!(placements(&pattern, &mesh, &RightOnlyFrames).is_err());
            } else {
                assert!(placements(&pattern, &mesh, &RightOnlyFrames).is_ok());
            }
        }
    }

    // Independent numerical joints and skin regions, not an anatomical body or oracle. The
    // asymmetric variant changes only left joints and radii; the unchanged right readings are
    // the compatibility control. Feed these through BodyFrames::new, not a mock PartFrames.
    fn posed_limb_model(asymmetric: bool) -> (ftx_body::AnnyModel, ftx_body::PosedBody) {
        use ftx_body::{Affine, AnnyData, AnnyModel, PosedBody};
        use ftx_core::Mat3;

        let labels = [
            "root",
            "upperarm01.R",
            "upperarm02.R",
            "wrist.R",
            "upperarm01.L",
            "upperarm02.L",
            "wrist.L",
            "upperleg01.R",
            "lowerleg01.R",
            "foot.R",
            "upperleg01.L",
            "lowerleg01.L",
            "foot.L",
            "neck01",
            "head",
        ];
        let parents = [-1, 0, 1, 2, 0, 4, 5, 0, 7, 8, 0, 10, 11, 0, 13];
        let right_shoulder = Vec3::new(-0.2, 0., 0.8);
        let right_up = Vec3::new(0.6, 0., 0.8);
        let (left_shoulder, left_up, left_radius, left_length) = if asymmetric {
            (
                Vec3::new(0.24, 0.025, 0.85),
                Vec3::new(-0.8, 0., 0.6),
                0.035,
                0.4,
            )
        } else {
            (Vec3::new(0.2, 0., 0.8), Vec3::new(-0.6, 0., 0.8), 0.02, 0.3)
        };
        let (left_hip, left_knee, left_ankle, left_leg_radius) = if asymmetric {
            (
                Vec3::new(0.11, 0., 0.54),
                Vec3::new(0.16, 0., 0.31),
                Vec3::new(0.23, 0., 0.06),
                0.045,
            )
        } else {
            (
                Vec3::new(0.08, 0., 0.5),
                Vec3::new(0.08, 0., 0.25),
                Vec3::new(0.08, 0., 0.05),
                0.025,
            )
        };
        let heads = vec![
            Vec3::ZERO,
            right_shoulder,
            right_shoulder - right_up * 0.12,
            right_shoulder - right_up * 0.3,
            left_shoulder,
            left_shoulder - left_up * 0.12,
            left_shoulder - left_up * left_length,
            Vec3::new(-0.08, 0., 0.5),
            Vec3::new(-0.08, 0., 0.25),
            Vec3::new(-0.08, 0., 0.05),
            left_hip,
            left_knee,
            left_ankle,
            Vec3::new(0., 0., 0.9),
            Vec3::new(0., 0., 0.95),
        ];
        let mut poses: Vec<_> = heads
            .iter()
            .map(|&t| Affine {
                m: Mat3::IDENTITY,
                t,
            })
            .collect();
        for (i, up) in [(1, right_up), (4, left_up)] {
            let across = Vec3::new(0., 1., 0.);
            poses[i].m = Mat3::from_cols(across, -up, across.cross(-up));
        }
        let mut vertices = vec![
            Vec3::new(-0.1, -0.05, 0.5),
            Vec3::new(0.1, -0.05, 0.5),
            Vec3::new(0.1, 0.05, 0.5),
            Vec3::new(-0.1, 0.05, 0.5),
            Vec3::new(0., 0., 0.4),
            Vec3::ZERO,
            Vec3::new(0., 0., 1.),
        ];
        let mut owners = vec![0; vertices.len()];
        owners[5] = 9; // The floor belongs to the foot, not the torso's crotch section.
        for (bone, origin, up, radius) in [
            (1, right_shoulder, right_up, 0.02),
            (4, left_shoulder, left_up, left_radius),
        ] {
            let front = Vec3::new(0., -1., 0.);
            let lat = up.cross(front);
            for offset in [front, -front, lat, -lat] {
                vertices.push(origin - up * 0.01 + offset * radius);
                owners.push(bone);
            }
        }
        let left_leg_up = (left_hip - left_ankle).normalized();
        for (bone, origin, up, radius) in [
            (7, Vec3::new(-0.08, 0., 0.425), Vec3::new(0., 0., 1.), 0.025),
            (
                10,
                Vec3::new(left_hip.x, 0., 0.425),
                left_leg_up,
                left_leg_radius,
            ),
        ] {
            for k in 0..=40 {
                for sign in [-1., 1.] {
                    vertices.push(
                        origin + up * (-0.4 + k as f64 * 0.01) + Vec3::new(0., sign * radius, 0.),
                    );
                    owners.push(bone);
                }
            }
        }
        let n = vertices.len();
        let j = labels.len();
        let model = AnnyModel::from_data(AnnyData {
            header: serde_json::from_value(json!({
                "schema":"ftxbody/1","model":"anny","anny_version":"0.6.1",
                "configuration":"synthetic independent limb regions","licence":"synthetic","package_sha256":{},
                "counts":{"vertices":n,"faces":0,"bones":j,"blendshapes":0,"phenotype_blendshapes":0,"local_changes":0,"influences":1},
                "phenotype":{"features":[],"components":[],"anchors":{},"exposed":[],"fixed_value":0.5},
                "labels":{"blendshapes":[],"bones":labels,"bone_parents":parents,"local_changes":[]},
                "y_axis":[0,1,0],"degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],"waist_loop_final":[0,1,2,3],"arrays":{}
            })).unwrap(),
            template_vertices: vertices.clone(), faces: vec![], blendshape_row_ptr: vec![0],
            blendshape_vertex: vec![], blendshape_delta: vec![], phenotype_mask: vec![],
            template_bone_heads: heads.clone(), template_bone_tails: heads,
            bone_heads_blendshapes: vec![], bone_tails_blendshapes: vec![], bone_rolls: vec![Mat3::IDENTITY; j],
            vertex_bone_indices: owners, vertex_bone_weights: vec![1.; n],
            base_mesh_vertex_indices: (0..n as u32).collect(),
        }).unwrap();
        (
            model,
            PosedBody {
                vertices,
                bone_poses: poses,
                transforms: vec![Affine::IDENTITY; j],
            },
        )
    }

    #[test]
    fn anny_physical_panels_use_actual_asymmetric_left_joints_and_skin() {
        let (model, posed) = posed_limb_model(true);
        let frames = BodyFrames::new(&model, &posed).unwrap();
        let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-10, "{a} != {b}");
        near(frames.section_radius("arm_R", 0.), 23.);
        near(frames.section_radius("arm_L", 0.), 40.25);
        near(frames.section_radius("leg_R", -200.), 25.);
        near(frames.section_radius("leg_L", -200.), 45.);
        near(frames.landmark_h("arm_R", "shoulder_tip").unwrap(), 16.);
        near(frames.landmark_h("arm_L", "shoulder_tip").unwrap(), 28.);
        near(frames.landmark_h("arm_R", "wrist").unwrap(), -300.);
        near(frames.landmark_h("arm_L", "wrist").unwrap(), -400.);
        let left_up = Vec3::new(-0.12, 0., 0.48).normalized();
        for (name, z) in [
            ("ankle", 60.),
            ("calf", 210.),
            ("knee", 310.),
            ("waist", 500.),
        ] {
            near(
                frames.landmark_h("leg_L", name).unwrap(),
                (z - 425.) / left_up.z,
            );
        }
        let mut pieces = Vec::new();
        for (part, anchor) in [("arm", "shoulder_tip"), ("leg", "knee")] {
            for side in ["R", "L"] {
                let mut piece = physical_panel(&format!("{part}_{side}"), part, Some(side));
                piece["placement"]["y0_landmark"] = json!(anchor);
                piece["placement"]["y0"] = json!(0.);
                piece["placement"]["x0"] = json!(13.);
                piece["placement"]["theta_deg"] = json!(0.);
                pieces.push(piece);
            }
        }
        let pattern = Pattern::new(
            json!({"schema":"fabric-pattern/1","units":"mm","pieces":pieces,"seams":[]}),
        );
        let mesh = build_garment(&pattern, 12.).unwrap();
        let places = placements(&pattern, &mesh, &frames).unwrap();
        let wrapped = wrapped_positions(&mesh, &places);
        let expected = [
            Vec3::new(-191.6, -30., 811.2),
            Vec3::new(219.2, -22.25, 865.6),
            Vec3::new(-80., -32., 248.),
            Vec3::new(138.75 - 2. * left_up.x, -52., 310. - 2. * left_up.z),
        ];
        for (k, id) in ["arm_R", "arm_L", "leg_R", "leg_L"].into_iter().enumerate() {
            let p = &places[k];
            let index = mesh.corner[&(id.into(), "a".into())];
            assert!(
                (wrapped[index] - expected[k] * MM).norm() < 1e-12,
                "{id}: {:?}",
                wrapped[index]
            );
            assert_eq!(p.frame, PartFrames::frame(&frames, id).unwrap());
            assert!(!p.mirrored);
            assert!((p.frame.up.cross(p.frame.front) - p.frame.lat).norm() < 1e-14);
            // The native roll uses the identical physical anchor throughout partial folding.
            for progress in [0., 0.4, 1.] {
                let rolled = roll_points(&[Vec2::new(13., -2.)], p, progress);
                assert!((rolled[0] - expected[k] * MM).norm() < 1e-12);
            }
        }
    }

    #[test]
    fn anny_symmetric_left_and_generated_mirror_pairs_keep_their_conventions() {
        let reflect = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        for asymmetric in [false, true] {
            let (model, posed) = posed_limb_model(asymmetric);
            let frames = BodyFrames::new(&model, &posed).unwrap();
            let right = PartFrames::frame(&frames, "arm_R").unwrap();
            let left = PartFrames::frame(&frames, "arm_L").unwrap();
            if !asymmetric {
                assert_eq!(left.origin, reflect(right.origin));
                assert_eq!(left.up, reflect(right.up));
                assert_eq!(left.front, reflect(right.front));
                assert_eq!(left.lat, -reflect(right.lat));
                assert_eq!(left.lat.cross(left.up), left.front);
                for name in ["shoulder_tip", "elbow", "wrist"] {
                    assert_eq!(
                        frames.landmark_h("arm_L", name),
                        frames.landmark_h("arm_R", name)
                    );
                }
            } else {
                assert_ne!(left.origin, reflect(right.origin));
            }
            let mut piece = physical_panel("pair", "arm", None);
            piece["cut"]["qty"] = json!(2);
            piece["placement"]["y0_landmark"] = json!("shoulder_tip");
            let pattern = Pattern::new(
                json!({"schema":"fabric-pattern/1","units":"mm","pieces":[piece],"seams":[]}),
            );
            let mesh = build_garment(&pattern, 12.).unwrap();
            let places = placements(&pattern, &mesh, &frames).unwrap();
            assert_eq!(places[0].frame, right);
            assert_eq!(places[1].frame, right);
            assert!(!places[0].mirrored && places[1].mirrored);
            let r = wrap_points(&[Vec2::new(23., 7.)], &places[0])[0];
            let l = wrap_points(&[Vec2::new(-23., 7.)], &places[1])[0];
            assert_eq!(l, reflect(r));
        }
    }

    /// A body's frames with its limbs declared independently posed.
    struct IndependentLimbs<'a>(&'a BodyFrames);

    impl PartFrames for IndependentLimbs<'_> {
        fn frame(&self, part: &str) -> Option<PartFrame> {
            PartFrames::frame(self.0, part)
        }
        fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
            PartFrames::landmark_h(self.0, part, name)
        }
        fn section_radius(&self, part: &str, h: f64) -> f64 {
            PartFrames::section_radius(self.0, part, h)
        }
        fn independent_limbs(&self) -> bool {
            true
        }
    }

    #[test]
    fn mirrored_limb_instances_wrap_their_own_limb_when_limbs_are_posed_independently() {
        let mut piece = physical_panel("pair", "arm", None);
        piece["cut"]["qty"] = json!(2);
        piece["placement"]["y0_landmark"] = json!("shoulder_tip");
        let pattern = Pattern::new(
            json!({"schema":"fabric-pattern/1","units":"mm","pieces":[piece],"seams":[]}),
        );
        let mesh = build_garment(&pattern, 12.).unwrap();
        let (lo, hi) = mesh.inst_range[1];
        // Each wrapped point of the mirrored instance (mm) and its distance from a limb's axis.
        let off_axis = |place: &WrapPlacement, limb: PartFrame| {
            wrap_points(&mesh.verts2d[lo..hi], place)
                .into_iter()
                .map(|p| {
                    let d = p / MM - limb.origin;
                    (d - limb.up * d.dot(limb.up)).norm()
                })
                .collect::<Vec<_>>()
        };
        for asymmetric in [false, true] {
            let (model, posed) = posed_limb_model(asymmetric);
            let frames = BodyFrames::new(&model, &posed).unwrap();
            let left = PartFrames::frame(&frames, "arm_L").unwrap();
            let reference = placements(&pattern, &mesh, &frames).unwrap();
            let own = placements(&pattern, &mesh, &IndependentLimbs(&frames)).unwrap();
            // The right instance is placed as before; the mirrored one stays a mirrored instance.
            assert_eq!(own[0].frame, reference[0].frame);
            assert!(own[1].mirrored && !own[1].own_side);
            if !asymmetric {
                // A symmetric pose: the same placement, bit for bit.
                assert_eq!(own[1].frame, reference[1].frame);
                assert_eq!(
                    wrapped_positions(&mesh, &own),
                    wrapped_positions(&mesh, &reference)
                );
            } else {
                // The left sleeve lies on its wrap cylinder round the left arm itself...
                for d in off_axis(&own[1], left) {
                    assert!(
                        (d - own[1].radius).abs() < 1e-9 * own[1].radius,
                        "{d} vs {}",
                        own[1].radius
                    );
                }
                // ...where the reference puts it round the reflection of the right arm.
                assert!(
                    off_axis(&reference[1], left)
                        .iter()
                        .any(|d| (d - reference[1].radius).abs() > 1.)
                );
            }
        }
    }

    #[test]
    fn anny_missing_left_region_refuses_instead_of_borrowing_the_right() {
        let (model, posed) = posed_limb_model(true);
        let mut data = model.data().clone();
        for bone in &mut data.vertex_bone_indices {
            if *bone == 4 {
                *bone = 1;
            }
        }
        let model = ftx_body::AnnyModel::from_data(data).unwrap();
        let error = BodyFrames::new(&model, &posed).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("left upper-arm"));
    }

    #[test]
    fn physical_legs_use_the_selected_side_at_the_completed_wrap() {
        let mut right = physical_panel("right", "leg", Some("R"));
        let mut left = physical_panel("left", "leg", Some("L"));
        for (piece, angle) in [(&mut right, 90.), (&mut left, -90.)] {
            piece["placement"]["radius_mm"] = json!(30.);
            piece["placement"]["theta_deg"] = json!(angle);
            piece["placement"]["x0"] = json!(13.);
        }
        let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[right,left],"seams":[]}));
        let mesh = build_garment(&pattern, 12.).unwrap();
        let places = placements(&pattern, &mesh, &AsymmetricFrames).unwrap();
        for (k, p) in places.iter().enumerate() {
            assert!(p.own_side && !p.mirrored);
            assert_eq!(p.own_side_left, k == 1);
            let (lo, hi) = mesh.inst_range[k];
            let wrapped = wrap_points(&mesh.verts2d[lo..hi], p);
            assert!(
                wrapped
                    .iter()
                    .all(|q| if k == 0 { q.x <= -MM } else { q.x >= MM })
            );
            let anchor = [Vec2::new(p.x0, 0.)];
            let flat = roll_points(&anchor, p, 0.)[0];
            assert!(if k == 0 { flat.x > 0. } else { flat.x < 0. });
            let completed = roll_points(&anchor, p, 1.)[0];
            assert_eq!(completed.x, if k == 0 { -MM } else { MM });
        }
    }

    #[test]
    fn absent_physical_side_preserves_pair_fold_and_single_placement() {
        let mut pair = physical_panel("pair", "leg", None);
        pair["cut"]["qty"] = json!(2);
        let mut fold = physical_panel("fold", "torso", None);
        fold["cut"]["on_fold"] = json!("e3");
        let single = physical_panel("single", "arm", None);
        let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[pair,fold,single],"seams":[]}));
        let mesh = build_garment(&pattern, 12.).unwrap();
        let places = placements(&pattern, &mesh, &DummyFrames).unwrap();
        assert_eq!(mesh.instances, ["pair.R", "pair.L", "fold", "single"]);
        for (k, p) in places.iter().enumerate() {
            assert_eq!(p.mirrored, k == 1);
            assert_eq!(p.own_side, k < 2);
            assert!(!p.own_side_left);
            assert_eq!(p.x0, 17.);
            assert_eq!(p.h0, 495.);
            assert_eq!(p.radius, 87.);
        }
        let q = [Vec2::new(23., 7.)];
        let q_mirror = [Vec2::new(-23., 7.)];
        let right = wrap_points(&q, &places[0])[0];
        let left = wrap_points(&q_mirror, &places[1])[0];
        assert_eq!(left, Vec3::new(-right.x, right.y, right.z));
    }

    #[test]
    fn material_axes_scale_then_mirror_with_each_instance() {
        let pattern = Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm",
            "pieces":[{"id":"p","cut":{"qty":2},"sim_scale":[2,0.5],
                "grain":{"p0":[0,0],"p1":[3,4]},
                "vertices":{"a":[0,0],"b":[40,0],"c":[40,20],"d":[0,20]},
                "edges":[{"id":"e0","from":"a","to":"b"},{"id":"e1","from":"b","to":"c"},
                    {"id":"e2","from":"c","to":"d"},{"id":"e3","from":"d","to":"a"}],
                "placement":{"part":"torso"}}],"seams":[]}));
        let mesh = build_garment(&pattern, 15.).unwrap();
        let rest = grain_rest(&pattern, &mesh, true).unwrap();
        assert_eq!(mesh.instances.len(), 2);
        for (k, id) in mesh.instances.iter().enumerate() {
            let mirrored = pattern.instance(id).unwrap().mirrored;
            let direction = Vec2::new(if mirrored { -6. } else { 6. }, 2.).normalized();
            let (lo, hi) = mesh.inst_range[k];
            for (p, r) in mesh.verts2d[lo..hi].iter().zip(&rest[lo..hi]) {
                let expected = Vec2::new(direction.dot(*p), direction.cross(*p)) * 1e-3;
                assert!((*r - expected).norm() < 1e-15);
            }
        }
    }

    struct DummyFrames;
    impl PartFrames for DummyFrames {
        fn frame(&self, _: &str) -> Option<PartFrame> {
            Some(PartFrame {
                origin: Vec3::ZERO,
                up: Vec3::new(0.0, 0.0, 1.0),
                front: Vec3::new(0.0, -1.0, 0.0),
                lat: Vec3::new(1.0, 0.0, 0.0),
            })
        }
        fn landmark_h(&self, _: &str, _: &str) -> Option<f64> {
            Some(500.0)
        }
        fn section_radius(&self, _: &str, _: f64) -> f64 {
            80.0
        }
    }

    #[test]
    fn inferred_placement_clearance_and_radius_keys_are_honoured() {
        let pattern = Pattern::new(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "pieces": [
                {
                    "id": "bodice",
                    "cut": {"qty": 1},
                    "vertices": {"a": [0, 0], "b": [50, 0], "c": [50, 50], "d": [0, 50]},
                    "edges": [
                        {"id": "e0", "from": "a", "to": "b"},
                        {"id": "e1", "from": "b", "to": "c"},
                        {"id": "e2", "from": "c", "to": "d"},
                        {"id": "e3", "from": "d", "to": "a"}
                    ],
                    "placement": {"part": "torso", "clearance": 20.0, "inferred": true}
                },
                {
                    "id": "sleeve",
                    "cut": {"qty": 1},
                    "vertices": {"a": [0, 0], "b": [50, 0], "c": [50, 50], "d": [0, 50]},
                    "edges": [
                        {"id": "e0", "from": "a", "to": "b"},
                        {"id": "e1", "from": "b", "to": "c"},
                        {"id": "e2", "from": "c", "to": "d"},
                        {"id": "e3", "from": "d", "to": "a"}
                    ],
                    "placement": {"part": "arm", "y0_landmark": "shoulder_tip", "radius": 38.95, "inferred": true}
                }
            ],
            "seams": []
        }));
        let mesh = build_garment(&pattern, 25.0).unwrap();
        let pl = placements(&pattern, &mesh, &DummyFrames).unwrap();
        assert_eq!(pl.len(), 2);
        assert!((pl[0].radius - 100.0).abs() < 1e-12);
        assert!((pl[1].radius - 38.95).abs() < 1e-12);
    }

    #[test]
    fn elastic_ring_refuses_zero_perimeter_degenerate_ring_instead_of_nan() {
        let mesh = GarmentMesh {
            instances: vec!["band".into()],
            verts2d: vec![Vec2::ZERO; 3],
            faces: Vec::new(),
            owner: vec![0, 0, 0],
            inst_range: vec![(0, 3)],
            pairs: Vec::new(),
            seams: Vec::new(),
            corner: BTreeMap::new(),
            index: BTreeMap::new(),
            creases: Vec::new(),
        };
        let weld = vec![0, 1, 2];
        let casing = ElasticCasing {
            instance: "band".into(),
            length: 0.4,
            ea: ELASTIC_EA,
            material: None,
            load_instances: vec!["band".into()],
        };
        let path = vec![0, 1, 2, 0];
        let err = elastic_ring(&mesh, &weld, &casing, "top edge", &[&path]).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("zero or non-finite perimeter"));
    }

    #[test]
    fn max_speed_or_nan_propagates_nan_instead_of_reporting_zero() {
        assert_eq!(max_speed_or_nan(&[]), 0.0);
        assert!((max_speed_or_nan(&[Vec3::new(3.0, 4.0, 0.0)]) - 5.0).abs() < 1e-12);
        assert!(
            max_speed_or_nan(&[Vec3::new(1.0, 0.0, 0.0), Vec3::new(f64::NAN, 0.0, 0.0)]).is_nan()
        );
        assert!(max_speed_or_nan(&[Vec3::new(f64::INFINITY, 0.0, 0.0)]).is_infinite());
    }

    fn two_panel_cylinder_scene_inputs() -> (Pattern, Vec<Vec3>, Vec<[usize; 3]>) {
        // Two 100 mm x 100 mm torso panels sewn together along their right/left side seams into a
        // loose open-ended tube around a small 40 mm cylinder at z in [0.40, 0.70].
        let pattern = Pattern::new(json!({
            "schema": "fabric-pattern/1",
            "units": "mm",
            "pieces": [
                {
                    "id": "front",
                    "cut": {"qty": 1},
                    "vertices": {"a": [-50, 0], "b": [50, 0], "c": [50, 100], "d": [-50, 100]},
                    "edges": [
                        {"id": "bottom", "from": "a", "to": "b"},
                        {"id": "side_r", "from": "b", "to": "c"},
                        {"id": "top", "from": "c", "to": "d"},
                        {"id": "side_l", "from": "d", "to": "a"}
                    ],
                    "placement": {"part": "torso", "radius_mm": 80.0, "theta_deg": 0.0}
                },
                {
                    "id": "back",
                    "cut": {"qty": 1},
                    "vertices": {"a": [-50, 0], "b": [50, 0], "c": [50, 100], "d": [-50, 100]},
                    "edges": [
                        {"id": "bottom", "from": "a", "to": "b"},
                        {"id": "side_r", "from": "b", "to": "c"},
                        {"id": "top", "from": "c", "to": "d"},
                        {"id": "side_l", "from": "d", "to": "a"}
                    ],
                    "placement": {"part": "torso", "radius_mm": 80.0, "theta_deg": 180.0}
                }
            ],
            "seams": [
                {"a": "front:side_r", "b": "back:side_l", "reverse": true}
            ]
        }));
        // Small open cylinder of radius 0.04 m centered on the z axis, well inside the 0.08 m wrap.
        let n = 16usize;
        let (r, z0, z1) = (0.04, 0.35, 0.75);
        let mut verts = Vec::with_capacity(2 * n);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let (s, c) = det::sin_cos(a);
            verts.push(Vec3::new(r * c, r * s, z0));
            verts.push(Vec3::new(r * c, r * s, z1));
        }
        let mut tris = Vec::with_capacity(2 * n);
        for i in 0..n {
            let j = (i + 1) % n;
            tris.push([2 * i, 2 * j, 2 * j + 1]);
            tris.push([2 * i, 2 * j + 1, 2 * i + 1]);
        }
        (pattern, verts, tris)
    }

    #[test]
    fn scene_build_options_construction_and_self_repulsion_preserve_legacy_and_apply_cleanly() {
        let (pattern, skin_v, skin_t) = two_panel_cylinder_scene_inputs();
        let mesh = build_garment(&pattern, 20.0).unwrap();
        let fabric = Fabric::default();
        let contact = ContactParams {
            dhat: 2e-3,
            thickness: 5e-4,
            kappa: 100.0,
            mu: 0.3,
            eps_v: 1e-3,
        };

        let legacy = build_scene(
            &pattern,
            mesh.clone(),
            &DummyFrames,
            &skin_v,
            &skin_t,
            fabric,
            0.05,
            contact,
            None,
        )
        .unwrap();
        let default_opt = build_scene_with_options(
            &pattern,
            mesh.clone(),
            &DummyFrames,
            &skin_v,
            &skin_t,
            fabric,
            0.05,
            contact,
            None,
            SceneBuildOptions::default(),
        )
        .unwrap();
        assert_eq!(legacy.solver.x, default_opt.solver.x);
        assert_eq!(legacy.solver.model.mass, default_opt.solver.model.mass);
        assert_eq!(
            (0..legacy.solver.model.hinges.len())
                .map(|h| legacy.solver.model.hinge_rigidity(h))
                .collect::<Vec<_>>(),
            (0..default_opt.solver.model.hinges.len())
                .map(|h| default_opt.solver.model.hinge_rigidity(h))
                .collect::<Vec<_>>()
        );

        let constructed = build_scene_with_options(
            &pattern,
            mesh.clone(),
            &DummyFrames,
            &skin_v,
            &skin_t,
            fabric,
            0.05,
            contact,
            None,
            SceneBuildOptions {
                construction: ConstructionMechanicsOptions::standard(),
                start_self_repulsion: Some(SelfRepulsionOptions::default()),
            },
        )
        .unwrap();
        let m_legacy: f64 = legacy.solver.model.mass.iter().sum();
        let m_constructed: f64 = constructed.solver.model.mass.iter().sum();
        assert!(
            m_constructed > 1.2 * m_legacy,
            "folded hems and seam allowances must increase total cloth mass: legacy={m_legacy} constructed={m_constructed}"
        );
        let max_rig_legacy = (0..legacy.solver.model.hinges.len())
            .map(|h| legacy.solver.model.hinge_rigidity(h))
            .fold(0.0, f64::max);
        let max_rig_constructed = (0..constructed.solver.model.hinges.len())
            .map(|h| constructed.solver.model.hinge_rigidity(h))
            .fold(0.0, f64::max);
        assert!(
            (max_rig_constructed - 5.0 * max_rig_legacy).abs() <= 1e-12 * max_rig_legacy,
            "hem band hinges must carry 5x base bending rigidity: legacy={max_rig_legacy} constructed={max_rig_constructed}"
        );

        // Invalid SceneBuildOptions refuse fail-closed.
        let bad_opts = SceneBuildOptions {
            construction: ConstructionMechanicsOptions {
                hem_mass_multiplier: 2.0,
                hem_width_mm: 0.0,
                ..ConstructionMechanicsOptions::default()
            },
            start_self_repulsion: None,
        };
        assert_eq!(
            build_scene_with_options(
                &pattern,
                mesh,
                &DummyFrames,
                &skin_v,
                &skin_t,
                fabric,
                0.05,
                contact,
                None,
                bad_opts,
            )
            .err()
            .expect("invalid construction options must refuse")
            .code,
            RefusalCode::OutOfRange
        );
    }
}

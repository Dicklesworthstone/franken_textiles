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
use crate::garment::GarmentMesh;
use crate::nearest::NearestTree;
use crate::start::{
    GATHER_COMPRESSION, MM, PartFrame, StartState, WrapPlacement, start_state, wrap_points,
};

pub use crate::material::Fabric;

/// Body frames, landmarks and section radii in the form the wrap needs.
pub trait PartFrames {
    /// The frame of `part` (`torso`, `arm_R`, `leg_R`, or explicitly selected `arm_L`/`leg_L`).
    fn frame(&self, part: &str) -> Option<PartFrame>;
    /// A landmark's height coordinate in a part frame (mm).
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64>;
    /// Largest distance from the part axis to the body near `h` (mm).
    fn section_radius(&self, part: &str, h: f64) -> f64;
}

fn right_part(part: &str) -> &str {
    match part {
        "arm_L" => "arm_R",
        "leg_L" => "leg_R",
        _ => part,
    }
}

fn symmetric_left_frame(frame: PartFrame) -> PartFrame {
    let reflect = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    let up = reflect(frame.up);
    let front = reflect(frame.front);
    PartFrame {
        origin: reflect(frame.origin),
        up,
        front,
        // Recompute handedness: reflecting the lateral vector alone turns public normals inward.
        lat: up.cross(front),
    }
}

// Anny's current placement authority supplies only right limb measurements. Explicit left panels
// use a symmetric initial-placement convention, not a claim about asymmetric anatomy. Other
// PartFrames providers can supply independently qualified left frames, landmarks and radii.
impl PartFrames for BodyFrames {
    fn frame(&self, part: &str) -> Option<PartFrame> {
        BodyFrames::frame(self, right_part(part)).map(|f| {
            let frame = PartFrame {
                origin: f.origin,
                up: f.up,
                front: f.front,
                lat: f.lat,
            };
            if matches!(part, "arm_L" | "leg_L") {
                symmetric_left_frame(frame)
            } else {
                frame
            }
        })
    }
    fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        BodyFrames::landmark_h(self, right_part(part), name)
    }
    fn section_radius(&self, part: &str, h: f64) -> f64 {
        BodyFrames::section_radius(self, right_part(part), h)
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
        let physical = physical_side(piece).map_err(|e| bad(format!("{id}: {}", e.message())))?;
        let key = if part == "torso" {
            "torso".to_string()
        } else {
            format!("{part}_{}", physical.unwrap_or(Side::R).as_str())
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
        let radius = if let Some(r) = pl
            .get("radius_mm")
            .or_else(|| pl.get("radius"))
            .and_then(Value::as_f64)
        {
            r
        } else {
            let (lo, hi) = mesh.inst_range[k];
            let ys = mesh.verts2d[lo..hi].iter().map(|p| h0 + p.y);
            let (hmin, hmax) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| {
                (a.min(h), b.max(h))
            });
            let clearance = pl
                .get("clearance_mm")
                .or_else(|| pl.get("clearance"))
                .and_then(Value::as_f64)
                .unwrap_or(25.0);
            linspace(hmin, hmax, 9)
                .into_iter()
                .map(|h| frames.section_radius(&key, h))
                .fold(0.0, f64::max)
                + clearance
        };
        out.push(WrapPlacement {
            frame,
            h0,
            radius,
            theta_deg: num("theta_deg", 0.0),
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
    /// The cloth offset the contact ramps up to (m).
    pub target_thickness: f64,
    /// The solver (positions are the welded vertices).
    pub solver: Solver,
    /// The casing elastic's rings (fold, seam line) as ranges of the model's `elastics`; empty without one.
    pub elastic_rings: Vec<std::ops::Range<usize>>,
    /// Independent casing ownership and load domains; ring ranges index `elastic_rings`.
    pub elastic_groups: Vec<ElasticGroup>,
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
    let scale = casing.length / segments.iter().map(|s| s.2).sum::<f64>();
    Ok(segments
        .into_iter()
        .map(|(a, b, l)| ElasticSegment {
            v: [a, b],
            rest: l * scale,
            ea: casing.ea / 2.0,
        })
        .collect())
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
    )
}

pub(crate) fn build_scene_resolved(
    pattern: &Pattern,
    mesh: GarmentMesh,
    frames: &dyn PartFrames,
    skin_vertices: &[Vec3],
    skin_triangles: &[[usize; 3]],
    material: &crate::material::MaterialInputs,
    elastics: &[ElasticCasing],
) -> Result<Scene, Refusal> {
    let mut scene = build_scene_inner(
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
    )?;
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
    piece_fabrics: Option<&[Fabric]>,
    response: Option<&ftx_pattern::material_response::CompiledMembrane>,
    piece_responses: Option<&[Option<ftx_pattern::material_response::CompiledMembrane>]>,
    contact_materials: Option<&crate::material::ContactMaterials>,
) -> Result<Scene, Refusal> {
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
                        .ok_or_else(|| bad(format!("no material for instance {id}")))
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
                for &v in &element.v {
                    loaded_mass[v] += density * element.area / 3.0;
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
                field.piece_ids[piece]
            })
            .collect();
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
        target_thickness: contact.thickness,
        solver,
        elastic_rings,
        elastic_groups,
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

/// One accepted step; the Newton residual is in N, correction in m and potential in J.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseStep {
    /// Newton diagnostics for this accepted implicit step.
    pub solve: SolveReport,
    /// Largest instantaneous vertex speed after the step (m/s), never a window-mean drift.
    pub max_speed_m_s: f64,
    /// Accepted endpoint contact state, absent if contact is disabled.
    pub contact: Option<crate::contact::ContactSample>,
}

/// One phase's per-step reports.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PhaseReport {
    /// Steps taken.
    pub steps: usize,
    /// Newton iterations, summed.
    pub iterations: usize,
    /// Steps that did not converge.
    pub unconverged: usize,
    /// Sparse factorizations, summed.
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
    /// Accepted steps in execution order. A failed step produces no sample or animation frame.
    pub samples: Vec<PhaseStep>,
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
    mut frame: impl FnMut(usize, &[Vec3]),
) -> Result<PhaseReport, Refusal> {
    scene.solver.gravity = Vec3::new(0.0, 0.0, -gravity);
    let mut rep = PhaseReport::default();
    for k in 0..steps {
        let thickness = scene.solver.contact.as_ref().map(|c| c.params.thickness);
        ramp_thickness(scene);
        let r: SolveReport = match scene.solver.step(h) {
            Ok(r) => r,
            Err(e) => {
                if let (Some(c), Some(thickness)) = (&mut scene.solver.contact, thickness) {
                    c.params.thickness = thickness;
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
        rep.samples.push(PhaseStep {
            solve: r,
            max_speed_m_s: scene.solver.v.iter().map(|v| v.norm()).fold(0.0, f64::max),
            contact: scene
                .solver
                .contact
                .as_ref()
                .map(|c| c.accepted_sample(&scene.solver.x)),
        });
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

#[cfg(test)]
mod grain_tests {
    use super::*;
    use crate::garment::build_garment;
    use crate::start::roll_points;
    use serde_json::json;

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

    #[test]
    fn symmetric_left_frame_retains_public_side_handedness() {
        let up = Vec3::new(0.6, 0., 0.8);
        let front = Vec3::new(0., -1., 0.);
        let right = PartFrame {
            origin: Vec3::new(-200., 12., 400.),
            up,
            front,
            lat: up.cross(front),
        };
        let left = symmetric_left_frame(right);
        assert_eq!(left.origin, Vec3::new(200., 12., 400.));
        assert_eq!(left.up, Vec3::new(-0.6, 0., 0.8));
        assert_eq!(left.front, front);
        assert_eq!(left.lat, Vec3::new(0.8, 0., 0.6));
        assert_eq!(left.lat.cross(left.up), left.front);
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
}

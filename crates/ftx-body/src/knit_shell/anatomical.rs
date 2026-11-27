//! Source-defined anatomical openings on a connected skin. Cutting one selected contour, rather
//! than discarding an entire spatial half-space, preserves other limbs crossing the same plane.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::exact::{Sign, orient2d};
use ftx_core::intersection::validate_mesh;
use ftx_core::shaping::{
    BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface,
};
use ftx_core::{Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

use super::{
    MAX_SHELL_TRIANGLES, MAX_SHELL_VERTICES, audit_open_shell, validate_ease_profile,
    validate_vertex_links,
};
use crate::drape::skin_mesh;
use crate::model::{AnnyModel, PosedBody};

/// The source of an anatomical section. A plane's origin is an interior anatomical witness,
/// not merely a point somewhere on the infinite plane. An authored segmentation is a continuous
/// piecewise-linear scalar field on the input skin; its zero set is the requested opening.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnatomicalSection {
    /// A genuine landmark plane, selecting only the closed section containing `origin_m`.
    Plane {
        /// Interior joint/section point in metres.
        origin_m: [f64; 3],
        /// Nonzero plane normal; normalized on admission.
        normal: [f64; 3],
    },
    /// An anatomical source classification. Exactly one closed separating zero contour is required.
    Segmentation {
        /// One finite value per source vertex, with both signs present; no guessed fill values.
        signed_membership: Vec<f64>,
    },
}

/// A named opening and the locally adjacent side of it that belongs to the garment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnatomicalCut {
    /// Semantic opening name, unique within the shell.
    pub name: String,
    /// Hem/cuff start (`t=0`) versus neck/armhole end (`t=1`).
    pub is_start: bool,
    /// Keep the component adjacent to the positive side of the selected contour.
    pub keep_positive: bool,
    /// Actual landmark plane or source segmentation.
    pub section: AnatomicalSection,
}

/// Explicit anatomical input for callers that own qualified landmarks or segmentation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnatomicalShellSpec {
    /// Ordered cuts; the resulting named openings must remain disjoint.
    pub cuts: Vec<AnatomicalCut>,
    /// Requested signed normal displacement in metres at every source vertex. Values are
    /// interpolated conformingly when triangles are split; they are never silently clamped.
    pub radial_offsets_m: Vec<f64>,
    /// Lower bound for each nonnegative requested displacement, not closest-body clearance.
    pub min_clearance_m: f64,
    /// Explicit consent for any negative requested displacement.
    #[serde(default)]
    pub allow_negative_ease: bool,
}

/// Anatomical shaping from an actual posed Anny model, before collider trim caps are inserted.
///
/// Wrist planes use the independently posed elbow-to-wrist axes; the neck uses `neck01` to
/// `head`; the waist uses the plane of the model's authored ordered waist loop. Armholes use the
/// source rig's equal arm-subtree/non-arm influence boundary. That boundary is rig anatomy,
/// not an independently measured clothing armhole or evidence of physical fit.
#[derive(Clone, Debug)]
pub struct AnnyAnatomicalShell<'a> {
    /// Loaded source model, including weights, hierarchy and waist-loop provenance.
    pub model: &'a AnnyModel,
    /// Independently posed left and right skin and bone transforms.
    pub posed: &'a PosedBody,
    /// Canonical fitted-body/report identity supplied by the caller.
    pub source_body_sha256: String,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn geometry(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

impl ShapingSurface for AnnyAnatomicalShell<'_> {
    fn build_ease_shell(
        &self,
        region: KnitShellRegion,
        ease: &EaseProfile,
    ) -> Result<EaseShellMesh, Refusal> {
        validate_ease_profile(ease)?;
        if region == KnitShellRegion::Tube {
            return Err(
                invalid("an anatomical shell requires a named body region").repair(
                    "Use build_ease_shell_from_mesh for an explicitly supplied generic tube.",
                ),
            );
        }
        let data = self.model.data();
        let counts = &data.header.counts;
        let parents = &data.header.labels.bone_parents;
        let width = counts.vertices.checked_mul(counts.influences);
        if counts.influences == 0
            || self.posed.vertices.len() != counts.vertices
            || self.posed.bone_poses.len() != counts.bones
            || parents.len() != counts.bones
            || data.header.labels.bones.len() != counts.bones
            || parents
                .iter()
                .enumerate()
                .any(|(i, &p)| p < -1 || p >= i as i64)
            || width != Some(data.vertex_bone_indices.len())
            || width != Some(data.vertex_bone_weights.len())
            || data
                .vertex_bone_indices
                .iter()
                .any(|&i| i as usize >= counts.bones)
        {
            return Err(invalid(
                "posed geometry and rig tables do not match the anatomical source",
            ));
        }
        let head = |name: &str| -> Result<Vec3, Refusal> {
            let i = self.model.bone_index(name).ok_or_else(|| {
                invalid(format!("anatomical knit shell requires source bone {name}"))
            })?;
            let p = self.posed.bone_poses[i].t;
            if !p.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "non-finite anatomical joint",
                ));
            }
            Ok(p)
        };
        let roots = ["upperarm01.L", "upperarm01.R"].map(|name| {
            self.model.bone_index(name).ok_or_else(|| {
                invalid(format!("anatomical knit shell requires source bone {name}"))
            })
        });
        let [left, right] = roots;
        let roots = [left?, right?];
        let mut arm_bones = vec![[false; 2]; counts.bones];
        for (side, &root) in roots.iter().enumerate() {
            arm_bones[root][side] = true;
            for i in root + 1..counts.bones {
                if let Ok(parent) = usize::try_from(parents[i]) {
                    arm_bones[i][side] = arm_bones[parent][side];
                }
            }
        }
        if arm_bones.iter().any(|sides| sides[0] && sides[1]) {
            return Err(invalid(
                "left and right arm subtrees overlap in the anatomical rig",
            ));
        }
        let mut weights = Vec::with_capacity(counts.vertices);
        for v in 0..counts.vertices {
            let mut total = 0.0;
            let mut arms = [0.0; 2];
            for slot in 0..counts.influences {
                let index = v * counts.influences + slot;
                let w = data.vertex_bone_weights[index];
                if !w.is_finite() || w < 0.0 {
                    return Err(invalid(
                        "anatomical source skinning weights must be finite and nonnegative",
                    ));
                }
                total += w;
                let bone = data.vertex_bone_indices[index] as usize;
                for (side, influence) in arms.iter_mut().enumerate() {
                    if arm_bones[bone][side] {
                        *influence += w;
                    }
                }
            }
            if !total.is_finite() || total <= 0.0 {
                return Err(invalid(
                    "anatomical source vertex has no positive skinning influence",
                ));
            }
            weights.push(arms.map(|w| w / total));
        }
        // Extract the genuine largest closed skin, retaining its source vertex map. Collider
        // trim fans must never be substituted for an anatomical section of the original skin.
        let skin = skin_mesh(&self.posed.vertices, &data.faces)?;
        let mut cuts = Vec::new();
        let armhole = |side: usize, keep_positive: bool| AnatomicalCut {
            name: if side == 0 {
                "left_armhole"
            } else {
                "right_armhole"
            }
            .into(),
            is_start: false,
            keep_positive,
            section: AnatomicalSection::Segmentation {
                signed_membership: skin
                    .source_vertex
                    .iter()
                    .map(|&v| 2.0 * weights[v][side] - 1.0)
                    .collect(),
            },
        };
        let wrist = |side: usize| -> Result<AnatomicalCut, Refusal> {
            let suffix = if side == 0 { "L" } else { "R" };
            let wrist = head(&format!("wrist.{suffix}"))?;
            let elbow = head(&format!("lowerarm01.{suffix}"))?;
            Ok(AnatomicalCut {
                name: if side == 0 {
                    "left_wrist_cuff"
                } else {
                    "right_wrist_cuff"
                }
                .into(),
                is_start: true,
                keep_positive: false,
                section: AnatomicalSection::Plane {
                    origin_m: wrist.into(),
                    normal: (wrist - elbow).into(),
                },
            })
        };
        match region {
            KnitShellRegion::LeftSleeve | KnitShellRegion::RightSleeve => {
                let side = usize::from(region == KnitShellRegion::RightSleeve);
                cuts.push(armhole(side, true));
                cuts.push(wrist(side)?);
            }
            KnitShellRegion::Torso | KnitShellRegion::FullUpperBody => {
                let waist = &data.header.waist_loop_final;
                if waist.len() < 3 || waist.iter().any(|&v| v >= self.posed.vertices.len()) {
                    return Err(invalid(
                        "an anatomical torso requires the source model's waist loop",
                    ));
                }
                let origin = waist
                    .iter()
                    .fold(Vec3::ZERO, |sum, &v| sum + self.posed.vertices[v])
                    / waist.len() as f64;
                let neck = head("neck01")?;
                let mut waist_normal = waist.iter().zip(waist.iter().cycle().skip(1)).fold(
                    Vec3::ZERO,
                    |sum, (&a, &b)| {
                        sum + (self.posed.vertices[a] - origin)
                            .cross(self.posed.vertices[b] - origin)
                    },
                );
                let proximal = waist_normal.dot(neck - origin);
                if !proximal.is_finite() || proximal == 0.0 {
                    return Err(geometry(
                        "the source waist loop does not define a plane toward the neck",
                    ));
                }
                if proximal < 0.0 {
                    waist_normal = -waist_normal;
                }
                cuts.push(AnatomicalCut {
                    name: "waist_hem".into(),
                    is_start: true,
                    keep_positive: true,
                    section: AnatomicalSection::Plane {
                        origin_m: origin.into(),
                        normal: waist_normal.into(),
                    },
                });
                cuts.push(AnatomicalCut {
                    name: "neck".into(),
                    is_start: false,
                    keep_positive: false,
                    section: AnatomicalSection::Plane {
                        origin_m: neck.into(),
                        normal: (head("head")? - neck).into(),
                    },
                });
                if region == KnitShellRegion::FullUpperBody {
                    cuts.push(wrist(0)?);
                    cuts.push(wrist(1)?);
                } else {
                    cuts.push(armhole(0, false));
                    cuts.push(armhole(1, false));
                }
            }
            KnitShellRegion::Tube => unreachable!("tube refused above"),
        }
        let radial_offsets_m = skin
            .source_vertex
            .iter()
            .map(|&v| {
                let millimetres = match region {
                    KnitShellRegion::LeftSleeve | KnitShellRegion::RightSleeve => {
                        ease.limb_radial_mm
                    }
                    KnitShellRegion::FullUpperBody => {
                        let arm = weights[v][0] + weights[v][1];
                        (1.0 - arm) * ease.torso_radial_mm + arm * ease.limb_radial_mm
                    }
                    _ => ease.torso_radial_mm,
                };
                millimetres * 0.001
            })
            .collect();
        build_anatomical_ease_shell(
            &skin.vertices,
            &skin.triangles,
            &AnatomicalShellSpec {
                cuts,
                radial_offsets_m,
                min_clearance_m: ease.min_clearance_mm * 0.001,
                allow_negative_ease: ease.allow_negative_ease,
            },
            &self.source_body_sha256,
        )
    }
}

#[derive(Clone)]
struct Mesh {
    positions: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    fields: Vec<Vec<f64>>,
    offsets: Vec<f64>,
    boundaries: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Plane {
    origin: Vec3,
    normal: Vec3,
}

fn edge(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Builds a connected genus-zero anatomical shell with exactly the declared genuine openings.
///
/// The supplied mesh must be one outward closed skin; section contours are cut locally, so a
/// wrist plane may cross the torso or the other arm without removing either. Source segmentation
/// fields must identify one closed contour. The final mesh is audited before and after exact
/// requested normal ease; no cap faces, inferred anatomy or silently reduced ease are produced.
///
/// # Errors
/// Refuses malformed source data, ambiguous/nonseparating cuts, intersecting or missing named
/// openings, inward/nonmanifold geometry, unsupported handles and unsafe requested offsets.
pub fn build_anatomical_ease_shell(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    spec: &AnatomicalShellSpec,
    source_body_sha256: &str,
) -> Result<EaseShellMesh, Refusal> {
    if vertices.len() > MAX_SHELL_VERTICES || triangles.len() > MAX_SHELL_TRIANGLES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "anatomical source exceeds shell work bounds",
        ));
    }
    validate_mesh(vertices, triangles)?;
    if !(2..=8).contains(&spec.cuts.len()) || spec.radial_offsets_m.len() != vertices.len() {
        return Err(invalid(
            "an anatomical shell needs 2–8 cuts and one signed ease value per source vertex",
        ));
    }
    Refusal::finite("minimum shell displacement", spec.min_clearance_m)?;
    if spec.min_clearance_m < 0.0 {
        return Err(invalid("minimum shell displacement cannot be negative"));
    }
    let mut names = BTreeSet::new();
    let mut planes = Vec::new();
    let mut fields = Vec::new();
    for cut in &spec.cuts {
        if cut.name.trim().is_empty() || !names.insert(&cut.name) {
            return Err(invalid(
                "anatomical opening names must be nonempty and unique",
            ));
        }
        match &cut.section {
            AnatomicalSection::Plane { origin_m, normal } => {
                let origin = Vec3::from(*origin_m);
                let normal = Vec3::from(*normal);
                let length = normal.norm();
                if !origin.is_finite() || !length.is_finite() || length == 0.0 {
                    return Err(invalid(format!(
                        "{} has an invalid anatomical plane",
                        cut.name
                    )));
                }
                // validate_mesh bounds source coordinates for exact predicates; bound the witness
                // with the same admission instead of allowing an unbounded external query point.
                validate_mesh(&[origin], &[])?;
                let normal = normal / length;
                planes.push(Some(Plane { origin, normal }));
                fields.push(vertices.iter().map(|&p| (p - origin).dot(normal)).collect());
            }
            AnatomicalSection::Segmentation { signed_membership } => {
                if signed_membership.len() != vertices.len()
                    || signed_membership.iter().any(|v| !v.is_finite())
                {
                    return Err(invalid(format!(
                        "{} lacks finite source segmentation for every vertex",
                        cut.name
                    )));
                }
                planes.push(None);
                fields.push(signed_membership.clone());
            }
        }
    }
    if spec.cuts.iter().all(|c| c.is_start) || spec.cuts.iter().all(|c| !c.is_start) {
        return Err(invalid(
            "an anatomical shaping shell needs both start and end openings",
        ));
    }
    for &offset in &spec.radial_offsets_m {
        Refusal::finite("requested anatomical ease", offset)?;
        if offset < 0.0 && !spec.allow_negative_ease {
            return Err(invalid(
                "negative anatomical ease requires explicit consent",
            ));
        }
    }
    let mut mesh = Mesh {
        positions: vertices.to_vec(),
        triangles: triangles.to_vec(),
        fields,
        offsets: spec.radial_offsets_m.clone(),
        boundaries: vec![0; vertices.len()],
    };
    let source_topology = topology(&mesh.triangles)?;
    if !source_topology.boundary_next.is_empty() {
        return Err(geometry(
            "anatomical extraction requires original closed skin, before trim caps or holes",
        ));
    }
    let origin = mesh.positions[mesh.triangles[0][0]];
    let volume_six = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| mesh.positions[v] - origin);
            a.dot(b.cross(c))
        })
        .sum::<f64>();
    if !volume_six.is_finite() || volume_six <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::InwardWound,
            "anatomical source must enclose positive outward-wound volume",
        ));
    }
    for (index, cut) in spec.cuts.iter().enumerate() {
        apply_cut(&mut mesh, index, cut, planes[index])?;
    }
    compact(&mut mesh);
    finish(mesh, spec, &planes, source_body_sha256)
}

/// Face index and its oriented half-edge endpoints.
type EdgeIncidence = (usize, usize, usize);
type EdgeMap = BTreeMap<(usize, usize), Vec<EdgeIncidence>>;

struct Topology {
    edges: EdgeMap,
    adjacent: Vec<Vec<usize>>,
    boundary_next: BTreeMap<u32, u32>,
}

fn topology(triangles: &[[usize; 3]]) -> Result<Topology, Refusal> {
    if triangles.is_empty() {
        return Err(geometry("anatomical cut removed the whole skin"));
    }
    let mut edges: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (face, triangle) in triangles.iter().enumerate() {
        for i in 0..3 {
            let (a, b) = (triangle[i], triangle[(i + 1) % 3]);
            edges.entry(edge(a, b)).or_default().push((face, a, b));
        }
    }
    let mut adjacent = vec![Vec::new(); triangles.len()];
    let mut boundary_next = BTreeMap::new();
    let mut incoming = BTreeSet::new();
    for occurrences in edges.values() {
        match *occurrences.as_slice() {
            [(face_a, a, b), (face_b, c, d)] if a == d && b == c => {
                adjacent[face_a].push(face_b);
                adjacent[face_b].push(face_a);
            }
            [(_, a, b)] => {
                if boundary_next.insert(a as u32, b as u32).is_some() || !incoming.insert(b) {
                    return Err(geometry("anatomical mesh has a pinched boundary"));
                }
            }
            _ => {
                return Err(geometry(
                    "anatomical skin has inconsistent winding or nonmanifold edges",
                ));
            }
        }
    }
    Ok(Topology {
        edges,
        adjacent,
        boundary_next,
    })
}

fn split_polygon(
    mesh: &mut Mesh,
    triangle: &[usize; 3],
    cut: usize,
    positive: bool,
    intersections: &mut BTreeMap<(usize, usize), usize>,
    plane: Option<Plane>,
) -> Result<Vec<usize>, Refusal> {
    let mut polygon = Vec::new();
    let mut previous = triangle[2];
    for &current in triangle {
        let inside = |v: usize| {
            if positive {
                mesh.fields[cut][v] >= 0.0
            } else {
                mesh.fields[cut][v] <= 0.0
            }
        };
        let (previous_inside, current_inside) = (inside(previous), inside(current));
        if previous_inside != current_inside {
            let (a, b) = edge(previous, current);
            let vertex = if mesh.fields[cut][a] == 0.0 {
                a
            } else if mesh.fields[cut][b] == 0.0 {
                b
            } else if let Some(&v) = intersections.get(&(a, b)) {
                v
            } else {
                if mesh.positions.len() == MAX_SHELL_VERTICES {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "anatomical clipping exceeds vertex bound",
                    ));
                }
                let (va, vb) = (mesh.fields[cut][a], mesh.fields[cut][b]);
                // Scaling avoids overflow even for a large authored scalar field.
                let scale = va.abs().max(vb.abs());
                let t = (va / scale) / (va / scale - vb / scale);
                if !t.is_finite() || !(0.0..=1.0).contains(&t) {
                    return Err(geometry(
                        "anatomical section intersection is not representable",
                    ));
                }
                let mut point = mesh.positions[a] * (1.0 - t) + mesh.positions[b] * t;
                if let Some(plane) = plane {
                    point = point - plane.normal * (point - plane.origin).dot(plane.normal);
                }
                let v = mesh.positions.len();
                mesh.positions.push(point);
                for (field_index, field) in mesh.fields.iter_mut().enumerate() {
                    field.push(if field_index == cut {
                        0.0
                    } else {
                        (1.0 - t) * field[a] + t * field[b]
                    });
                }
                mesh.offsets
                    .push((1.0 - t) * mesh.offsets[a] + t * mesh.offsets[b]);
                mesh.boundaries
                    .push(mesh.boundaries[a] & mesh.boundaries[b]);
                intersections.insert((a, b), v);
                v
            };
            if polygon.last() != Some(&vertex) {
                polygon.push(vertex);
            }
        }
        if current_inside && polygon.last() != Some(&current) {
            polygon.push(current);
        }
        previous = current;
    }
    if polygon.first() == polygon.last() {
        polygon.pop();
    }
    Ok(polygon)
}

fn contains_witness(mesh: &Mesh, ring: &[usize], plane: Plane) -> Result<bool, Refusal> {
    let n = plane.normal;
    let drop_axis = if n.x.abs() >= n.y.abs() && n.x.abs() >= n.z.abs() {
        0
    } else if n.y.abs() >= n.z.abs() {
        1
    } else {
        2
    };
    let project = |p: Vec3| match drop_axis {
        0 => [p.y, p.z],
        1 => [p.x, p.z],
        _ => [p.x, p.y],
    };
    let witness = project(plane.origin);
    let mut winding = 0i32;
    for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let (a, b) = (project(mesh.positions[a]), project(mesh.positions[b]));
        let sign = orient2d(a, b, witness);
        if sign == Sign::Zero
            && witness[0] >= a[0].min(b[0])
            && witness[0] <= a[0].max(b[0])
            && witness[1] >= a[1].min(b[1])
            && witness[1] <= a[1].max(b[1])
        {
            return Err(geometry("anatomical witness lies on its section boundary"));
        }
        if a[1] <= witness[1] && b[1] > witness[1] && sign == Sign::Positive {
            winding += 1;
        } else if a[1] > witness[1] && b[1] <= witness[1] && sign == Sign::Negative {
            winding -= 1;
        }
    }
    Ok(winding != 0)
}

fn apply_cut(
    mesh: &mut Mesh,
    index: usize,
    cut: &AnatomicalCut,
    plane: Option<Plane>,
) -> Result<(), Refusal> {
    let mut intersections = BTreeMap::new();
    let mut split = Vec::new();
    let mut signs = Vec::new();
    for triangle in std::mem::take(&mut mesh.triangles) {
        if triangle.iter().all(|&v| mesh.fields[index][v] == 0.0) {
            return Err(geometry(format!(
                "{} contains an entire source face; its section is ambiguous",
                cut.name
            )));
        }
        for positive in [false, true] {
            let polygon =
                split_polygon(mesh, &triangle, index, positive, &mut intersections, plane)?;
            for j in 1..polygon.len().saturating_sub(1) {
                if split.len() == MAX_SHELL_TRIANGLES {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "anatomical clipping exceeds triangle bound",
                    ));
                }
                split.push([polygon[0], polygon[j], polygon[j + 1]]);
                signs.push(positive);
            }
        }
    }
    mesh.triangles = split;
    validate_mesh(&mesh.positions, &mesh.triangles)?;
    let graph = topology(&mesh.triangles)?;
    let mut section = BTreeMap::<usize, Vec<usize>>::new();
    for (&(a, b), occurrences) in &graph.edges {
        if let &[(fa, _, _), (fb, _, _)] = occurrences.as_slice()
            && mesh.fields[index][a] == 0.0
            && mesh.fields[index][b] == 0.0
            && signs[fa] != signs[fb]
        {
            section.entry(a).or_default().push(b);
            section.entry(b).or_default().push(a);
        }
    }
    let mut seen = BTreeSet::<usize>::new();
    let mut candidates = Vec::new();
    for &start in section.keys() {
        if seen.contains(&start) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(v) = stack.pop() {
            if component.insert(v) {
                stack.extend(&section[&v]);
            }
        }
        seen.extend(component.iter().copied());
        if component.iter().any(|v| section[v].len() != 2) {
            if plane.is_none() {
                return Err(geometry(format!(
                    "{} segmentation does not close as one simple ring",
                    cut.name
                )));
            }
            // A plane may cross an already open cuff on an unrelated limb. That chain is not
            // the closed anatomical section selected by the interior joint witness.
            continue;
        }
        let mut ring = vec![start];
        let mut previous = start;
        let mut current = section[&start][0];
        while current != start {
            ring.push(current);
            let neighbors = &section[&current];
            let next = if neighbors[0] == previous {
                neighbors[1]
            } else {
                neighbors[0]
            };
            previous = current;
            current = next;
        }
        if ring.len() >= 3
            && match plane {
                Some(plane) => contains_witness(mesh, &ring, plane)?,
                None => true,
            }
        {
            candidates.push(ring);
        }
    }
    if candidates.len() != 1 {
        return Err(geometry(format!(
            "{} identifies {} closed anatomical contours; exactly one is required",
            cut.name,
            candidates.len()
        ))
        .repair("Supply an interior source landmark or an unambiguous anatomical segmentation."));
    }
    let ring = candidates.pop().expect("one admitted contour");
    let cut_edges: BTreeSet<_> = ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&a, &b)| edge(a, b))
        .collect();
    let mut adjacent = vec![Vec::new(); mesh.triangles.len()];
    for (key, occurrences) in &graph.edges {
        if let &[(a, _, _), (b, _, _)] = occurrences.as_slice()
            && !cut_edges.contains(key)
        {
            adjacent[a].push(b);
            adjacent[b].push(a);
        }
    }
    let mut labels = vec![usize::MAX; mesh.triangles.len()];
    let mut count = 0;
    for face in 0..mesh.triangles.len() {
        if labels[face] != usize::MAX {
            continue;
        }
        labels[face] = count;
        let mut pending = vec![face];
        while let Some(f) = pending.pop() {
            for &other in &adjacent[f] {
                if labels[other] == usize::MAX {
                    labels[other] = count;
                    pending.push(other);
                }
            }
        }
        count += 1;
    }
    if count != 2 {
        return Err(geometry(format!(
            "{} does not separate one connected skin into two components",
            cut.name
        )));
    }
    let mut kept = BTreeSet::new();
    let mut removed = BTreeSet::new();
    for key in &cut_edges {
        for &(face, _, _) in &graph.edges[key] {
            if signs[face] == cut.keep_positive {
                kept.insert(labels[face]);
            } else {
                removed.insert(labels[face]);
            }
        }
    }
    if kept.len() != 1 || removed.len() != 1 || kept == removed {
        return Err(geometry(format!(
            "{} has inconsistent anatomical inside/outside adjacency",
            cut.name
        )));
    }
    let keep = *kept.first().expect("one retained component");
    mesh.triangles = mesh
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(i, &t)| (labels[i] == keep).then_some(t))
        .collect();
    for vertex in ring {
        mesh.boundaries[vertex] |= 1 << index;
    }
    Ok(())
}

fn compact(mesh: &mut Mesh) {
    let used: BTreeSet<_> = mesh.triangles.iter().flatten().copied().collect();
    let remap: BTreeMap<_, _> = used
        .iter()
        .enumerate()
        .map(|(to, &from)| (from, to))
        .collect();
    mesh.positions = used.iter().map(|&v| mesh.positions[v]).collect();
    mesh.offsets = used.iter().map(|&v| mesh.offsets[v]).collect();
    mesh.boundaries = used.iter().map(|&v| mesh.boundaries[v]).collect();
    for field in &mut mesh.fields {
        *field = used.iter().map(|&v| field[v]).collect();
    }
    for t in &mut mesh.triangles {
        *t = t.map(|v| remap[&v]);
    }
}

fn finish(
    mut mesh: Mesh,
    spec: &AnatomicalShellSpec,
    planes: &[Option<Plane>],
    source: &str,
) -> Result<EaseShellMesh, Refusal> {
    validate_mesh(&mesh.positions, &mesh.triangles)?;
    let graph = topology(&mesh.triangles)?;
    let mut reached = BTreeSet::new();
    let mut pending = vec![0];
    while let Some(face) = pending.pop() {
        if reached.insert(face) {
            pending.extend(&graph.adjacent[face]);
        }
    }
    if reached.len() != mesh.triangles.len() {
        return Err(geometry("anatomical shell is disconnected"));
    }
    let triangles: Vec<_> = mesh.triangles.iter().map(|t| t.map(|v| v as u32)).collect();
    validate_vertex_links(mesh.positions.len(), &triangles, &graph.boundary_next)?;
    let mut seen = BTreeSet::new();
    let mut rings = BTreeMap::new();
    for &start in graph.boundary_next.keys() {
        if seen.contains(&start) {
            continue;
        }
        let mut ring = Vec::new();
        let mut v = start;
        let mut common = u8::MAX;
        loop {
            if !seen.insert(v) {
                return Err(geometry("anatomical boundary pinches or fails to close"));
            }
            common &= mesh.boundaries[v as usize];
            ring.push(v);
            v = *graph
                .boundary_next
                .get(&v)
                .ok_or_else(|| geometry("anatomical boundary is an open chain"))?;
            if v == start {
                break;
            }
        }
        if ring.len() < 3 || common.count_ones() != 1 {
            return Err(geometry(
                "anatomical opening is missing its unique section provenance",
            ));
        }
        let cut = common.trailing_zeros() as usize;
        if rings.insert(cut, ring).is_some() {
            return Err(geometry("one anatomical cut produced multiple openings"));
        }
    }
    if rings.len() != spec.cuts.len() {
        return Err(geometry(
            "an anatomical cut removed or intersected a previously declared opening",
        ));
    }
    if mesh.positions.len() + mesh.triangles.len() + rings.len() != graph.edges.len() + 2 {
        return Err(geometry(
            "anatomical shell must have genus zero with exactly its named openings",
        ));
    }
    for (&index, ring) in &rings {
        if let Some(plane) = planes[index] {
            let area = ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .map(|(&a, &b)| {
                    (mesh.positions[a as usize] - plane.origin)
                        .cross(mesh.positions[b as usize] - plane.origin)
                        .dot(plane.normal)
                })
                .sum::<f64>();
            if !area.is_finite() || area == 0.0 {
                return Err(geometry("anatomical opening has zero projected area"));
            }
            if (area > 0.0) != spec.cuts[index].keep_positive {
                return Err(Refusal::new(
                    RefusalCode::InwardWound,
                    "anatomical shell is wound inward",
                ));
            }
        }
    }
    audit_open_shell(&mesh.positions, &triangles)?;
    let mut normals = vec![Vec3::ZERO; mesh.positions.len()];
    let mut shortest = vec![f64::INFINITY; mesh.positions.len()];
    for &[a, b, c] in &mesh.triangles {
        let normal =
            (mesh.positions[b] - mesh.positions[a]).cross(mesh.positions[c] - mesh.positions[a]);
        for v in [a, b, c] {
            normals[v] += normal;
        }
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let length = (mesh.positions[b] - mesh.positions[a]).norm();
            shortest[a] = shortest[a].min(length);
            shortest[b] = shortest[b].min(length);
        }
    }
    let original = mesh.positions.clone();
    for v in 0..mesh.positions.len() {
        let offset = mesh.offsets[v];
        if offset >= 0.0 && offset < spec.min_clearance_m {
            return Err(geometry(
                "requested anatomical ease is below the declared minimum displacement",
            ));
        }
        if offset < -0.35 * shortest[v] {
            return Err(geometry(
                "requested inward anatomical ease exceeds the conservative local edge bound",
            )
            .repair("Reduce the requested negative ease or supply a qualified offset surface."));
        }
        if offset == 0.0 {
            continue;
        }
        let mut normal = normals[v];
        for (index, plane) in planes.iter().enumerate() {
            if mesh.boundaries[v] & (1 << index) != 0
                && let Some(plane) = plane
            {
                normal = normal - plane.normal * normal.dot(plane.normal);
            }
        }
        let length = normal.norm();
        if !length.is_finite() || length == 0.0 {
            return Err(geometry(
                "anatomical ease has no representable boundary-compatible normal",
            ));
        }
        mesh.positions[v] += normal * (offset / length);
    }
    for &[a, b, c] in &mesh.triangles {
        let before = (original[b] - original[a]).cross(original[c] - original[a]);
        let after =
            (mesh.positions[b] - mesh.positions[a]).cross(mesh.positions[c] - mesh.positions[a]);
        if before.dot(after) <= 0.0 || !after.is_finite() {
            return Err(geometry(
                "requested anatomical ease inverts or collapses a triangle",
            ));
        }
    }
    if mesh.offsets.iter().any(|&offset| offset != 0.0) {
        audit_open_shell(&mesh.positions, &triangles)?;
    }
    let mut boundary_rings = Vec::new();
    for (index, ring) in rings {
        let perimeter_m = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(&a, &b)| (mesh.positions[a as usize] - mesh.positions[b as usize]).norm())
            .sum::<f64>();
        if !perimeter_m.is_finite() || perimeter_m <= 0.0 {
            return Err(geometry("anatomical opening has invalid perimeter"));
        }
        boundary_rings.push(BoundaryRing {
            name: spec.cuts[index].name.clone(),
            is_start: spec.cuts[index].is_start,
            vertices: ring,
            perimeter_m,
        });
    }
    boundary_rings.sort_by(|a, b| (!a.is_start, &a.name).cmp(&(!b.is_start, &b.name)));
    Ok(EaseShellMesh {
        vertices_m: mesh.positions.into_iter().map(Into::into).collect(),
        triangles,
        boundary_rings,
        source_body_sha256: source.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Affine;
    use crate::data::AnnyData;
    use ftx_core::Mat3;

    // A closed, outward voxel-union skin with four branches: waist, neck and two arms.
    // Shearing both arms downwards puts the genuine cuffs below the torso hem. A global
    // z half-space would amputate them. This is an analytic geometry fixture, not a person.
    fn branched_skin() -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let xs: [f64; 8] = [-3.0, -2.0, -1.0, -0.5, 0.5, 1.0, 2.0, 3.0];
        let zs = [-1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
        let mut cells = BTreeSet::new();
        for x in 0..xs.len() - 1 {
            for z in 0..zs.len() - 1 {
                let middle = 0.5 * (xs[x] + xs[x + 1]);
                if (middle.abs() < 1.0 && z < 4)
                    || (middle.abs() < 0.5 && z == 4)
                    || (middle.abs() >= 1.0 && z == 2)
                {
                    cells.insert((x, z));
                }
            }
        }
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut indices = BTreeMap::new();
        let mut quad = |corners: [(usize, usize, usize); 4]| {
            let ids = corners.map(|key @ (x, y, z)| {
                *indices.entry(key).or_insert_with(|| {
                    let v = vertices.len();
                    let descent = if xs[x].abs() > 1.0 {
                        0.8 * (xs[x].abs() - 1.0)
                    } else {
                        0.0
                    };
                    vertices.push(Vec3::new(
                        xs[x],
                        if y == 0 { -0.4 } else { 0.4 },
                        zs[z] - descent,
                    ));
                    v
                })
            });
            triangles.extend([[ids[0], ids[1], ids[2]], [ids[0], ids[2], ids[3]]]);
        };
        for &(x, z) in &cells {
            if x == 0 || !cells.contains(&(x - 1, z)) {
                quad([(x, 0, z), (x, 0, z + 1), (x, 1, z + 1), (x, 1, z)]);
            }
            if !cells.contains(&(x + 1, z)) {
                quad([
                    (x + 1, 0, z),
                    (x + 1, 1, z),
                    (x + 1, 1, z + 1),
                    (x + 1, 0, z + 1),
                ]);
            }
            quad([(x, 0, z), (x + 1, 0, z), (x + 1, 0, z + 1), (x, 0, z + 1)]);
            quad([(x, 1, z), (x, 1, z + 1), (x + 1, 1, z + 1), (x + 1, 1, z)]);
            if z == 0 || !cells.contains(&(x, z - 1)) {
                quad([(x, 0, z), (x, 1, z), (x + 1, 1, z), (x + 1, 0, z)]);
            }
            if !cells.contains(&(x, z + 1)) {
                quad([
                    (x, 0, z + 1),
                    (x + 1, 0, z + 1),
                    (x + 1, 1, z + 1),
                    (x, 1, z + 1),
                ]);
            }
        }
        (vertices, triangles)
    }

    fn plane(
        name: &str,
        origin: [f64; 3],
        normal: [f64; 3],
        start: bool,
        keep: bool,
    ) -> AnatomicalCut {
        AnatomicalCut {
            name: name.into(),
            is_start: start,
            keep_positive: keep,
            section: AnatomicalSection::Plane {
                origin_m: origin,
                normal,
            },
        }
    }

    fn full_spec(vertices: usize) -> AnatomicalShellSpec {
        AnatomicalShellSpec {
            cuts: vec![
                plane("waist_hem", [0.0, 0.0, 0.25], [0.0, 0.0, 1.0], true, true),
                plane("neck", [0.0, 0.0, 3.35], [0.0, 0.0, 1.0], false, false),
                plane(
                    "left_wrist_cuff",
                    [2.65, 0.0, 0.18],
                    [1.0, 0.0, -0.8],
                    true,
                    false,
                ),
                plane(
                    "right_wrist_cuff",
                    [-2.65, 0.0, 0.18],
                    [-1.0, 0.0, -0.8],
                    true,
                    false,
                ),
            ],
            radial_offsets_m: vec![0.0; vertices],
            min_clearance_m: 0.0,
            allow_negative_ease: false,
        }
    }

    fn assert_planar_rings(shell: &EaseShellMesh, spec: &AnatomicalShellSpec) {
        for cut in &spec.cuts {
            if let AnatomicalSection::Plane { origin_m, normal } = &cut.section {
                let ring = shell
                    .boundary_rings
                    .iter()
                    .find(|r| r.name == cut.name)
                    .unwrap();
                let origin = Vec3::from(*origin_m);
                let normal = Vec3::from(*normal).normalized();
                for &v in &ring.vertices {
                    assert!(
                        (Vec3::from(shell.vertices_m[v as usize]) - origin)
                            .dot(normal)
                            .abs()
                            < 1e-13
                    );
                }
            }
        }
    }

    #[test]
    fn branched_shell_keeps_lowered_arms_and_exact_named_plane_rings() {
        let (vertices, triangles) = branched_skin();
        let spec = full_spec(vertices.len());
        let shell = build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-branches")
            .unwrap();
        assert_eq!(shell.boundary_rings.len(), 4);
        assert_eq!(
            shell.boundary_rings.iter().filter(|r| r.is_start).count(),
            3
        );
        assert_planar_rings(&shell, &spec);
        for side in [-1.0, 1.0] {
            assert!(
                shell
                    .vertices_m
                    .iter()
                    .any(|p| side * p[0] > 2.0 && p[2] < 0.0)
            );
        }
        for ring in &shell.boundary_rings {
            let expected = match ring.name.as_str() {
                "waist_hem" => 5.6,
                "neck" => 3.6,
                "left_wrist_cuff" | "right_wrist_cuff" => 2.0 * (0.8 + 1.0 / 1.64_f64.sqrt()),
                name => panic!("unexpected opening {name}"),
            };
            assert!(
                (ring.perimeter_m - expected).abs() < 1e-12,
                "{}: {} vs {expected}",
                ring.name,
                ring.perimeter_m
            );
        }
        assert_eq!(
            shell,
            build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-branches")
                .unwrap()
        );
    }

    #[test]
    fn independent_segmentation_and_wrist_cut_extract_each_sleeve() {
        let (vertices, triangles) = branched_skin();
        for side in [-1.0, 1.0] {
            let mut spec = full_spec(vertices.len());
            let wrist = spec.cuts[if side > 0.0 { 2 } else { 3 }].clone();
            spec.cuts = vec![
                AnatomicalCut {
                    name: "source_armhole".into(),
                    is_start: false,
                    keep_positive: true,
                    section: AnatomicalSection::Segmentation {
                        signed_membership: vertices
                            .iter()
                            .map(|p| side * p.x - 1.35 + 0.15 * p.y * p.z)
                            .collect(),
                    },
                },
                wrist,
            ];
            let shell =
                build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-sleeve")
                    .unwrap();
            assert_eq!(shell.boundary_rings.len(), 2);
            assert!(shell.vertices_m.iter().all(|p| side * p[0] > 1.0));
            assert!(
                shell
                    .boundary_rings
                    .iter()
                    .any(|r| r.name == "source_armhole" && !r.is_start)
            );
            assert_planar_rings(&shell, &spec);
        }
    }

    #[test]
    fn anatomical_offsets_preserve_requested_distance_and_planes() {
        let (vertices, triangles) = branched_skin();
        let mut spec = full_spec(vertices.len());
        let original =
            build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-ease").unwrap();
        spec.radial_offsets_m.fill(0.003);
        spec.min_clearance_m = 0.002;
        let offset =
            build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-ease").unwrap();
        assert_eq!(offset.triangles, original.triangles);
        assert_planar_rings(&offset, &spec);
        for (&a, &b) in original.vertices_m.iter().zip(&offset.vertices_m) {
            assert!(((Vec3::from(a) - Vec3::from(b)).norm() - 0.003).abs() < 1e-14);
        }
        spec.radial_offsets_m.fill(0.001);
        let refusal = build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-ease")
            .unwrap_err();
        assert!(refusal.message.contains("minimum"));
        spec.radial_offsets_m.fill(-0.1);
        spec.allow_negative_ease = true;
        let refusal = build_anatomical_ease_shell(&vertices, &triangles, &spec, "synthetic-ease")
            .unwrap_err();
        assert!(refusal.message.contains("inward"));
    }

    #[test]
    fn missing_anatomical_witness_or_intersected_opening_refuses() {
        let (vertices, triangles) = branched_skin();
        let mut spec = full_spec(vertices.len());
        spec.cuts[0].section = AnatomicalSection::Plane {
            origin_m: [10.0, 0.0, 0.25],
            normal: [0.0, 0.0, 1.0],
        };
        let refusal = build_anatomical_ease_shell(&vertices, &triangles, &spec, "invalid-witness")
            .unwrap_err();
        assert!(refusal.message.contains("waist_hem"));
        let mut spec = full_spec(vertices.len());
        spec.cuts.push(plane(
            "second_hem",
            [0.0, 0.0, 0.75],
            [0.0, 0.0, 1.0],
            true,
            true,
        ));
        let refusal =
            build_anatomical_ease_shell(&vertices, &triangles, &spec, "lost-opening").unwrap_err();
        assert!(refusal.message.contains("previously declared opening"));
        let mut spec = full_spec(vertices.len());
        spec.cuts[0].section = AnatomicalSection::Segmentation {
            signed_membership: vec![1.0],
        };
        assert!(
            build_anatomical_ease_shell(&vertices, &triangles, &spec, "missing-field").is_err()
        );
    }

    fn rig_fixture() -> (AnnyModel, PosedBody) {
        let (vertices, triangles) = branched_skin();
        let labels = [
            "root",
            "upperarm01.L",
            "lowerarm01.L",
            "wrist.L",
            "upperarm01.R",
            "lowerarm01.R",
            "wrist.R",
            "neck01",
            "head",
        ];
        let parents = [-1, 0, 1, 2, 0, 4, 5, 0, 7];
        let heads = vec![
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.5),
            Vec3::new(1.5, 0.0, 1.1),
            Vec3::new(2.65, 0.0, 0.18),
            Vec3::new(-1.0, 0.0, 1.5),
            Vec3::new(-1.5, 0.0, 1.1),
            Vec3::new(-2.65, 0.0, 0.18),
            Vec3::new(0.0, 0.0, 3.35),
            Vec3::new(0.0, 0.0, 3.8),
        ];
        let mut waist: Vec<_> = vertices
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (p.z == 0.0 && p.x.abs() <= 1.0).then_some(i))
            .collect();
        waist.sort_by(|&a, &b| {
            vertices[a]
                .y
                .atan2(vertices[a].x)
                .total_cmp(&vertices[b].y.atan2(vertices[b].x))
        });
        let mut vertex_bone_indices = Vec::new();
        let mut vertex_bone_weights = Vec::new();
        for p in &vertices {
            let arm = if p.x.abs() <= 0.5 {
                0.0
            } else if p.x.abs() == 1.0 {
                0.25
            } else if p.x.abs() == 2.0 {
                0.75
            } else {
                1.0
            };
            let left = if p.x > 0.0 { arm } else { 0.0 };
            let right = if p.x < 0.0 { arm } else { 0.0 };
            vertex_bone_indices.extend([0, 1, 4]);
            vertex_bone_weights.extend([1.0 - left - right, left, right]);
        }
        let header = serde_json::from_value(serde_json::json!({
            "schema": "ftxbody/1", "model": "synthetic", "anny_version": "test",
            "configuration": "anatomical shell rig witness", "licence": "synthetic", "package_sha256": {},
            "counts": {
                "vertices": vertices.len(), "faces": triangles.len(), "bones": labels.len(),
                "blendshapes": 0, "phenotype_blendshapes": 0, "local_changes": 0, "influences": 3
            },
            "phenotype": {"features": [], "components": [], "anchors": {}, "exposed": [], "fixed_value": 0.5},
            "y_axis": [0.0, 1.0, 0.0],
            "degenerate_rotation": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            "labels": {"blendshapes": [], "bones": labels, "bone_parents": parents, "local_changes": []},
            "waist_loop_final": waist, "arrays": {}
        })).unwrap();
        let model = AnnyModel::from_data(AnnyData {
            header,
            template_vertices: vertices.clone(),
            faces: triangles.iter().map(|t| t.map(|v| v as u32)).collect(),
            blendshape_row_ptr: vec![0],
            blendshape_vertex: Vec::new(),
            blendshape_delta: Vec::new(),
            phenotype_mask: Vec::new(),
            template_bone_heads: heads.clone(),
            template_bone_tails: heads
                .iter()
                .map(|p| *p + Vec3::new(0.0, 0.1, 0.0))
                .collect(),
            bone_heads_blendshapes: Vec::new(),
            bone_tails_blendshapes: Vec::new(),
            bone_rolls: vec![Mat3::IDENTITY; labels.len()],
            vertex_bone_indices,
            vertex_bone_weights,
            base_mesh_vertex_indices: (0..vertices.len() as u32).collect(),
        })
        .unwrap();
        let bone_poses: Vec<_> = heads
            .into_iter()
            .map(|t| Affine {
                m: Mat3::IDENTITY,
                t,
            })
            .collect();
        let posed = PosedBody {
            vertices,
            transforms: bone_poses.clone(),
            bone_poses,
        };
        (model, posed)
    }

    #[test]
    fn model_adapter_uses_actual_rig_fields_and_independent_joints() {
        let (model, posed) = rig_fixture();
        let adapter = AnnyAnatomicalShell {
            model: &model,
            posed: &posed,
            source_body_sha256: "synthetic-source-rig".into(),
        };
        let ease = EaseProfile {
            torso_radial_mm: 0.0,
            limb_radial_mm: 0.0,
            min_clearance_mm: 0.0,
            allow_negative_ease: false,
        };
        for (region, expected) in [
            (
                KnitShellRegion::FullUpperBody,
                vec!["left_wrist_cuff", "neck", "right_wrist_cuff", "waist_hem"],
            ),
            (
                KnitShellRegion::Torso,
                vec!["left_armhole", "neck", "right_armhole", "waist_hem"],
            ),
            (
                KnitShellRegion::LeftSleeve,
                vec!["left_armhole", "left_wrist_cuff"],
            ),
            (
                KnitShellRegion::RightSleeve,
                vec!["right_armhole", "right_wrist_cuff"],
            ),
        ] {
            let shell = adapter.build_ease_shell(region, &ease).unwrap();
            let mut names: Vec<_> = shell
                .boundary_rings
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            names.sort_unstable();
            assert_eq!(names, expected);
            assert_eq!(shell.source_body_sha256, "synthetic-source-rig");
        }
        let mut altered = posed.clone();
        altered.bone_poses[3].t = Vec3::new(20.0, 0.0, 0.18);
        let bad = AnnyAnatomicalShell {
            model: &model,
            posed: &altered,
            source_body_sha256: "bad-source-joint".into(),
        };
        assert!(
            bad.build_ease_shell(KnitShellRegion::LeftSleeve, &ease)
                .is_err()
        );
        // Changing only the left wrist must not alter independently sourced right-sleeve cuts.
        assert_eq!(
            adapter
                .build_ease_shell(KnitShellRegion::RightSleeve, &ease)
                .unwrap(),
            bad.build_ease_shell(KnitShellRegion::RightSleeve, &ease)
                .map(|mut s| {
                    s.source_body_sha256 = "synthetic-source-rig".into();
                    s
                })
                .unwrap()
        );
    }

    #[test]
    fn anatomical_model_sections_follow_rigid_body_rotation() {
        let (model, posed) = rig_fixture();
        let ease = EaseProfile {
            torso_radial_mm: 0.0,
            limb_radial_mm: 0.0,
            min_clearance_mm: 0.0,
            allow_negative_ease: false,
        };
        let original = AnnyAnatomicalShell {
            model: &model,
            posed: &posed,
            source_body_sha256: "synthetic-rotation".into(),
        }
        .build_ease_shell(KnitShellRegion::FullUpperBody, &ease)
        .unwrap();
        // An exact axis permutation puts the body's waist normal along world X. This
        // catches a hidden global-height plane without adding trigonometric fixture error.
        let rotate = |p: Vec3| Vec3::new(p.z, p.y, -p.x);
        let mut rotated = posed.clone();
        for p in &mut rotated.vertices {
            *p = rotate(*p);
        }
        for pose in &mut rotated.bone_poses {
            pose.t = rotate(pose.t);
        }
        let shell = AnnyAnatomicalShell {
            model: &model,
            posed: &rotated,
            source_body_sha256: "synthetic-rotation".into(),
        }
        .build_ease_shell(KnitShellRegion::FullUpperBody, &ease)
        .unwrap();
        assert_eq!(shell.triangles, original.triangles);
        assert_eq!(shell.vertices_m.len(), original.vertices_m.len());
        for (&source, &got) in original.vertices_m.iter().zip(&shell.vertices_m) {
            assert!((rotate(Vec3::from(source)) - Vec3::from(got)).norm() < 1e-13);
        }
        for (a, b) in original.boundary_rings.iter().zip(&shell.boundary_rings) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.vertices, b.vertices);
            assert!((a.perimeter_m - b.perimeter_m).abs() < 1e-12);
        }
    }
}

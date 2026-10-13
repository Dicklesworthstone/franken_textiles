//! The body as the drape needs it (plan §9.5): poses as local-bone deltas, a ramp from the rest pose, a
//! nappy, and the skin as a closed, outward-wound triangle mesh with its checks.
//!
//! **Joint rotations.** Each posed joint turns its bone about its head by an angle φ about an axis `a`
//! given in the rest pose's world frame. Because a child bone's world rotation is its parent's times its
//! own (spec 05 §5.2), the local-bone delta of that joint is the rotation vector `φ·R_restᵀ·a`
//! (`R_rest` the bone's rest frame), whatever its parents do. A ramp scales every φ by t ∈ [0, 1].
//!
//! - **Arms** (`upperarm01.L/R`): the bone's rest direction `d = R_rest·ŷ` hangs θ₀ = acos(−d_z) from
//!   vertical (about 43° on Anny); abduction θ turns it in the plane of `d` and the vertical:
//!   `a = normalize(d × (0, 0, −1))`, φ = θ₀ − θ. The drape's A-pose is 35°; 50° and arms up (150°) are
//!   for ride-up checks.
//! - **Straight arms** (optional; [`Pose::drape`]): Anny's rest elbow is bent about 46° with the forearm
//!   pointing forward, and `upperarm02` sits about 10° off `upperarm01`, so no straight sleeve tube
//!   contains the arm. `upperarm02` turns onto `upperarm01`'s rest direction and `lowerarm01` onto
//!   `upperarm02`'s (each the minimal rotation, `a = normalize(d × d′)`, φ = acos(d·d′)); composed, the
//!   whole arm lies along `upperarm01`.
//! - **Legs** (about the world +X axis, the body's left): seated = hips (`upperleg01.L/R`) −90°, knees
//!   (`lowerleg01.L/R`) +90°; squat (upright trunk) = hips −100°, knees +110°, feet (`foot.L/R`) −10° so
//!   the soles stay level.
//!
//! **Nappy.** Anny has no nappy; [`nappy_local_changes`] adds `buttocks-volume-incr` = s and
//! `hip-scale-depth-incr` = s/2. On the 2.5-year reference girl s = 1 adds about 113 mm to the torso's
//! seat girth (measured on the Python reference); calibrating s to a taped `seat_over_nappy` needs the
//! tape functionals of FTX-09.
//!
//! **Skin.** The converted mesh is four closed shells: the skin, two eyes and the tongue. [`skin_mesh`]
//! keeps the shell enclosing the most volume and refuses it unless every edge has exactly two triangles
//! traversing it in opposite directions, no triangle is degenerate, and the enclosed volume is positive.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec3, det};

use crate::kinematics::Affine;
use crate::model::{AnnyModel, BodyParams, RestBody};

/// Leg posture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legs {
    /// The rest legs.
    Standing,
    /// Hips and knees at 90°.
    Seated,
    /// Hips 100°, knees 110°, feet level; trunk upright.
    Squat,
}

/// A drape pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Abduction of both arms from vertical (degrees, 0–180); `None` keeps the rest arms.
    pub arm_abduction_deg: Option<f64>,
    /// Leg posture.
    pub legs: Legs,
    /// Straighten both elbows (and the upper arm's twist segment) onto the upper arm's line.
    pub straight_arms: bool,
}

impl Pose {
    /// The drape's default arm abduction (spec 02 `A_POSE_DEG`).
    pub const A_POSE_DEG: f64 = 35.0;
    /// Arms up for ride-up checks.
    pub const ARMS_UP_DEG: f64 = 150.0;

    /// The rest pose (identity deltas).
    #[must_use]
    pub fn rest() -> Self {
        Self {
            arm_abduction_deg: None,
            legs: Legs::Standing,
            straight_arms: false,
        }
    }

    /// Standing, arms at `deg` from vertical.
    #[must_use]
    pub fn a_pose(deg: f64) -> Self {
        Self {
            arm_abduction_deg: Some(deg),
            legs: Legs::Standing,
            straight_arms: false,
        }
    }

    /// Standing, arms at [`Pose::ARMS_UP_DEG`].
    #[must_use]
    pub fn arms_up() -> Self {
        Self::a_pose(Self::ARMS_UP_DEG)
    }

    /// Seated, arms in the A-pose.
    #[must_use]
    pub fn seated() -> Self {
        Self {
            arm_abduction_deg: Some(Self::A_POSE_DEG),
            legs: Legs::Seated,
            straight_arms: false,
        }
    }

    /// Squatting, arms in the A-pose.
    #[must_use]
    pub fn squat() -> Self {
        Self {
            arm_abduction_deg: Some(Self::A_POSE_DEG),
            legs: Legs::Squat,
            straight_arms: false,
        }
    }

    /// The drape's pose: standing, arms at [`Pose::A_POSE_DEG`] and straight.
    #[must_use]
    pub fn drape() -> Self {
        Self {
            straight_arms: true,
            ..Self::a_pose(Self::A_POSE_DEG)
        }
    }
}

/// One joint rotation: a bone turned about its head by `angle` (radians) about `axis` (unit, rest-pose
/// world frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointRotation {
    /// Bone index.
    pub bone: usize,
    /// Axis in the rest pose's world frame.
    pub axis: Vec3,
    /// Angle (radians).
    pub angle: f64,
}

const SEATED: [(&str, f64); 4] = [
    ("upperleg01.L", -90.0),
    ("upperleg01.R", -90.0),
    ("lowerleg01.L", 90.0),
    ("lowerleg01.R", 90.0),
];

const SQUAT: [(&str, f64); 6] = [
    ("upperleg01.L", -100.0),
    ("upperleg01.R", -100.0),
    ("lowerleg01.L", 110.0),
    ("lowerleg01.R", 110.0),
    ("foot.L", -10.0),
    ("foot.R", -10.0),
];

fn bone(model: &AnnyModel, label: &str) -> Result<usize, Refusal> {
    model.bone_index(label).ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("the body model has no bone {label}"),
        )
    })
}

/// The joint rotations of `pose` on this rest body, in a fixed order (arms L, R; straightening L,
/// R; then legs).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for an abduction outside 0–180°; [`RefusalCode::InvalidDocument`] when
/// the model lacks a posed bone.
pub fn joint_rotations(
    model: &AnnyModel,
    rest: &RestBody,
    pose: &Pose,
) -> Result<Vec<JointRotation>, Refusal> {
    let mut out = Vec::new();
    if let Some(deg) = pose.arm_abduction_deg {
        if !(0.0..=180.0).contains(&deg) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("arm abduction {deg}° is outside 0–180°"),
            ));
        }
        for label in ["upperarm01.L", "upperarm01.R"] {
            let b = bone(model, label)?;
            let d = rest.bone_frames[b].m.col(1).normalized();
            let theta0 = det::acos((-d.z).clamp(-1.0, 1.0));
            out.push(JointRotation {
                bone: b,
                axis: d.cross(Vec3::new(0.0, 0.0, -1.0)).normalized(),
                angle: theta0 - deg.to_radians(),
            });
        }
    }
    if pose.straight_arms {
        for side in ["L", "R"] {
            let dir = |label: &str| -> Result<(usize, Vec3), Refusal> {
                let b = bone(model, &format!("{label}.{side}"))?;
                Ok((b, rest.bone_frames[b].m.col(1).normalized()))
            };
            let ((_, d1), (b2, d2), (b3, d3)) =
                (dir("upperarm01")?, dir("upperarm02")?, dir("lowerarm01")?);
            for (b, from, to) in [(b2, d2, d1), (b3, d3, d2)] {
                let axis = from.cross(to);
                if axis.norm() > 1e-12 {
                    out.push(JointRotation {
                        bone: b,
                        axis: axis.normalized(),
                        angle: det::acos(from.dot(to).clamp(-1.0, 1.0)),
                    });
                }
            }
        }
    }
    let legs: &[(&str, f64)] = match pose.legs {
        Legs::Standing => &[],
        Legs::Seated => &SEATED,
        Legs::Squat => &SQUAT,
    };
    for &(label, deg) in legs {
        out.push(JointRotation {
            bone: bone(model, label)?,
            axis: Vec3::new(1.0, 0.0, 0.0),
            angle: deg.to_radians(),
        });
    }
    Ok(out)
}

/// Local-bone deltas for [`AnnyModel::pose`]: `pose` reached a fraction `t` of the way from rest.
///
/// # Errors
/// As [`joint_rotations`]; [`RefusalCode::OutOfRange`] for `t` outside 0–1.
pub fn pose_deltas(
    model: &AnnyModel,
    rest: &RestBody,
    pose: &Pose,
    t: f64,
) -> Result<Vec<Affine>, Refusal> {
    if !(0.0..=1.0).contains(&t) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("pose ramp fraction {t} is outside 0–1"),
        ));
    }
    let mut deltas = vec![Affine::IDENTITY; rest.bone_frames.len()];
    for j in joint_rotations(model, rest, pose)? {
        let r = rest.bone_frames[j.bone].m.transpose().mul_vec(j.axis) * (j.angle * t);
        deltas[j.bone] = Affine::from_rotvec(r, Vec3::ZERO);
    }
    Ok(deltas)
}

/// Body parameters with a nappy of size `s` added to `params`' local changes
/// (`buttocks-volume-incr` += s, `hip-scale-depth-incr` += s/2).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] for a negative or non-finite `s` or a local-change vector of the wrong
/// length; [`RefusalCode::InvalidDocument`] when the model lacks the two local changes.
pub fn nappy_local_changes(
    model: &AnnyModel,
    params: &BodyParams,
    s: f64,
) -> Result<BodyParams, Refusal> {
    if !(s.is_finite() && s >= 0.0) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("nappy size {s} must be finite and non-negative"),
        ));
    }
    let n = model.data().header.labels.local_changes.len();
    if params.local_changes.len() != n {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "{} local changes given; the model has {n}",
                params.local_changes.len()
            ),
        ));
    }
    let mut out = params.clone();
    for (label, k) in [("buttocks-volume-incr", 1.0), ("hip-scale-depth-incr", 0.5)] {
        let i = model.local_change_index(label).ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("the body model has no local change {label}"),
            )
        })?;
        out.local_changes[i] += k * s;
    }
    Ok(out)
}

/// Checks of a skin mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinReport {
    /// Closed shells left out (eyes, tongue).
    pub shells_dropped: usize,
    /// Triangles in the shells left out.
    pub triangles_dropped: usize,
    /// Enclosed volume (m³; positive = outward winding).
    pub volume: f64,
    /// Surface area (m²).
    pub area: f64,
    /// Smallest triangle area (m²).
    pub min_triangle_area: f64,
}

/// The skin: a closed, outward-wound triangle mesh (metres) with each vertex's index in the source mesh.
#[derive(Clone, Debug)]
pub struct SkinMesh {
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// Triangles (outward winding).
    pub triangles: Vec<[usize; 3]>,
    /// Index of each vertex in the source mesh.
    pub source_vertex: Vec<usize>,
    /// Checks.
    pub report: SkinReport,
}

impl SkinMesh {
    /// Wavefront OBJ text (metres, z up, 17 significant digits; 1-based indices).
    #[must_use]
    pub fn to_obj(&self) -> String {
        use std::fmt::Write;
        let mut s = String::with_capacity(64 * (self.vertices.len() + self.triangles.len()));
        let _ = writeln!(
            s,
            "# FrankenTextiles skin mesh (metres, z up, the body faces -y)"
        );
        for v in &self.vertices {
            let _ = writeln!(s, "v {:.17e} {:.17e} {:.17e}", v.x, v.y, v.z);
        }
        for t in &self.triangles {
            let _ = writeln!(s, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1);
        }
        s
    }
}

fn find(root: &mut [usize], mut i: usize) -> usize {
    while root[i] != i {
        root[i] = root[root[i]];
        i = root[i];
    }
    i
}

/// The skin shell of a body mesh, checked (see the module notes).
///
/// # Errors
/// [`RefusalCode::DegenerateGeometry`] for an empty mesh, an index out of range, a degenerate triangle,
/// or a skin that is open, non-manifold or inconsistently wound; [`RefusalCode::InwardWound`] for a
/// negative enclosed volume; [`RefusalCode::NonFinite`] for a non-finite vertex.
pub fn skin_mesh(vertices: &[Vec3], faces: &[[u32; 3]]) -> Result<SkinMesh, Refusal> {
    let degenerate = |m: String| Refusal::new(RefusalCode::DegenerateGeometry, m);
    if faces.is_empty()
        || faces
            .iter()
            .flatten()
            .any(|&i| i as usize >= vertices.len())
    {
        return Err(degenerate(
            "the body mesh is empty or indexes a missing vertex".into(),
        ));
    }
    if vertices.iter().any(|v| !v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "the body mesh has a non-finite vertex",
        ));
    }
    // Shells: triangles connected through shared vertices.
    let mut root: Vec<usize> = (0..vertices.len()).collect();
    for f in faces {
        for k in 1..3 {
            let (a, b) = (
                find(&mut root, f[0] as usize),
                find(&mut root, f[k] as usize),
            );
            root[a.max(b)] = a.min(b);
        }
    }
    // Each shell's enclosed volume; the skin encloses the most (ties to the lowest root).
    let mut shells: BTreeMap<usize, f64> = BTreeMap::new();
    for f in faces {
        let [a, b, c] = f.map(|i| vertices[i as usize]);
        *shells.entry(find(&mut root, f[0] as usize)).or_default() += a.cross(b).dot(c) / 6.0;
    }
    let (&skin, _) = shells
        .iter()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()).then(b.0.cmp(a.0)))
        .ok_or_else(|| degenerate("the body mesh has no triangles".into()))?;
    let mut index = vec![usize::MAX; vertices.len()];
    let (mut out_v, mut source, mut tris) = (Vec::new(), Vec::new(), Vec::new());
    for f in faces {
        if find(&mut root, f[0] as usize) != skin {
            continue;
        }
        tris.push(f.map(|i| {
            let i = i as usize;
            if index[i] == usize::MAX {
                index[i] = out_v.len();
                out_v.push(vertices[i]);
                source.push(i);
            }
            index[i]
        }));
    }
    // Every directed edge once, and its reverse once: closed, manifold, consistently wound.
    let mut directed: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for t in &tris {
        for k in 0..3 {
            *directed.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    if let Some((&(a, b), _)) = directed
        .iter()
        .find(|&(&(a, b), &n)| n != 1 || directed.get(&(b, a)) != Some(&1))
    {
        return Err(degenerate(format!(
            "the skin is open, non-manifold or inconsistently wound at the edge {}–{}",
            source[a], source[b]
        )));
    }
    let (mut volume, mut area, mut min_area) = (0.0, 0.0, f64::INFINITY);
    for t in &tris {
        let (a, b, c) = (out_v[t[0]], out_v[t[1]], out_v[t[2]]);
        volume += a.cross(b).dot(c) / 6.0;
        let ar = 0.5 * (b - a).cross(c - a).norm();
        area += ar;
        min_area = min_area.min(ar);
    }
    if min_area <= 1e-14 {
        return Err(degenerate(format!(
            "the skin has a degenerate triangle (area {min_area:.3e} m²)"
        )));
    }
    if volume <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::InwardWound,
            format!("the skin encloses a negative volume ({volume:.6} m³): it is wound inward"),
        ));
    }
    let (shells_dropped, triangles_dropped) = (shells.len() - 1, faces.len() - tris.len());
    Ok(SkinMesh {
        vertices: out_v,
        triangles: tris,
        source_vertex: source,
        report: SkinReport {
            shells_dropped,
            triangles_dropped,
            volume,
            area,
            min_triangle_area: min_area,
        },
    })
}

/// The skin without the regions skinned mostly to the given bones and their descendants (the hands and
/// feet, which no garment in scope covers and whose thin fingers and toes a coarse garment mesh cannot
/// clear), each opening closed by a fan around its centroid; checked as [`skin_mesh`].
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] for an unknown bone; [`RefusalCode::DegenerateGeometry`] when an
/// opening is not a simple loop; and as [`skin_mesh`].
pub fn trimmed_skin(
    model: &AnnyModel,
    vertices: &[Vec3],
    roots: &[&str],
) -> Result<SkinMesh, Refusal> {
    let data = model.data();
    let parents = &data.header.labels.bone_parents;
    let mut cut = vec![false; parents.len()];
    for label in roots {
        let r = bone(model, label)?;
        cut[r] = true;
        for j in r + 1..parents.len() {
            if let Ok(p) = usize::try_from(parents[j])
                && cut[p]
            {
                cut[j] = true;
            }
        }
    }
    let k = data.header.counts.influences;
    let dropped: Vec<bool> = (0..vertices.len())
        .map(|v| {
            let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
            let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
            cut[data.vertex_bone_indices[v * k + best] as usize]
        })
        .collect();
    let skin = skin_mesh(vertices, &data.faces)?;
    let keep = |t: &[usize; 3]| t.iter().all(|&i| !dropped[skin.source_vertex[i]]);
    let mut tris: Vec<[usize; 3]> = skin.triangles.iter().filter(|t| keep(t)).copied().collect();
    let mut verts = skin.vertices.clone();
    // Openings: directed edges without their reverse, chained into loops.
    let mut directed: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for t in &tris {
        for e in 0..3 {
            *directed.entry((t[e], t[(e + 1) % 3])).or_default() += 1;
        }
    }
    let mut next: BTreeMap<usize, usize> = BTreeMap::new();
    for &(a, b) in directed.keys() {
        if !directed.contains_key(&(b, a)) && next.insert(b, a).is_some() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("the trimmed skin has a pinched opening at vertex {b}"),
            ));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for &s in next.keys() {
        if !seen.insert(s) {
            continue;
        }
        let mut ring = vec![s];
        let mut v = next[&s];
        while v != s {
            if !seen.insert(v) || ring.len() > next.len() {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "the trimmed skin has an opening that is not a simple loop",
                ));
            }
            ring.push(v);
            v = *next.get(&v).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "the trimmed skin has an open boundary chain",
                )
            })?;
        }
        // `next` runs along the reversed boundary, so (ring[i], ring[i+1], centre) winds outward.
        let c = verts.len();
        let centre = ring.iter().fold(Vec3::ZERO, |a, &i| a + verts[i]) / ring.len() as f64;
        verts.push(centre);
        for i in 0..ring.len() {
            tris.push([ring[i], ring[(i + 1) % ring.len()], c]);
        }
    }
    let faces: Vec<[u32; 3]> = tris
        .iter()
        .map(|t| t.map(|i| u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    let mut out = skin_mesh(&verts, &faces)?;
    // Source indices of the original mesh (cap centres get usize::MAX).
    out.source_vertex = out
        .source_vertex
        .iter()
        .map(|&i| skin.source_vertex.get(i).copied().unwrap_or(usize::MAX))
        .collect();
    Ok(out)
}

/// The posed skin of a body: rest shape from `params`, `pose` a fraction `t` of the way from rest.
///
/// # Errors
/// As [`AnnyModel::rest`], [`pose_deltas`], [`AnnyModel::pose`] and [`skin_mesh`].
pub fn posed_skin(
    model: &AnnyModel,
    params: &BodyParams,
    pose: &Pose,
    t: f64,
) -> Result<SkinMesh, Refusal> {
    let rest = model.rest(params)?;
    let deltas = pose_deltas(model, &rest, pose, t)?;
    let posed = model.pose(&rest, &deltas)?;
    skin_mesh(&posed.vertices, &model.data().faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube (outward) at `o` with side `s`, as 12 triangles.
    fn cube(o: Vec3, s: f64, base: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let v: Vec<Vec3> = (0..8)
            .map(|i| {
                o + Vec3::new(
                    f64::from(i & 1) * s,
                    f64::from((i >> 1) & 1) * s,
                    f64::from((i >> 2) & 1) * s,
                )
            })
            .collect();
        let quads = [
            [0, 2, 3, 1], // z = 0, normal −z
            [4, 5, 7, 6], // z = s, +z
            [0, 1, 5, 4], // y = 0, −y
            [2, 6, 7, 3], // y = s, +y
            [0, 4, 6, 2], // x = 0, −x
            [1, 3, 7, 5], // x = s, +x
        ];
        let f = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .map(|t: [u32; 3]| t.map(|i| i + base))
            .collect();
        (v, f)
    }

    /// The skin is the shell enclosing the most volume (here listed second); the small inner shell is
    /// dropped and counted; volume and area are exact for a cube.
    #[test]
    fn skin_keeps_the_largest_closed_shell() {
        let (mut v, mut f) = cube(Vec3::new(0.4, 0.4, 0.4), 0.1, 0);
        let (v2, f2) = cube(Vec3::ZERO, 1.0, 8);
        v.extend(v2);
        f.extend(f2);
        let s = skin_mesh(&v, &f).unwrap();
        assert_eq!(s.source_vertex[0], 8);
        assert_eq!((s.vertices.len(), s.triangles.len()), (8, 12));
        assert_eq!(s.report.shells_dropped, 1);
        assert_eq!(s.report.triangles_dropped, 12);
        assert!((s.report.volume - 1.0).abs() < 1e-15);
        assert!((s.report.area - 6.0).abs() < 1e-15);
    }

    /// Inward winding, an open skin and a flipped triangle are refused.
    #[test]
    fn bad_skins_are_refused() {
        let (v, f) = cube(Vec3::ZERO, 1.0, 0);
        let inward: Vec<[u32; 3]> = f.iter().map(|t| [t[0], t[2], t[1]]).collect();
        assert_eq!(
            skin_mesh(&v, &inward).unwrap_err().code,
            RefusalCode::InwardWound
        );
        assert_eq!(
            skin_mesh(&v, &f[..11]).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        let mut flipped = f.clone();
        flipped[0] = [f[0][0], f[0][2], f[0][1]];
        assert_eq!(
            skin_mesh(&v, &flipped).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
    }
}

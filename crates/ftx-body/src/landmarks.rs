//! Part frames, landmark heights and section radii on a posed Anny body: what the drape needs to place
//! pattern pieces (spec 04 §3.2) on the realistic body instead of the reference's dress form.
//!
//! Everything is read off the posed skeleton and skin, in millimetres in the posed (root-relative) frame;
//! heights are above the floor (the lowest skin point). Vertices are assigned to a region by their
//! dominant skinning bone: an arm (the descendants of `upperarm01`), a leg (of `upperleg01`), or the
//! torso (everything else, head and neck included).
//!
//! | Item | Definition |
//! |---|---|
//! | floor | lowest skin point |
//! | waist | mean height of Anny's 46-vertex waist loop |
//! | crotch | lowest torso vertex within 4 mm of the sagittal plane |
//! | hip | crotch + 0.45·(waist − crotch), the reference's seat level |
//! | knee, ankle | `lowerleg01`, `foot` heads; calf = ankle + 0.6·(knee − ankle), as the reference |
//! | chest | the armpit: the lowest upper-arm vertex within one arm radius of the torso's side (the reference's C7 − armscye depth) |
//! | across_back | midway between chest and shoulder, as the reference |
//! | shoulder | shoulder tip: the arm frame origin plus `tip_h` along the arm |
//! | snp | `neck01` head; c7 = snp + 5 mm (the reference form's snp = C7 − 5); head_top = highest skin point |
//! | torso frame | origin (0, y_c, floor) with y_c the waist loop's mid-depth; up +Z, front −Y, lat +X |
//! | arm_R / arm_L frame | origin the corresponding `upperarm01` head, up from elbow to shoulder, front −Y made orthogonal |
//! | upper-arm radius r | median distance of upper-arm vertices from the arm axis; `tip_h = 0.8·r` as the reference |
//! | leg_R / leg_L frame | origin (corresponding hip-joint x, y_c, crotch + 25 mm), up from that ankle to hip joint |
//! | section radius | torso: farthest torso vertex from the axis within ±10 mm of h; arm: 1.15·r of that arm; leg: farthest same-leg vertex from its axis within ±10 mm of h, and same-half torso vertices when within 20 mm below the origin or above (the pelvis) |
//!
//! Frames are meant to be taken on the standing A-pose body; other poses are reached by a pose ramp.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::model::{AnnyModel, PosedBody};

/// A part frame (mm): `h` runs along `up` from `origin`; θ = 0 faces `front`, 90° faces `lat`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Origin (mm).
    pub origin: Vec3,
    /// Proximal direction (unit).
    pub up: Vec3,
    /// θ = 0 (unit, ⟂ up).
    pub front: Vec3,
    /// `up × front`.
    pub lat: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Torso,
    ArmR,
    ArmL,
    LegR,
    LegL,
}

/// Frames, landmarks and section radii of one posed body.
#[derive(Clone, Debug)]
pub struct BodyFrames {
    /// The torso frame.
    pub torso: Frame,
    /// The wearer's-right arm frame.
    pub arm_r: Frame,
    /// The wearer's-left arm frame from its own posed shoulder and bone direction.
    pub arm_l: Frame,
    /// The wearer's-right leg frame.
    pub leg_r: Frame,
    /// The wearer's-left leg frame from its own posed hip and ankle.
    pub leg_l: Frame,
    /// World height of the floor (mm).
    pub floor_z: f64,
    /// Right shoulder tip along the arm's `up` from its origin (mm).
    pub tip_h: f64,
    /// Left shoulder tip along the left arm's `up` from its origin (mm).
    pub tip_h_l: f64,
    /// Right upper-arm radius (mm).
    pub upper_arm_radius: f64,
    /// Left upper-arm radius (mm), measured from left upper-arm vertices.
    pub upper_arm_radius_l: f64,
    /// Right shoulder tip to wrist (mm).
    pub arm_length: f64,
    /// Left shoulder tip to wrist (mm).
    pub arm_length_l: f64,
    /// Landmark heights above the floor (mm).
    pub landmarks: BTreeMap<String, f64>,
    /// The neck frame: origin the `neck01` head (the base of the neck), `up` towards the `head` bone's head.
    pub neck: Frame,
    /// The `head` bone's head (mm): the axis of the horizontal head sections.
    pub head_bone: Vec3,
    /// The wearer's-right shoulder joint, the `upperarm01.R` head (mm).
    pub shoulder_joint_r: Vec3,
    /// Torso vertices (head and neck included; mm).
    pub(crate) torso_points: Vec<Vec3>,
    /// Wearer's-right leg vertices (mm).
    pub(crate) leg_points: Vec<Vec3>,
    /// Wearer's-left leg vertices (mm).
    pub(crate) leg_points_l: Vec<Vec3>,
    /// Left ankle, calf and knee heights above the floor (mm), in that order.
    pub(crate) leg_landmarks_l: [f64; 3],
}

fn orthonormal(up: Vec3) -> Frame {
    let up = up.normalized();
    let f = Vec3::new(0.0, -1.0, 0.0);
    let front = (f - up * f.dot(up)).normalized();
    Frame {
        origin: Vec3::ZERO,
        up,
        front,
        lat: up.cross(front),
    }
}

/// Distance of `p` from the line through `o` along unit `u`, and the coordinate of `p` along it.
fn axial(p: Vec3, o: Vec3, u: Vec3) -> (f64, f64) {
    let d = p - o;
    let h = d.dot(u);
    ((d - u * h).norm(), h)
}

fn descendants(parents: &[i64], root: usize) -> Vec<bool> {
    let mut inside = vec![false; parents.len()];
    inside[root] = true;
    // Parents precede children (checked when the model is read), so one pass suffices.
    for j in root + 1..parents.len() {
        if let Ok(p) = usize::try_from(parents[j])
            && inside[p]
        {
            inside[j] = true;
        }
    }
    inside
}

impl BodyFrames {
    /// Frames and landmarks of `posed` (from `model`).
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] when the model lacks one of the bones used;
    /// [`RefusalCode::DegenerateGeometry`] when a region has no vertices.
    pub fn new(model: &AnnyModel, posed: &PosedBody) -> Result<Self, Refusal> {
        let data = model.data();
        let bone = |label: &str| {
            model.bone_index(label).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("the body model has no bone {label}"),
                )
            })
        };
        let parents = &data.header.labels.bone_parents;
        let chains = [
            (descendants(parents, bone("upperarm01.R")?), Region::ArmR),
            (descendants(parents, bone("upperarm01.L")?), Region::ArmL),
            (descendants(parents, bone("upperleg01.R")?), Region::LegR),
            (descendants(parents, bone("upperleg01.L")?), Region::LegL),
        ];
        let k = data.header.counts.influences;
        let mm: Vec<Vec3> = posed.vertices.iter().map(|v| *v * 1000.0).collect();
        let region: Vec<Region> = (0..mm.len())
            .map(|v| {
                let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
                let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
                let b = data.vertex_bone_indices[v * k + best] as usize;
                chains
                    .iter()
                    .find(|(c, _)| c[b])
                    .map_or(Region::Torso, |(_, r)| *r)
            })
            .collect();
        let head = |label: &str| -> Result<Vec3, Refusal> {
            Ok(posed.bone_poses[bone(label)?].t * 1000.0)
        };
        let floor_z = mm.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        let top_z = mm.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
        let waist_loop: Vec<Vec3> = data
            .header
            .waist_loop_final
            .iter()
            .map(|&i| mm[i])
            .collect();
        if waist_loop.is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "the body model has no waist loop",
            ));
        }
        let waist_z = waist_loop.iter().map(|p| p.z).sum::<f64>() / waist_loop.len() as f64;
        let (ylo, yhi) = waist_loop
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                (a.min(p.y), b.max(p.y))
            });
        let y_c = 0.5 * (ylo + yhi);
        let crotch_z = mm
            .iter()
            .zip(&region)
            .filter(|(p, r)| **r == Region::Torso && p.x.abs() < 4.0 && p.z < waist_z)
            .map(|(p, _)| p.z)
            .fold(f64::INFINITY, f64::min);
        let none = |what: &str| {
            Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("the body has no {what} vertices"),
            )
        };
        if !crotch_z.is_finite() {
            return Err(none("crotch"));
        }
        // Arm.
        let shoulder = head("upperarm01.R")?;
        let mut arm = orthonormal(-posed.bone_poses[bone("upperarm01.R")?].m.col(1));
        arm.origin = shoulder;
        let upper = [bone("upperarm01.R")?, bone("upperarm02.R")?];
        let mut radii: Vec<f64> = (0..mm.len())
            .filter(|&v| {
                let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
                let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
                upper.contains(&(data.vertex_bone_indices[v * k + best] as usize))
            })
            .map(|v| axial(mm[v], arm.origin, arm.up).0)
            .collect();
        if radii.is_empty() {
            return Err(none("upper-arm"));
        }
        radii.sort_by(f64::total_cmp);
        let upper_arm_radius = radii[radii.len() / 2];
        let tip_h = 0.8 * upper_arm_radius;
        // Armpit: the lowest upper-arm vertex within one arm radius of the torso's side (|x| below the
        // shoulder joint's).
        let armpit_z = (0..mm.len())
            .filter(|&v| {
                let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
                let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
                upper.contains(&(data.vertex_bone_indices[v * k + best] as usize))
                    && mm[v].x.abs() < shoulder.x.abs() + upper_arm_radius
            })
            .map(|v| mm[v].z)
            .fold(f64::INFINITY, f64::min);
        if !armpit_z.is_finite() {
            return Err(none("armpit"));
        }
        let arm_length = (head("wrist.R")? - shoulder).norm() + tip_h;
        // Explicit physical left panels use the same definitions on their own posed joints and
        // skin. Reflecting the right side loses independently posed or shaped left limbs.
        let shoulder_l = head("upperarm01.L")?;
        let mut arm_l = orthonormal(-posed.bone_poses[bone("upperarm01.L")?].m.col(1));
        arm_l.origin = shoulder_l;
        let upper_l = [bone("upperarm01.L")?, bone("upperarm02.L")?];
        let mut radii_l: Vec<f64> = (0..mm.len())
            .filter(|&v| {
                let row = &data.vertex_bone_weights[v * k..(v + 1) * k];
                let best = (0..k).fold(0, |b, j| if row[j] > row[b] { j } else { b });
                upper_l.contains(&(data.vertex_bone_indices[v * k + best] as usize))
            })
            .map(|v| axial(mm[v], arm_l.origin, arm_l.up).0)
            .collect();
        if radii_l.is_empty() {
            return Err(none("left upper-arm"));
        }
        radii_l.sort_by(f64::total_cmp);
        let upper_arm_radius_l = radii_l[radii_l.len() / 2];
        let tip_h_l = 0.8 * upper_arm_radius_l;
        let arm_length_l = (head("wrist.L")? - shoulder_l).norm() + tip_h_l;
        // Leg.
        let hip = head("upperleg01.R")?;
        let ankle = head("foot.R")?;
        let mut leg = orthonormal(hip - ankle);
        leg.origin = Vec3::new(hip.x, y_c, crotch_z + 25.0);
        let hip_l = head("upperleg01.L")?;
        let ankle_l = head("foot.L")?;
        let knee_l = head("lowerleg01.L")?;
        let mut leg_l = orthonormal(hip_l - ankle_l);
        leg_l.origin = Vec3::new(hip_l.x, y_c, crotch_z + 25.0);
        // Torso.
        let torso = Frame {
            origin: Vec3::new(0.0, y_c, floor_z),
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        };
        let knee_z = head("lowerleg01.R")?.z;
        let shoulder_z = (shoulder + arm.up * tip_h).z;
        let snp_z = head("neck01")?.z;
        // Neck: up the neck01 bone's line to the head bone.
        let head_bone = head("head")?;
        let mut neck = orthonormal(head_bone - head("neck01")?);
        neck.origin = head("neck01")?;
        let landmarks: BTreeMap<String, f64> = [
            ("floor", floor_z),
            ("ankle", ankle.z),
            ("calf", ankle.z + 0.6 * (knee_z - ankle.z)),
            ("knee", knee_z),
            ("crotch", crotch_z),
            ("hip", crotch_z + 0.45 * (waist_z - crotch_z)),
            ("waist", waist_z),
            ("chest", armpit_z),
            ("across_back", 0.5 * (armpit_z + shoulder_z)),
            ("shoulder", shoulder_z),
            ("snp", snp_z),
            ("c7", snp_z + 5.0),
            ("head_top", top_z),
        ]
        .into_iter()
        .map(|(n, z)| (n.to_string(), z - floor_z))
        .collect();
        let pick = |want: Region| -> Vec<Vec3> {
            mm.iter()
                .zip(&region)
                .filter(|(_, r)| **r == want)
                .map(|(p, _)| *p)
                .collect()
        };
        Ok(Self {
            torso,
            arm_r: arm,
            arm_l,
            leg_r: leg,
            leg_l,
            floor_z,
            tip_h,
            tip_h_l,
            upper_arm_radius,
            upper_arm_radius_l,
            arm_length,
            arm_length_l,
            landmarks,
            neck,
            head_bone,
            shoulder_joint_r: shoulder,
            torso_points: pick(Region::Torso),
            leg_points: pick(Region::LegR),
            leg_points_l: pick(Region::LegL),
            leg_landmarks_l: [
                ankle_l.z - floor_z,
                ankle_l.z + 0.6 * (knee_l.z - ankle_l.z) - floor_z,
                knee_l.z - floor_z,
            ],
        })
    }

    /// The frame of `part` (`torso`, `arm_R`, `arm_L`, `leg_R`, `leg_L`).
    #[must_use]
    pub fn frame(&self, part: &str) -> Option<Frame> {
        match part {
            "torso" => Some(self.torso),
            "arm_R" => Some(self.arm_r),
            "arm_L" => Some(self.arm_l),
            "leg_R" => Some(self.leg_r),
            "leg_L" => Some(self.leg_l),
            _ => None,
        }
    }

    /// Height coordinate of a landmark in a part frame (torso: above the floor; limbs: along `up` from
    /// the origin), as the reference's `landmark_h`.
    #[must_use]
    pub fn landmark_h(&self, part: &str, name: &str) -> Option<f64> {
        match part {
            "torso" => self.landmarks.get(name).copied(),
            "arm_R" => match name {
                "shoulder_tip" => Some(self.tip_h),
                "elbow" => Some(self.tip_h - 0.55 * self.arm_length),
                "wrist" => Some(self.tip_h - self.arm_length),
                _ => None,
            },
            "arm_L" => match name {
                "shoulder_tip" => Some(self.tip_h_l),
                "elbow" => Some(self.tip_h_l - 0.55 * self.arm_length_l),
                "wrist" => Some(self.tip_h_l - self.arm_length_l),
                _ => None,
            },
            "leg_R" => {
                let z = self.floor_z + self.landmarks.get(name)?;
                Some(-(self.leg_r.origin.z - z) / self.leg_r.up.z)
            }
            "leg_L" => {
                let h = match name {
                    "ankle" => self.leg_landmarks_l[0],
                    "calf" => self.leg_landmarks_l[1],
                    "knee" => self.leg_landmarks_l[2],
                    _ => *self.landmarks.get(name)?,
                };
                let z = self.floor_z + h;
                Some(-(self.leg_l.origin.z - z) / self.leg_l.up.z)
            }
            _ => None,
        }
    }

    /// Largest distance from the part axis to the body near height `h` (mm; see the module table).
    #[must_use]
    pub fn section_radius(&self, part: &str, h: f64) -> f64 {
        const SLAB: f64 = 10.0;
        match part {
            "torso" => {
                let z = self.floor_z + h;
                self.torso_points
                    .iter()
                    .filter(|p| (p.z - z).abs() <= SLAB)
                    .map(|p| (p.x * p.x + (p.y - self.torso.origin.y).powi(2)).sqrt())
                    .fold(0.0, f64::max)
            }
            "arm_R" => 1.15 * self.upper_arm_radius,
            "arm_L" => 1.15 * self.upper_arm_radius_l,
            "leg_R" | "leg_L" => {
                let left = part == "leg_L";
                let (f, points) = if left {
                    (&self.leg_l, &self.leg_points_l)
                } else {
                    (&self.leg_r, &self.leg_points)
                };
                let mut r = points
                    .iter()
                    .map(|p| axial(*p, f.origin, f.up))
                    .filter(|(_, ph)| (ph - h).abs() <= SLAB)
                    .map(|(d, _)| d)
                    .fold(0.0, f64::max);
                let z = f.origin.z + h * f.up.z;
                if z > f.origin.z - 20.0 {
                    r = self
                        .torso_points
                        .iter()
                        .filter(|p| {
                            (if left { p.x >= 0.0 } else { p.x <= 0.0 }) && (p.z - z).abs() <= SLAB
                        })
                        .map(|p| axial(*p, f.origin, f.up).0)
                        .fold(r, f64::max);
                }
                r
            }
            _ => 0.0,
        }
    }
}

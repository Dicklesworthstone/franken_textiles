//! The Anny forward model (spec 05 §2–6): phenotype and local-change coefficients, the rest shape and
//! skeleton, posing and anthropometry. Batch size 1 (a deliberate narrowing of the package's batched API).

use std::path::Path;

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3};

use crate::data::AnnyData;
use crate::kinematics::{
    Affine, SkinningMode, dual_quaternion_skinning, forward_kinematics, linear_blend_skinning,
    rest_bone_frames,
};

/// The six exposed phenotype parameters. Each is on its anchor scale (spec 05 §2.1): gender 0 = male,
/// 1 = female; age is morphological, −1/3 (newborn anchor) … 1 (old), with 0 = birth, 0.05 = 1 y,
/// 0.215 = 4 y, 0.7857 ≈ 30 y (see [`age_parameter_for_years`]); the others 0 … 1. Values outside the
/// anchors are clamped, as the package does by default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phenotype {
    /// 0 male … 1 female.
    pub gender: f64,
    /// Morphological age parameter.
    pub age: f64,
    /// 0 min … 1 max muscle.
    pub muscle: f64,
    /// 0 min … 1 max weight.
    pub weight: f64,
    /// 0 min … 1 max height.
    pub height: f64,
    /// 0 ideal … 1 uncommon proportions.
    pub proportions: f64,
}

impl Default for Phenotype {
    fn default() -> Self {
        Self {
            gender: 0.5,
            age: 0.5,
            muscle: 0.5,
            weight: 0.5,
            height: 0.5,
            proportions: 0.5,
        }
    }
}

impl Phenotype {
    fn value(&self, feature: &str) -> Option<f64> {
        Some(match feature {
            "gender" => self.gender,
            "age" => self.age,
            "muscle" => self.muscle,
            "weight" => self.weight,
            "height" => self.height,
            "proportions" => self.proportions,
            _ => return None,
        })
    }
}

/// The package's WHO-calibrated map from years to the age parameter (spec 05 §7.1): piecewise linear,
/// extrapolated beyond both ends, the same for both sexes.
#[must_use]
pub fn age_parameter_for_years(years: f64) -> f64 {
    const YEARS: [f64; 8] = [0.0, 1.0, 4.0, 11.0, 16.0, 18.0, 64.0, 110.0];
    const PARAM: [f64; 8] = [0.0, 0.05, 0.215, 0.415, 0.67, 0.77, 0.83, 1.0];
    let i = YEARS
        .partition_point(|&y| y < years)
        .clamp(1, YEARS.len() - 1);
    let a = (years - YEARS[i - 1]) / (YEARS[i] - YEARS[i - 1]);
    PARAM[i - 1] + a * (PARAM[i] - PARAM[i - 1])
}

/// Weights of the anchors for a value (spec 05 §2.2): clamp, find the segment by a left search, split
/// linearly. Exactly two neighbouring weights are non-zero and they sum to 1.
#[must_use]
pub fn anchor_weights(x: f64, anchors: &[f64]) -> Vec<f64> {
    let n = anchors.len();
    let mut w = vec![0.0; n];
    if n == 0 {
        return w;
    }
    if x.is_nan() {
        return vec![f64::NAN; n];
    }
    if n == 1 {
        w[0] = 1.0;
        return w;
    }
    let x = x.max(anchors[0]).min(anchors[n - 1]);
    let i = anchors.partition_point(|&a| a < x).clamp(1, n - 1);
    let alpha = (x - anchors[i - 1]) / (anchors[i] - anchors[i - 1]);
    w[i - 1] = 1.0 - alpha;
    w[i] = alpha;
    w
}

/// Body parameters: the phenotype and the local changes (length L, label order; 0 = none).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyParams {
    /// Phenotype.
    pub phenotype: Phenotype,
    /// Local-change values in label order; ±1 is the authoring range, beyond it extrapolates.
    pub local_changes: Vec<f64>,
}

/// The rest (A-pose) body: the template frame, metres, z up, facing −y; soles near z = −0.82 m.
#[derive(Clone, Debug)]
pub struct RestBody {
    /// Rest vertices (V).
    pub vertices: Vec<Vec3>,
    /// Rest bone heads (J).
    pub bone_heads: Vec<Vec3>,
    /// Rest bone tails (J).
    pub bone_tails: Vec<Vec3>,
    /// Rest pose of each bone (J).
    pub bone_frames: Vec<Affine>,
}

/// A posed body: root-relative (the root head is at the origin under the identity root delta).
#[derive(Clone, Debug)]
pub struct PosedBody {
    /// Skinned vertices (V).
    pub vertices: Vec<Vec3>,
    /// World pose of each bone (J).
    pub bone_poses: Vec<Affine>,
    /// Skinning transform of each bone (J).
    pub transforms: Vec<Affine>,
}

/// Anthropometry of a rest body (spec 05 §6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anthropometry {
    /// Max z − min z over every vertex (m).
    pub height: f64,
    /// Length of the fixed 46-vertex waist loop (m); follows hollows, not a tape.
    pub waist_circumference: f64,
    /// Enclosed volume including the eyes and tongue (m³).
    pub volume: f64,
    /// 980 kg/m³ × volume.
    pub mass: f64,
    /// mass / height².
    pub bmi: f64,
}

/// The loaded model.
#[derive(Clone, Debug)]
pub struct AnnyModel {
    data: AnnyData,
    /// For each of the 26 components: index into the feature list and into that feature's anchors.
    components: Vec<(usize, usize)>,
    /// True only for CSR rows whose indices are exactly 0..vertices, in that order.
    dense_rows: Vec<bool>,
}

fn is_dense_row(indices: &[u32], vertices: usize) -> bool {
    indices.len() == vertices
        && indices
            .iter()
            .enumerate()
            .all(|(i, &vertex)| vertex as usize == i)
}

impl AnnyModel {
    /// Loads a converted model file.
    ///
    /// # Errors
    /// See [`AnnyData::read`].
    pub fn load(path: &Path, verify: bool) -> Result<Self, Refusal> {
        Self::from_data(AnnyData::read(path, verify)?)
    }

    /// Wraps already-read data.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if a component names an unknown feature.
    pub fn from_data(data: AnnyData) -> Result<Self, Refusal> {
        let meta = &data.header.phenotype;
        let mut components = Vec::with_capacity(meta.components.len());
        let mut seen: Vec<(String, usize)> = Vec::new();
        for (feature, _) in &meta.components {
            let f = meta
                .features
                .iter()
                .position(|x| x == feature)
                .ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!("phenotype component of unknown feature '{feature}'"),
                    )
                })?;
            let k = match seen.iter_mut().find(|(n, _)| n == feature) {
                Some((_, count)) => {
                    *count += 1;
                    *count - 1
                }
                None => {
                    seen.push((feature.clone(), 1));
                    0
                }
            };
            components.push((f, k));
        }
        let dense_rows = data
            .blendshape_row_ptr
            .windows(2)
            .map(|range| {
                data.blendshape_vertex
                    .get(range[0] as usize..range[1] as usize)
                    .is_some_and(|row| is_dense_row(row, data.header.counts.vertices))
            })
            .collect();
        Ok(Self {
            data,
            components,
            dense_rows,
        })
    }

    /// The underlying arrays and metadata.
    #[must_use]
    pub fn data(&self) -> &AnnyData {
        &self.data
    }

    /// Index of a bone label.
    #[must_use]
    pub fn bone_index(&self, label: &str) -> Option<usize> {
        self.data
            .header
            .labels
            .bones
            .iter()
            .position(|b| b == label)
    }

    /// Index of a local-change label.
    #[must_use]
    pub fn local_change_index(&self, label: &str) -> Option<usize> {
        self.data
            .header
            .labels
            .local_changes
            .iter()
            .position(|b| b == label)
    }

    /// Parameters with this phenotype and no local changes.
    #[must_use]
    pub fn params(&self, phenotype: Phenotype) -> BodyParams {
        BodyParams {
            phenotype,
            local_changes: vec![0.0; self.data.header.counts.local_changes],
        }
    }

    /// The S blend-shape coefficients (spec 05 §2.3, §3).
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] for a non-finite parameter; [`RefusalCode::OutOfRange`] when the
    /// local-change vector has the wrong length.
    pub fn coefficients(&self, p: &BodyParams) -> Result<Vec<f64>, Refusal> {
        let h = &self.data.header;
        let ph = &h.phenotype;
        let c = &h.counts;
        if p.local_changes.len() != c.local_changes {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} local-change values given; the model has {}",
                    p.local_changes.len(),
                    c.local_changes
                ),
            ));
        }
        Refusal::finite("fixed_value", ph.fixed_value)?;
        for f in &ph.exposed {
            Refusal::finite(f, p.phenotype.value(f).unwrap_or(ph.fixed_value))?;
        }
        for (j, v) in p.local_changes.iter().enumerate() {
            let label = h
                .labels
                .local_changes
                .get(j)
                .map_or("local_change", String::as_str);
            Refusal::finite(label, *v)?;
        }
        // Weight of every feature's anchors; race splits its fixed values evenly (v / Σv).
        let mut feature_weights: Vec<Vec<f64>> = Vec::with_capacity(ph.features.len());
        for f in &ph.features {
            if f == "race" {
                let v = ph.fixed_value;
                let n = ph.components.iter().filter(|(g, _)| g == "race").count();
                if n == 0 {
                    feature_weights.push(Vec::new());
                } else {
                    let sum = (0..n).fold(0.0, |acc, _| acc + v);
                    if !sum.is_finite() || sum == 0.0 {
                        return Err(Refusal::new(
                            RefusalCode::InvalidDocument,
                            "race phenotype fixed_value cannot be normalized",
                        ));
                    }
                    feature_weights.push(vec![v / sum; n]);
                }
            } else {
                let anchors = ph.anchors.get(f).ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::InvalidDocument,
                        format!("phenotype feature '{f}' has no anchors"),
                    )
                })?;
                let x = if ph.exposed.contains(f) {
                    p.phenotype.value(f).unwrap_or(ph.fixed_value)
                } else {
                    ph.fixed_value
                };
                feature_weights.push(anchor_weights(x, anchors));
            }
        }
        let mut w: Vec<f64> = Vec::with_capacity(self.components.len());
        for &(f, k) in &self.components {
            let weight = feature_weights
                .get(f)
                .and_then(|weights| weights.get(k))
                .copied()
                .ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::InvalidDocument,
                        "phenotype component index exceeds feature anchor count",
                    )
                })?;
            w.push(weight);
        }
        let ncomp = w.len();
        let mut coef = Vec::with_capacity(c.blendshapes);
        if ncomp > 0 {
            if c.phenotype_blendshapes.checked_mul(ncomp) != Some(self.data.phenotype_mask.len()) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "phenotype mask length does not match phenotype blendshape and component counts",
                ));
            }
            for row in self.data.phenotype_mask.chunks_exact(ncomp) {
                let mut prod = 1.0;
                for (m, &mask) in row.iter().enumerate() {
                    prod *= if mask == 1 { w[m] } else { 1.0 };
                }
                coef.push(prod);
            }
        } else if c.phenotype_blendshapes != 0 || !self.data.phenotype_mask.is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "phenotype blendshapes require at least one phenotype component",
            ));
        }
        for &v in &p.local_changes {
            coef.push(if v >= 0.0 { v } else { 0.0 });
            coef.push(if -v >= 0.0 { -v } else { 0.0 });
        }
        if coef.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "blendshape coefficients contain a non-finite value",
            ));
        }
        Ok(coef)
    }

    /// The rest body (spec 05 §4).
    ///
    /// # Errors
    /// As [`AnnyModel::coefficients`].
    pub fn rest(&self, p: &BodyParams) -> Result<RestBody, Refusal> {
        let d = &self.data;
        let c = &d.header.counts;
        let coef = self.coefficients(p)?;
        let expected_bone_bs = coef.len().checked_mul(c.bones);
        if d.template_vertices.len() != c.vertices
            || d.template_bone_heads.len() != c.bones
            || d.template_bone_tails.len() != c.bones
            || d.bone_rolls.len() != c.bones
            || d.blendshape_row_ptr.len() < coef.len() + 1
            || self.dense_rows.len() < coef.len()
            || expected_bone_bs.is_none_or(|n| {
                d.bone_heads_blendshapes.len() < n || d.bone_tails_blendshapes.len() < n
            })
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "body model template or blendshape tables do not match header counts",
            ));
        }
        // Σ c_k·B_k first, then the template (the package's `template + einsum`).
        let mut sum = vec![Vec3::ZERO; c.vertices];
        let mut head_sum = vec![Vec3::ZERO; c.bones];
        let mut tail_sum = vec![Vec3::ZERO; c.bones];
        for (k, &ck) in coef.iter().enumerate() {
            if ck == 0.0 {
                continue;
            }
            let (a, b) = (
                d.blendshape_row_ptr[k] as usize,
                d.blendshape_row_ptr[k + 1] as usize,
            );
            if a > b || b > d.blendshape_delta.len() || b > d.blendshape_vertex.len() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "blendshape row pointer exceeds table bounds",
                ));
            }
            if self.dense_rows[k] {
                // Identity indices preserve the sparse path's accumulation order exactly.
                for (value, &delta) in sum.iter_mut().zip(&d.blendshape_delta[a..b]) {
                    *value += delta * ck;
                }
            } else {
                for (&vi, &delta) in d.blendshape_vertex[a..b]
                    .iter()
                    .zip(&d.blendshape_delta[a..b])
                {
                    let slot = sum.get_mut(vi as usize).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::InvalidDocument,
                            "blendshape vertex index exceeds vertex count",
                        )
                    })?;
                    *slot += delta * ck;
                }
            }
            for j in 0..c.bones {
                head_sum[j] += d.bone_heads_blendshapes[k * c.bones + j] * ck;
                tail_sum[j] += d.bone_tails_blendshapes[k * c.bones + j] * ck;
            }
        }
        let vertices: Vec<Vec3> = d
            .template_vertices
            .iter()
            .zip(&sum)
            .map(|(&t, &s)| t + s)
            .collect();
        let bone_heads: Vec<Vec3> = d
            .template_bone_heads
            .iter()
            .zip(&head_sum)
            .map(|(&t, &s)| t + s)
            .collect();
        let bone_tails: Vec<Vec3> = d
            .template_bone_tails
            .iter()
            .zip(&tail_sum)
            .map(|(&t, &s)| t + s)
            .collect();
        let degenerate = Mat3::from_rows(d.header.degenerate_rotation);
        let bone_frames = rest_bone_frames(
            &bone_heads,
            &bone_tails,
            &d.bone_rolls,
            Vec3::from(d.header.y_axis),
            degenerate,
        );
        if vertices
            .iter()
            .chain(&bone_heads)
            .chain(&bone_tails)
            .any(|v| !v.is_finite())
            || bone_frames
                .iter()
                .any(|a| !a.t.is_finite() || a.m.rows.iter().flatten().any(|v| !v.is_finite()))
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "rest body evaluation produced a non-finite vertex or bone frame",
            ));
        }
        Ok(RestBody {
            vertices,
            bone_heads,
            bone_tails,
            bone_frames,
        })
    }

    /// Poses a rest body with one local-bone delta per bone (spec 05 §5.1) using upstream Anny
    /// linear blend skinning: the root's delta is its world pose; every other delta moves its bone
    /// about its own head in its rest frame.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] when `deltas` does not have one entry per bone;
    /// [`RefusalCode::NonFinite`] for a non-finite delta.
    pub fn pose(&self, rest: &RestBody, deltas: &[Affine]) -> Result<PosedBody, Refusal> {
        self.pose_with_skinning(rest, deltas, SkinningMode::LinearBlend)
    }

    /// Opt-in dual-quaternion skinning variant of [`AnnyModel::pose`] (plan §9.2): preserves local
    /// rigidity under large joint rotations and reduces LBS volume collapse at deep seated/squat
    /// bends. Never alters default LBS Anny parity.
    ///
    /// # Errors
    /// As [`AnnyModel::pose`].
    pub fn pose_dqs(&self, rest: &RestBody, deltas: &[Affine]) -> Result<PosedBody, Refusal> {
        self.pose_with_skinning(rest, deltas, SkinningMode::DualQuaternion)
    }

    /// Poses a rest body with an explicit [`SkinningMode`].
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] when `deltas` does not have one entry per bone;
    /// [`RefusalCode::NonFinite`] for a non-finite delta.
    pub fn pose_with_skinning(
        &self,
        rest: &RestBody,
        deltas: &[Affine],
        mode: SkinningMode,
    ) -> Result<PosedBody, Refusal> {
        let d = &self.data;
        let c = &d.header.counts;
        if deltas.len() != c.bones {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} bone deltas given; the model has {} bones",
                    deltas.len(),
                    c.bones
                ),
            ));
        }
        let width = c.vertices.checked_mul(c.influences);
        if rest.vertices.len() != c.vertices
            || rest.bone_frames.len() != c.bones
            || d.header.labels.bone_parents.len() != c.bones
            || width != Some(d.vertex_bone_indices.len())
            || width != Some(d.vertex_bone_weights.len())
            || d.header
                .labels
                .bone_parents
                .iter()
                .enumerate()
                .any(|(j, &p)| p >= 0 && p as u64 >= j as u64)
            || d.vertex_bone_indices.iter().any(|&b| b as usize >= c.bones)
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "rest body dimensions, bone hierarchy, or skinning tables do not match the body model",
            ));
        }
        let affine_finite =
            |a: &Affine| a.t.is_finite() && a.m.rows.iter().flatten().all(|v| v.is_finite());
        if deltas.iter().any(|a| !affine_finite(a))
            || rest.bone_frames.iter().any(|a| !affine_finite(a))
            || rest.vertices.iter().any(|v| !v.is_finite())
            || d.vertex_bone_weights.iter().any(|w| !w.is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "pose input holds a non-finite number",
            ));
        }
        let (bone_poses, transforms) =
            forward_kinematics(&rest.bone_frames, deltas, &d.header.labels.bone_parents);
        let vertices = match mode {
            SkinningMode::LinearBlend => linear_blend_skinning(
                &rest.vertices,
                &d.vertex_bone_indices,
                &d.vertex_bone_weights,
                c.influences,
                &transforms,
            ),
            SkinningMode::DualQuaternion => dual_quaternion_skinning(
                &rest.vertices,
                &d.vertex_bone_indices,
                &d.vertex_bone_weights,
                c.influences,
                &transforms,
            ),
        };
        if vertices.iter().any(|v| !v.is_finite())
            || bone_poses
                .iter()
                .chain(&transforms)
                .any(|a| !affine_finite(a))
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                format!(
                    "{} skinning produced a non-finite vertex or bone transform",
                    mode.as_str()
                ),
            ));
        }
        Ok(PosedBody {
            vertices,
            bone_poses,
            transforms,
        })
    }

    /// Height, waist loop, volume, mass and BMI of a rest body (spec 05 §6).
    #[must_use]
    pub fn anthropometry(&self, rest: &RestBody) -> Anthropometry {
        let v = &rest.vertices;
        let (lo, hi) = v
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.z), hi.max(p.z))
            });
        let height = hi - lo;
        let loop_ = &self.data.header.waist_loop_final;
        let n = loop_.len();
        let waist = (0..n)
            .map(|i| (v[loop_[(i + n - 1) % n]] - v[loop_[i]]).norm())
            .sum::<f64>();
        let volume = self
            .data
            .faces
            .iter()
            .map(|f| {
                let (a, b, c) = (v[f[0] as usize], v[f[1] as usize], v[f[2] as usize]);
                a.cross(b).dot(c) / 6.0
            })
            .sum::<f64>()
            .abs();
        let mass = 980.0 * volume;
        Anthropometry {
            height,
            waist_circumference: waist,
            volume,
            mass,
            bmi: mass / (height * height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_rows_require_the_complete_identity_sequence() {
        assert!(is_dense_row(&[0, 1, 2, 3], 4));
        for row in [
            &[][..],
            &[0, 1, 2],
            &[0, 2, 3],
            &[1, 2, 3, 4],
            &[0, 2, 1, 3],
            &[0, 1, 1, 3],
            &[0, 1, 2, 3, 3],
        ] {
            assert!(!is_dense_row(row, 4), "{row:?}");
        }
    }

    #[test]
    fn anchor_weights_follow_the_package_rules() {
        let a5 = [-1.0 / 3.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
        assert_eq!(anchor_weights(0.0, &a5), vec![0.0, 1.0, 0.0, 0.0, 0.0]);
        assert_eq!(anchor_weights(-1.0, &a5), vec![1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(anchor_weights(2.0, &a5), vec![0.0, 0.0, 0.0, 0.0, 1.0]);
        let w = anchor_weights(0.1325, &a5);
        assert!(
            (w[1] - 0.6025).abs() < 1e-15 && (w[2] - 0.3975).abs() < 1e-15,
            "{w:?}"
        );
        let a3 = [0.0, 0.5, 1.0];
        assert_eq!(anchor_weights(0.5, &a3), vec![0.0, 1.0, 0.0]);
        assert_eq!(anchor_weights(0.25, &[0.0, 1.0]), vec![0.75, 0.25]);
    }

    #[test]
    fn who_age_map() {
        for (y, p) in [
            (0.0, 0.0),
            (1.0, 0.05),
            (4.0, 0.215),
            (11.0, 0.415),
            (16.0, 0.67),
            (110.0, 1.0),
        ] {
            assert!((age_parameter_for_years(y) - p).abs() < 1e-15, "{y}");
        }
        assert!((age_parameter_for_years(2.5) - 0.1325).abs() < 1e-15);
        assert!((age_parameter_for_years(30.0) - 0.7857).abs() < 1e-4);
        assert!(
            age_parameter_for_years(-1.0) < 0.0,
            "extrapolates below birth"
        );
    }

    #[test]
    fn anchor_weights_propagates_nan_instead_of_clamping() {
        let w = anchor_weights(f64::NAN, &[0.0, 0.5, 1.0]);
        assert_eq!(w.len(), 3);
        assert!(w.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn zero_component_model_rest_and_pose_validate_and_fail_closed() {
        let data = AnnyData {
            header: serde_json::from_value(serde_json::json!({
                "schema":"ftxbody/1", "model":"anny", "anny_version":"0.6.1",
                "configuration":"synthetic", "licence":"synthetic", "package_sha256":{},
                "counts":{"vertices":2,"faces":0,"bones":2,"blendshapes":2,
                    "phenotype_blendshapes":0,"local_changes":1,"influences":1},
                "phenotype":{"features":[],"components":[],"anchors":{},"exposed":[],"fixed_value":0.5},
                "labels":{"blendshapes":["lc_pos","lc_neg"],"bones":["root","child"],
                    "bone_parents":[-1,0],"local_changes":["lc"]},
                "y_axis":[0,1,0], "degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],
                "waist_loop_final":[], "arrays":{}
            }))
            .unwrap(),
            template_vertices: vec![Vec3::ZERO, Vec3::new(0.1, 0.2, 0.3)],
            faces: vec![],
            blendshape_row_ptr: vec![0, 1, 1],
            blendshape_vertex: vec![1],
            blendshape_delta: vec![Vec3::new(1e300, 0.0, 0.0)],
            phenotype_mask: vec![],
            template_bone_heads: vec![Vec3::ZERO, Vec3::new(0.0, 0.0, 0.5)],
            template_bone_tails: vec![Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.0, 0.0, 1.0)],
            bone_heads_blendshapes: vec![Vec3::ZERO; 4],
            bone_tails_blendshapes: vec![Vec3::ZERO; 4],
            bone_rolls: vec![Mat3::IDENTITY; 2],
            vertex_bone_indices: vec![0, 0],
            vertex_bone_weights: vec![1.0, 1.0],
            base_mesh_vertex_indices: vec![0, 1],
        };
        let model = AnnyModel::from_data(data).unwrap();
        // Zero phenotype components must not panic in chunks_exact(0).
        let zero_params = model.params(Phenotype::default());
        let rest = model.rest(&zero_params).unwrap();
        assert_eq!(rest.vertices.len(), 2);

        // Overflowing local_change refuses NonFinite instead of returning INF vertices.
        let mut big = zero_params;
        big.local_changes[0] = 1e20;
        assert_eq!(model.rest(&big).unwrap_err().code, RefusalCode::NonFinite);

        // Mismatched RestBody refuses InvalidDocument instead of panicking.
        let mut short_rest = rest.clone();
        short_rest.bone_frames.pop();
        assert_eq!(
            model
                .pose(&short_rest, &[Affine::IDENTITY; 2])
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        // Non-finite rest bone frame refuses NonFinite even on an unweighted child bone.
        let mut nan_rest = rest.clone();
        nan_rest.bone_frames[1].t.x = f64::NAN;
        assert_eq!(
            model
                .pose(&nan_rest, &[Affine::IDENTITY; 2])
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );

        // Overflowing bone transform on an unweighted child bone refuses NonFinite.
        let mut huge_deltas = [Affine::IDENTITY; 2];
        huge_deltas[1].t = Vec3::new(1e308, 0.0, 0.0);
        huge_deltas[0].t = Vec3::new(1e308, 0.0, 0.0);
        assert_eq!(
            model.pose(&rest, &huge_deltas).unwrap_err().code,
            RefusalCode::NonFinite
        );

        // Truncated phenotype_mask with ncomp > 0 refuses InvalidDocument instead of silently omitting blendshapes.
        let mut bad_mask = model.data().clone();
        bad_mask.header.counts.phenotype_blendshapes = 2;
        bad_mask.header.phenotype.features = vec!["gender".into()];
        bad_mask.header.phenotype.components = vec![("gender".into(), "f".into())];
        bad_mask
            .header
            .phenotype
            .anchors
            .insert("gender".into(), vec![0.0]);
        bad_mask.header.phenotype.exposed = vec!["gender".into()];
        bad_mask.phenotype_mask = vec![1]; // expected 2 * 1 = 2 entries
        let bad_mask_model = AnnyModel::from_data(bad_mask).unwrap();
        let params = bad_mask_model.params(Phenotype::default());
        assert_eq!(
            bad_mask_model.coefficients(&params).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
}

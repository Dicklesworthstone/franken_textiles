//! The Anny forward model (spec 05 §2–6): phenotype and local-change coefficients, the rest shape and
//! skeleton, posing and anthropometry. Batch size 1 (a deliberate narrowing of the package's batched API).

use std::path::Path;

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3};

use crate::data::AnnyData;
use crate::kinematics::{Affine, forward_kinematics, linear_blend_skinning, rest_bone_frames};

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
        Ok(Self { data, components })
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
        for f in &ph.exposed {
            Refusal::finite(f, p.phenotype.value(f).unwrap_or(ph.fixed_value))?;
        }
        for (j, v) in p.local_changes.iter().enumerate() {
            Refusal::finite(&h.labels.local_changes[j], *v)?;
        }
        // Weight of every feature's anchors; race splits its fixed values evenly (v / Σv).
        let feature_weights: Vec<Vec<f64>> = ph
            .features
            .iter()
            .map(|f| {
                if f == "race" {
                    let v = ph.fixed_value;
                    let n = ph.components.iter().filter(|(g, _)| g == "race").count();
                    let sum = (0..n).fold(0.0, |acc, _| acc + v);
                    vec![v / sum; n]
                } else {
                    let x = if ph.exposed.contains(f) {
                        p.phenotype.value(f).unwrap_or(ph.fixed_value)
                    } else {
                        ph.fixed_value
                    };
                    anchor_weights(x, &ph.anchors[f])
                }
            })
            .collect();
        let w: Vec<f64> = self
            .components
            .iter()
            .map(|&(f, k)| feature_weights[f][k])
            .collect();
        let ncomp = w.len();
        let mut coef = Vec::with_capacity(c.blendshapes);
        for row in self.data.phenotype_mask.chunks_exact(ncomp) {
            let mut prod = 1.0;
            for (m, &mask) in row.iter().enumerate() {
                prod *= if mask == 1 { w[m] } else { 1.0 };
            }
            coef.push(prod);
        }
        for &v in &p.local_changes {
            coef.push(if v >= 0.0 { v } else { 0.0 });
            coef.push(if -v >= 0.0 { -v } else { 0.0 });
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
            for (&vi, &delta) in d.blendshape_vertex[a..b]
                .iter()
                .zip(&d.blendshape_delta[a..b])
            {
                sum[vi as usize] += delta * ck;
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
        Ok(RestBody {
            vertices,
            bone_heads,
            bone_tails,
            bone_frames,
        })
    }

    /// Poses a rest body with one local-bone delta per bone (spec 05 §5.1): the root's delta is its world
    /// pose; every other delta moves its bone about its own head in its rest frame.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] when `deltas` does not have one entry per bone;
    /// [`RefusalCode::NonFinite`] for a non-finite delta.
    pub fn pose(&self, rest: &RestBody, deltas: &[Affine]) -> Result<PosedBody, Refusal> {
        let d = &self.data;
        if deltas.len() != d.header.counts.bones {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} bone deltas given; the model has {} bones",
                    deltas.len(),
                    d.header.counts.bones
                ),
            ));
        }
        if deltas
            .iter()
            .any(|a| !a.t.is_finite() || a.m.rows.iter().flatten().any(|v| !v.is_finite()))
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "a bone delta holds a non-finite number",
            ));
        }
        let (bone_poses, transforms) =
            forward_kinematics(&rest.bone_frames, deltas, &d.header.labels.bone_parents);
        let vertices = linear_blend_skinning(
            &rest.vertices,
            &d.vertex_bone_indices,
            &d.vertex_bone_weights,
            d.header.counts.influences,
            &transforms,
        );
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
}

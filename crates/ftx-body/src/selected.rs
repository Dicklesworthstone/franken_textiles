//! Selected-vertex evaluation of the existing Anny skinning model (spec 05 §5).
//! Keep the full skeleton but skin only requested surface points. This is not a reduced body model.

use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::{
    Affine, AnnyModel, RestBody,
    kinematics::{forward_kinematics, linear_blend_skinning},
};

/// A sparse pose, deliberately distinct from `PosedBody`: it cannot stand in for a full skin mesh.
#[derive(Clone, Debug)]
pub struct SelectedPose {
    /// Original model indices, in caller order, including repetitions.
    pub vertex_indices: Vec<usize>,
    /// Skinned points in the same order, metres in the root-relative posed frame.
    pub vertices: Vec<Vec3>,
    /// Complete world-space bone poses; useful for bone-head correspondences.
    pub bone_poses: Vec<Affine>,
    /// Complete skinning transforms; useful for bone-tail correspondences.
    pub transforms: Vec<Affine>,
}

fn finite(a: &Affine) -> bool {
    a.t.is_finite() && a.m.rows.iter().flatten().all(|v| v.is_finite())
}

impl AnnyModel {
    /// Evaluate selected surface vertices and the full skeleton with the same operation order as
    /// [`Self::pose`]. An empty selection computes the skeleton only. Unselected rest vertices are
    /// neither skinned nor inspected; this API does not validate a full mesh for collision.
    ///
    /// # Errors
    /// Refuses malformed dimensions, indices, hierarchy, non-finite participating values or overflow.
    pub fn pose_selected(
        &self,
        rest: &RestBody,
        deltas: &[Affine],
        selected: &[usize],
    ) -> Result<SelectedPose, Refusal> {
        let d = self.data();
        let c = &d.header.counts;
        let width = c.vertices.checked_mul(c.influences);
        if c.bones == 0
            || c.influences == 0
            || rest.vertices.len() != c.vertices
            || rest.bone_frames.len() != c.bones
            || rest.bone_heads.len() != c.bones
            || rest.bone_tails.len() != c.bones
            || deltas.len() != c.bones
            || d.header.labels.bone_parents.len() != c.bones
            || width != Some(d.vertex_bone_indices.len())
            || width != Some(d.vertex_bone_weights.len())
            || selected.iter().any(|&i| i >= c.vertices)
            || d.header
                .labels
                .bone_parents
                .iter()
                .enumerate()
                .any(|(j, &p)| {
                    if j == 0 {
                        p != -1
                    } else {
                        p < 0 || p as u64 >= j as u64
                    }
                })
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "selected pose needs valid model/rest dimensions, vertex indices and bone hierarchy",
            ));
        }
        if rest.bone_frames.iter().chain(deltas).any(|a| !finite(a))
            || selected.iter().any(|&i| !rest.vertices[i].is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "non-finite selected pose input",
            ));
        }
        // Gather influence rows, not an approximation to their weighted transform. Reusing the
        // full-pose kernel preserves its accumulation order, including zero-weight influences.
        let mut points = Vec::with_capacity(selected.len());
        let mut indices = Vec::new();
        let mut weights = Vec::new();
        for &i in selected {
            let row = i * c.influences..(i + 1) * c.influences;
            if d.vertex_bone_indices[row.clone()]
                .iter()
                .any(|&b| b as usize >= c.bones)
                || d.vertex_bone_weights[row.clone()]
                    .iter()
                    .any(|w| !w.is_finite())
            {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "invalid selected skinning influence",
                ));
            }
            points.push(rest.vertices[i]);
            indices.extend_from_slice(&d.vertex_bone_indices[row.clone()]);
            weights.extend_from_slice(&d.vertex_bone_weights[row]);
        }
        let (bone_poses, transforms) =
            forward_kinematics(&rest.bone_frames, deltas, &d.header.labels.bone_parents);
        let vertices =
            linear_blend_skinning(&points, &indices, &weights, c.influences, &transforms);
        if vertices.iter().any(|p| !p.is_finite())
            || bone_poses.iter().chain(&transforms).any(|a| !finite(a))
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "selected pose overflowed",
            ));
        }
        Ok(SelectedPose {
            vertex_indices: selected.to_vec(),
            vertices,
            bone_poses,
            transforms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnnyData;
    use ftx_core::Mat3;
    use serde_json::json;

    fn fixture() -> (AnnyModel, RestBody, Vec<Affine>) {
        // Numerical skinning fixture only: no anatomical assets or measurements.
        let vertices = vec![
            Vec3::ZERO,
            Vec3::new(0.2, 0.3, 0.4),
            Vec3::new(-0.5, 0.7, 0.9),
        ];
        let frames = vec![
            Affine::IDENTITY,
            Affine::from_rotvec(Vec3::new(0.1, -0.2, 0.3), Vec3::new(0.0, 0.0, 0.5)),
        ];
        let data = AnnyData {
            header: serde_json::from_value(json!({
                "schema":"ftxbody/1", "model":"anny", "anny_version":"0.6.1",
                "configuration":"synthetic skinning test", "licence":"synthetic", "package_sha256":{},
                "counts":{"vertices":3,"faces":0,"bones":2,"blendshapes":0,"phenotype_blendshapes":0,"local_changes":0,"influences":2},
                "phenotype":{"features":[],"components":[],"anchors":{},"exposed":[],"fixed_value":0.5},
                "labels":{"blendshapes":[],"bones":["root","child"],"bone_parents":[-1,0],"local_changes":[]},
                "y_axis":[0,1,0], "degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]], "waist_loop_final":[], "arrays":{}
            })).unwrap(),
            template_vertices: vertices.clone(), faces: vec![], blendshape_row_ptr: vec![0],
            blendshape_vertex: vec![], blendshape_delta: vec![], phenotype_mask: vec![],
            template_bone_heads: vec![Vec3::ZERO; 2], template_bone_tails: vec![Vec3::ZERO; 2],
            bone_heads_blendshapes: vec![], bone_tails_blendshapes: vec![], bone_rolls: vec![Mat3::IDENTITY; 2],
            vertex_bone_indices: vec![0,1,1,0,0,1], vertex_bone_weights: vec![1.0,0.0,0.3,0.7,0.55,0.45],
            base_mesh_vertex_indices: vec![0,1,2],
        };
        let rest = RestBody {
            vertices,
            bone_frames: frames,
            bone_heads: vec![Vec3::ZERO; 2],
            bone_tails: vec![Vec3::ZERO; 2],
        };
        let deltas = vec![
            Affine::from_rotvec(Vec3::new(0.2, -0.1, 0.5), Vec3::new(0.4, -0.3, 0.1)),
            Affine::IDENTITY,
        ];
        (AnnyModel::from_data(data).unwrap(), rest, deltas)
    }

    #[test]
    fn selection_matches_full_skinning_bit_for_bit_in_caller_order() {
        let (model, rest, mut deltas) = fixture();
        for angle in [-1.2, -0.01, 0.0, 0.7, 1.5] {
            deltas[1] = Affine::from_rotvec(Vec3::new(angle, -angle / 2.0, 0.1), Vec3::ZERO);
            let full = model.pose(&rest, &deltas).unwrap();
            let ids = [2, 0, 1, 2];
            let sparse = model.pose_selected(&rest, &deltas, &ids).unwrap();
            assert_eq!(sparse.vertex_indices, ids);
            assert_eq!(sparse.transforms, full.transforms);
            assert_eq!(sparse.bone_poses, full.bone_poses);
            for (p, &i) in sparse.vertices.iter().zip(&ids) {
                let a: [f64; 3] = (*p).into();
                let b: [f64; 3] = full.vertices[i].into();
                assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits));
            }
            let skeleton = model.pose_selected(&rest, &deltas, &[]).unwrap();
            assert!(skeleton.vertices.is_empty());
            assert_eq!(skeleton.bone_poses, full.bone_poses);
        }
    }

    #[test]
    fn invalid_selections_and_participating_geometry_refuse() {
        let (model, rest, mut deltas) = fixture();
        assert!(model.pose_selected(&rest, &deltas, &[3]).is_err());
        assert!(model.pose_selected(&rest, &deltas[..1], &[0]).is_err());
        let mut short = rest.clone();
        short.bone_frames.pop();
        assert!(model.pose_selected(&short, &deltas, &[0]).is_err());
        let mut bad = rest.clone();
        bad.vertices[2].x = f64::NAN;
        assert!(model.pose_selected(&bad, &deltas, &[2]).is_err());
        assert!(model.pose_selected(&bad, &deltas, &[0]).is_ok());
        deltas[1].m.rows[0][0] = f64::INFINITY;
        assert!(model.pose_selected(&rest, &deltas, &[]).is_err());
    }

    #[test]
    fn malformed_influence_tables_and_hierarchies_refuse_without_panics() {
        let (model, rest, deltas) = fixture();
        for kind in 0..4 {
            let mut d = model.data().clone();
            match kind {
                0 => d.vertex_bone_indices[0] = 99,
                1 => d.vertex_bone_weights[0] = f64::NAN,
                2 => d.header.labels.bone_parents[1] = 1,
                _ => {
                    d.vertex_bone_weights.pop();
                }
            }
            let broken = AnnyModel::from_data(d).unwrap();
            assert!(broken.pose_selected(&rest, &deltas, &[0]).is_err());
        }
    }
}

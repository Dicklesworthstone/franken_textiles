//! Evaluate one synthetic wardrobe case with the same native fit and pose functions as ftx body.
//! Unpacked positions and the model stay in the generator's private working directory.
use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, Pose, Wearer, fit_tape, pose_deltas, skin_mesh, tape_targets,
};
use serde_json::{Value, json};
use std::error::Error;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let model = AnnyModel::from_data(AnnyData::read(Path::new(&args[1]), true)?)?;
    let doc: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let out = Path::new(&args[3]);
    let fit = fit_tape(
        &model,
        &Wearer::from_measurements(&doc)?,
        &tape_targets(&doc)?,
        &Pose::drape(),
    )?;
    let resting = model.rest(&fit.params)?;
    let data = model.data();
    let mut samples = Vec::new();
    let steps = if args[4] == "male" {
        vec![4]
    } else {
        vec![4, 35, 65, 95, 125]
    };
    let skin = skin_mesh(&resting.vertices, &data.faces)?;
    fs::write(
        out.join("topology.json"),
        serde_json::to_vec(&json!({
            "faces": data.faces, "skin_order": skin.source_vertex, "skin_faces": skin.triangles
        }))?,
    )?;
    let mut labels = vec![];
    for vertex in 0..resting.vertices.len() {
        let k = data.header.counts.influences;
        let first = vertex * k;
        let strongest = (0..k)
            .max_by(|a, b| {
                data.vertex_bone_weights[first + a].total_cmp(&data.vertex_bone_weights[first + b])
            })
            .unwrap();
        let mut bone = data.vertex_bone_indices[first + strongest] as usize;
        let mut region = 0_u8;
        loop {
            let label = data.header.labels.bones[bone].as_str();
            match label {
                "wrist.L" => {
                    region = 3;
                    break;
                }
                "wrist.R" => {
                    region = 4;
                    break;
                }
                "upperarm01.L" => {
                    region = 1;
                    break;
                }
                "upperarm01.R" => {
                    region = 2;
                    break;
                }
                "head" => {
                    region = 5;
                    break;
                }
                _ => {}
            }
            let Ok(parent) = usize::try_from(data.header.labels.bone_parents[bone]) else {
                break;
            };
            bone = parent;
        }
        labels.push(region);
    }
    fs::write(out.join("regions.bin"), labels)?;
    for deg in steps {
        let mut pose = Pose::a_pose(f64::from(deg));
        pose.straight_arms = true;
        let posed = model.pose(&resting, &pose_deltas(&model, &resting, &pose, 1.)?)?;
        let frames = BodyFrames::new(&model, &posed)?;
        let floor = frames.floor_z * 0.001;
        let mut bytes = Vec::new();
        for v in &posed.vertices {
            for cm in [v.x * 100., (v.z - floor) * 100., -v.y * 100.] {
                bytes.extend((cm as f32).to_le_bytes());
            }
        }
        fs::write(out.join(format!("{deg}.f32")), bytes)?;
        let mut marks = serde_json::Map::new();
        for (id, bone) in [
            ("shoulderLeft", "upperarm01.L"),
            ("shoulderRight", "upperarm01.R"),
            ("wristLeft", "wrist.L"),
            ("wristRight", "wrist.R"),
            ("neck", "neck01"),
        ] {
            let index = model.bone_index(bone).ok_or("missing landmark bone")?;
            let v = posed.bone_poses[index].t;
            marks.insert(
                id.into(),
                json!([v.x * 100., (v.z - floor) * 100., -v.y * 100.]),
            );
        }
        for (id, key) in [
            ("headTop", "head_top"),
            ("waist", "waist"),
            ("chest", "chest"),
            ("hip", "hip"),
        ] {
            marks.insert(id.into(), json!([0., frames.landmarks[key] / 10., 0.]));
        }
        samples.push(json!({"degrees":deg,"landmarks":marks}));
    }
    let outcomes: Value = fit
        .residuals
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                json!({
                    "target_mm":r.target_mm,"body_mm":r.body_mm,"residual_mm":r.body_mm-r.target_mm
                }),
            )
        })
        .collect();
    fs::write(
        out.join("fit.json"),
        serde_json::to_vec(&json!({
            "poses":samples,"tapes":outcomes,"mass_kg":fit.mass_kg,"iterations":fit.iterations
        }))?,
    )?;
    Ok(())
}

//! Bake one synthetic body for the browser's posing session, so that a page can drape and pose without the
//! converted body model: the frames and landmarks a drape places its pieces by, the surface the cloth
//! collides with, the surface the page shows, and each arm sampled at a list of angles.
//!
//!   bake-posable-body MODEL.ftxbody MEASUREMENTS.json NEW_OUT_DIR BASE_DEG ANGLE,ANGLE,...
//!
//! The body is fitted and posed by the same functions as `ftx drape` (tape fit, nappy occupancy, linear
//! blend skinning, the wrist-trimmed clothing skin). An arm angle whose collision surface crosses itself is
//! left out and named in the header. Writes `posable.json` (header) and `posable.bin` (little-endian
//! arrays at the header's offsets). Positions are single precision: a browser drape on this body is the
//! same solver on a rounded body, not a bit-for-bit copy of a native run. Never run on a real person's
//! measurements.
use ftx_body::drape::ArmPose;
use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, DEFAULT_POSE_AUDIT_PAIR_BUDGET, HandCollider, Pose, Wearer,
    clothing_skin_with_hands, fit_tape, nappy_occupancy, pose_deltas, skin_mesh, tape_targets,
};
use ftx_core::Vec3;
use serde_json::{Map, Value, json};
use std::error::Error;
use std::fs;
use std::path::Path;

/// Part axes sampled for the section radius, as millimetres along each part's own height coordinate.
/// Wide enough that both ends lie beyond the body, where the radius is zero.
const RADIUS_RANGE_MM: (i32, i32) = (-1600, 2400);
const PARTS: [&str; 5] = ["torso", "arm_R", "arm_L", "leg_R", "leg_L"];

struct Blob {
    bytes: Vec<u8>,
    directory: Map<String, Value>,
}

impl Blob {
    fn floats(&mut self, name: &str, values: impl Iterator<Item = f64>) {
        let offset = self.bytes.len();
        let mut count = 0;
        for value in values {
            self.bytes.extend((value as f32).to_le_bytes());
            count += 1;
        }
        self.directory
            .insert(name.into(), json!({"dtype": "f32", "offset": offset, "count": count}));
    }

    fn indices(&mut self, name: &str, values: impl Iterator<Item = usize>) {
        let offset = self.bytes.len();
        let mut count = 0;
        for value in values {
            self.bytes
                .extend(u32::try_from(value).expect("index fits u32").to_le_bytes());
            count += 1;
        }
        self.directory
            .insert(name.into(), json!({"dtype": "u32", "offset": offset, "count": count}));
    }
}

fn xyz(points: &[Vec3]) -> impl Iterator<Item = f64> + '_ {
    points.iter().flat_map(|p| [p.x, p.y, p.z])
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: bake-posable-body MODEL MEASUREMENTS.json NEW_OUT_DIR BASE_DEG ANGLE,ANGLE,...".into());
    }
    let model = AnnyModel::from_data(AnnyData::read(Path::new(&args[1]), true)?)?;
    let measurements: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let out = Path::new(&args[3]);
    let base: f64 = args[4].parse()?;
    // The base angle is always a sample (with no displacement), so that the body the opening drape uses is
    // exactly the body a session returns to at that angle.
    let mut requested: Vec<f64> = args[5]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    requested.push(base);
    requested.sort_by(f64::total_cmp);
    requested.dedup();
    fs::create_dir(out)?;

    let fit = fit_tape(
        &model,
        &Wearer::from_measurements(&measurements)?,
        &tape_targets(&measurements)?,
        &Pose::drape(),
    )?;
    let occupancy = nappy_occupancy(&model, &fit.params, &measurements)?;
    let params = occupancy.as_ref().map_or(&fit.params, |o| &o.params);
    let rest = model.rest(params)?;
    let posed_at = |left: f64, right: f64| -> Result<_, Box<dyn Error>> {
        let mut pose = Pose::drape();
        pose.arm_abduction_deg = Some(base);
        pose.arm_pose = Some(ArmPose {
            left_abduction_deg: left,
            right_abduction_deg: right,
        });
        pose.validate()?;
        let posed = model.pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?)?;
        let collision = clothing_skin_with_hands(&model, &posed.vertices, HandCollider::Wrist)?;
        let display = skin_mesh(&posed.vertices, &model.data().faces)?;
        Ok((posed, collision, display))
    };

    let (posed, collision, display) = posed_at(base, base)?;
    let audit = collision.audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET)?;
    if audit.intersecting_pairs != 0 {
        return Err(format!("the base pose's collision surface crosses itself at {base} degrees").into());
    }
    let frames = BodyFrames::new(&model, &posed)?;
    let mut blob = Blob {
        bytes: Vec::new(),
        directory: Map::new(),
    };
    blob.floats("collision_vertices", xyz(&collision.vertices));
    blob.indices("collision_triangles", collision.triangles.iter().flatten().copied());
    blob.floats("display_vertices", xyz(&display.vertices));
    blob.indices("display_triangles", display.triangles.iter().flatten().copied());

    let mut part_frames = Map::new();
    let mut landmarks = Map::new();
    let mut names: Vec<&str> = frames.landmarks.keys().map(String::as_str).collect();
    names.extend(["shoulder_tip", "elbow", "wrist", "ankle", "calf", "knee"]);
    names.sort_unstable();
    names.dedup();
    for part in PARTS {
        let frame = frames.frame(part).ok_or("a part frame is missing")?;
        let vector = |v: Vec3| json!([v.x, v.y, v.z]);
        part_frames.insert(
            part.into(),
            json!({"origin": vector(frame.origin), "up": vector(frame.up), "front": vector(frame.front), "lat": vector(frame.lat)}),
        );
        let heights: Map<String, Value> = names
            .iter()
            .filter_map(|name| frames.landmark_h(part, name).map(|h| ((*name).to_string(), json!(h))))
            .collect();
        landmarks.insert(part.into(), Value::Object(heights));
        blob.floats(
            &format!("radius_{part}"),
            (RADIUS_RANGE_MM.0..=RADIUS_RANGE_MM.1).map(|h| frames.section_radius(part, f64::from(h))),
        );
    }

    // Each arm on its own, the other at the base angle. Under linear blend skinning a vertex's position is
    // linear in its bones' transforms and each arm's bones turn with that arm only, so the body at any
    // pair of sampled angles is the base plus the two arms' own displacements.
    let mut admitted = Vec::new();
    let mut refused = Vec::new();
    let mut samples = Vec::new();
    for &angle in &requested {
        let (_, left_c, left_d) = posed_at(angle, base)?;
        let (_, right_c, right_d) = posed_at(base, angle)?;
        let crossing = [&left_c, &right_c]
            .iter()
            .map(|skin| skin.audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|audit| audit.intersecting_pairs != 0);
        if crossing {
            refused.push(angle);
        } else {
            admitted.push(angle);
            samples.push([left_c.vertices, left_d.vertices, right_c.vertices, right_d.vertices]);
        }
    }
    if admitted.len() < 2 {
        return Err("fewer than two arm angles have a collision surface that does not cross itself".into());
    }
    for (side, first) in [("left", 0), ("right", 2)] {
        for (surface, base_points, which) in [
            ("collision", &collision.vertices, first),
            ("display", &display.vertices, first + 1),
        ] {
            let moved: Vec<usize> = (0..base_points.len())
                .filter(|&v| samples.iter().any(|sample| (sample[which][v] - base_points[v]).norm() > 1e-9))
                .collect();
            blob.indices(&format!("{side}_{surface}_indices"), moved.iter().copied());
            blob.floats(
                &format!("{side}_{surface}_offsets"),
                samples.iter().flat_map(|sample| {
                    let (points, moved) = (&sample[which], &moved);
                    moved.iter().flat_map(move |&v| {
                        let d = points[v] - base_points[v];
                        [d.x, d.y, d.z]
                    })
                }),
            );
        }
    }

    let header = json!({
        "schema": "ftx-posable-body/1",
        "note": "A synthetic body baked for the browser's posing session: frames in mm, surfaces in metres (z up, facing -y), single precision.",
        "base_arm_deg": base,
        "arm_angles_deg": admitted,
        "arm_angles_refused_deg": refused,
        "hands": "wrist",
        "skinning": "lbs",
        "floor_mm": frames.floor_z,
        "radius_h0_mm": RADIUS_RANGE_MM.0,
        "frames": part_frames,
        "landmarks": landmarks,
        "counts": {
            "collision_vertices": collision.vertices.len(), "collision_triangles": collision.triangles.len(),
            "display_vertices": display.vertices.len(), "display_triangles": display.triangles.len(),
        },
        "fit": {"iterations": fit.iterations, "mass_kg": fit.mass_kg},
        "arrays": blob.directory,
    });
    fs::write(out.join("posable.json"), serde_json::to_vec(&header)?)?;
    fs::write(out.join("posable.bin"), &blob.bytes)?;
    println!(
        "{} collision and {} display vertices, {} arm angles ({} refused), {} bytes",
        collision.vertices.len(),
        display.vertices.len(),
        admitted.len(),
        refused.len(),
        blob.bytes.len()
    );
    Ok(())
}

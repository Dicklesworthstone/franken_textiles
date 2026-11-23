//! Bake one synthetic body for the browser's posing session, so that a page can drape and pose without the
//! converted body model: the frames and landmarks a drape places its pieces by, the surface the cloth
//! collides with, the surface the page shows, and each arm sampled at a list of angles.
//!
//!   bake-posable-body MODEL.ftxbody MEASUREMENTS.json NEW_OUT_DIR BASE_DEG ANGLE,ANGLE,... [whole|upper:MM]
//!
//! The body is fitted and posed by the same functions as `ftx drape` (tape fit, nappy occupancy, linear
//! blend skinning, the wrist-trimmed clothing skin). `upper:MM` (for a garment that ends above the hip,
//! such as a jacket) removes the legs at the top of the thigh and cuts the trunk MM above the floor, on
//! both surfaces, closing the opening like a tailor's form; the arms are never cut. Below that plane the
//! cloth would find no body, so choose it below anything the garment reaches. `whole`, the default,
//! keeps the whole body. An arm angle whose collision surface crosses itself
//! is left out and named in the header. Writes `posable.json` (header) and `posable.bin` (little-endian
//! arrays at the header's offsets). Positions are single precision: a browser drape on this body is the
//! same solver on a rounded body, not a bit-for-bit copy of a native run. Never run on a real person's
//! measurements.
use ftx_body::drape::ArmPose;
use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, DEFAULT_POSE_AUDIT_PAIR_BUDGET, HandCollider, Pose, Wearer,
    clothing_skin_with_hands, fit_tape, nappy_occupancy, pose_deltas, skin_mesh, tape_targets,
    trimmed_skin,
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

/// A horizontal cut that keeps a closed surface's part above a plane and closes the opening. It is found
/// once, on the base pose, and applied with the same topology to every pose: only the arms move between
/// poses, and vertices marked `exempt` (the arms) are never cut, so every pose's surface has the same
/// vertices and triangles.
struct Cut {
    /// Source vertices kept, in source order; they come first in the output.
    kept: Vec<usize>,
    /// New vertices on the plane: an edge's two source vertices and the fraction of the way from the first.
    on_plane: Vec<(usize, usize, f64)>,
    /// For each closed opening, its new vertices (indices into `on_plane`); its centre is their mean.
    caps: Vec<Vec<usize>>,
    triangles: Vec<[usize; 3]>,
}

enum Corner {
    Source(usize),
    Plane(usize),
}

impl Cut {
    fn new(points: &[Vec3], triangles: &[[usize; 3]], z: f64, exempt: &[bool]) -> Result<Self, String> {
        let above = |v: usize| exempt[v] || points[v].z >= z;
        let mut on_plane = Vec::new();
        let mut plane_index = std::collections::BTreeMap::new();
        let mut polygons: Vec<Vec<Corner>> = Vec::new();
        for &face in triangles {
            let count = face.iter().filter(|&&v| above(v)).count();
            if count == 3 || face.iter().any(|&v| exempt[v]) {
                polygons.push(face.map(Corner::Source).into());
                continue;
            }
            if count == 0 {
                continue;
            }
            let mut polygon = Vec::new();
            for k in 0..3 {
                let (a, b) = (face[k], face[(k + 1) % 3]);
                if above(a) {
                    polygon.push(Corner::Source(a));
                }
                if above(a) != above(b) {
                    let (lo, hi) = (a.min(b), a.max(b));
                    let index = *plane_index.entry((lo, hi)).or_insert_with(|| {
                        let t = (z - points[lo].z) / (points[hi].z - points[lo].z);
                        on_plane.push((lo, hi, t));
                        on_plane.len() - 1
                    });
                    polygon.push(Corner::Plane(index));
                }
            }
            polygons.push(polygon);
        }
        let mut kept: Vec<usize> = polygons
            .iter()
            .flatten()
            .filter_map(|c| if let Corner::Source(v) = c { Some(*v) } else { None })
            .collect();
        kept.sort_unstable();
        kept.dedup();
        let number = |c: &Corner| match c {
            Corner::Source(v) => kept.binary_search(v).expect("a kept vertex"),
            Corner::Plane(p) => kept.len() + p,
        };
        let mut out = Vec::new();
        for polygon in &polygons {
            for i in 1..polygon.len() - 1 {
                out.push([number(&polygon[0]), number(&polygon[i]), number(&polygon[i + 1])]);
            }
        }
        // The opening's edges are those used in one direction only; each must lie on the plane.
        let directed: std::collections::BTreeSet<(usize, usize)> =
            out.iter().flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect();
        let mut next = std::collections::BTreeMap::new();
        for &(a, b) in &directed {
            if !directed.contains(&(b, a)) {
                if a < kept.len() || b < kept.len() {
                    return Err("the cut left an opening off the plane: move it away from the arms".into());
                }
                next.insert(a, b);
            }
        }
        let mut caps = Vec::new();
        while let Some((&first, _)) = next.iter().next() {
            let mut cap = Vec::new();
            let mut at = first;
            while let Some(to) = next.remove(&at) {
                cap.push(at - kept.len());
                at = to;
            }
            if at != first {
                return Err("an opening made by the cut is not a closed loop".into());
            }
            let centre = kept.len() + on_plane.len() + caps.len();
            for k in 0..cap.len() {
                let (a, b) = (kept.len() + cap[k], kept.len() + cap[(k + 1) % cap.len()]);
                out.push([b, a, centre]);
            }
            caps.push(cap);
        }
        Ok(Self { kept, on_plane, caps, triangles: out })
    }

    fn apply(&self, points: &[Vec3]) -> Vec<Vec3> {
        let mut out: Vec<Vec3> = self.kept.iter().map(|&v| points[v]).collect();
        let plane: Vec<Vec3> = self
            .on_plane
            .iter()
            .map(|&(a, b, t)| points[a] + (points[b] - points[a]) * t)
            .collect();
        out.extend(&plane);
        for cap in &self.caps {
            let sum = cap.iter().fold(Vec3::ZERO, |s, &p| s + plane[p]);
            out.push(sum * (1.0 / cap.len() as f64));
        }
        out
    }

    /// Checks the cut surface is a closed, consistently wound skin, as a collision surface must be.
    fn check(&self, points: &[Vec3]) -> Result<(), Box<dyn Error>> {
        let faces: Vec<[u32; 3]> = self
            .triangles
            .iter()
            .map(|t| t.map(|v| u32::try_from(v).expect("index fits u32")))
            .collect();
        skin_mesh(&self.apply(points), &faces)?;
        Ok(())
    }
}

/// Which source vertices belong to an arm: those skinned most strongly to a bone under either upper arm.
fn arm_vertices(model: &AnnyModel) -> Vec<bool> {
    let data = model.data();
    let k = data.header.counts.influences;
    let arms: Vec<usize> = ["upperarm01.L", "upperarm01.R"].iter().filter_map(|b| model.bone_index(b)).collect();
    (0..data.header.counts.vertices)
        .map(|vertex| {
            let first = vertex * k;
            let strongest = (0..k)
                .max_by(|a, b| data.vertex_bone_weights[first + a].total_cmp(&data.vertex_bone_weights[first + b]))
                .expect("a vertex has influences");
            let mut bone = data.vertex_bone_indices[first + strongest] as usize;
            loop {
                if arms.contains(&bone) {
                    return true;
                }
                match usize::try_from(data.header.labels.bone_parents[bone]) {
                    Ok(parent) => bone = parent,
                    Err(_) => return false,
                }
            }
        })
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    // `upper:MM` cuts the trunk MM above the floor.
    let cut_mm: Option<f64> = match args.get(6).map(String::as_str) {
        None | Some("whole") => None,
        Some(form) => Some(
            form.strip_prefix("upper:")
                .and_then(|mm| mm.parse().ok())
                .filter(|mm: &f64| mm.is_finite() && *mm > 0.0)
                .ok_or_else(|| format!("the form is whole or upper:MM, not {form}"))?,
        ),
    };
    let upper = cut_mm.is_some();
    if !(6..=7).contains(&args.len()) {
        return Err(
            "usage: bake-posable-body MODEL MEASUREMENTS.json NEW_OUT_DIR BASE_DEG ANGLE,ANGLE,... [whole|upper:MM]".into(),
        );
    }
    // Legs end at the top of the thigh: everything skinned to either upper leg and below is removed.
    let legs: &[&str] = if upper { &["upperleg01.R", "upperleg01.L"] } else { &[] };
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
        let (collision, display) = if upper {
            let mut roots = vec!["wrist.R", "wrist.L"];
            roots.extend(legs);
            roots.extend(["levator05.R", "levator05.L"].into_iter().filter(|label| model.bone_index(label).is_some()));
            (trimmed_skin(&model, &posed.vertices, &roots)?, trimmed_skin(&model, &posed.vertices, legs)?)
        } else {
            (
                clothing_skin_with_hands(&model, &posed.vertices, HandCollider::Wrist)?,
                skin_mesh(&posed.vertices, &model.data().faces)?,
            )
        };
        Ok((posed, collision, display))
    };

    let (posed, collision_skin, display_skin) = posed_at(base, base)?;
    let frames = BodyFrames::new(&model, &posed)?;
    // The trunk's cut, found on the base pose and kept for every pose. Arm vertices are never cut.
    let arms = arm_vertices(&model);
    let exempt = |skin: &ftx_body::SkinMesh| -> Vec<bool> {
        skin.source_vertex.iter().map(|&s| arms.get(s).copied().unwrap_or(false)).collect()
    };
    let cuts = match cut_mm {
        Some(mm) => {
            let z = (frames.floor_z + mm) / 1000.0;
            let c = Cut::new(&collision_skin.vertices, &collision_skin.triangles, z, &exempt(&collision_skin))?;
            let d = Cut::new(&display_skin.vertices, &display_skin.triangles, z, &exempt(&display_skin))?;
            c.check(&collision_skin.vertices)?;
            d.check(&display_skin.vertices)?;
            Some((c, d))
        }
        None => None,
    };
    // A pose's two surfaces as the page receives them: (collision, display) points and triangles.
    let surfaces = |collision: &ftx_body::SkinMesh, display: &ftx_body::SkinMesh| match &cuts {
        Some((c, d)) => (c.apply(&collision.vertices), d.apply(&display.vertices)),
        None => (collision.vertices.clone(), display.vertices.clone()),
    };
    let (collision_triangles, display_triangles) = match &cuts {
        Some((c, d)) => (c.triangles.clone(), d.triangles.clone()),
        None => (collision_skin.triangles.clone(), display_skin.triangles.clone()),
    };
    // The moving-body step refuses to start from or move to a body that crosses itself.
    let crossings = |points: &[Vec3]| -> Result<Option<String>, Box<dyn Error>> {
        let faces: Vec<[u32; 3]> = collision_triangles
            .iter()
            .map(|t| t.map(|v| u32::try_from(v).expect("index fits u32")))
            .collect();
        let skin = skin_mesh(points, &faces)?;
        let audit = skin.audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET)?;
        Ok(audit.first_intersecting_pair.map(|[a, b]| {
            let centre = |face: usize| {
                let [i, j, k] = skin.triangles[face];
                (skin.vertices[i] + skin.vertices[j] + skin.vertices[k]) * (1000.0 / 3.0)
            };
            let (p, q) = (centre(a), centre(b));
            format!(
                "{} face pairs, the first near ({:.0}, {:.0}, {:.0}) and ({:.0}, {:.0}, {:.0}) mm",
                audit.intersecting_pairs, p.x, p.y, p.z, q.x, q.y, q.z
            )
        }))
    };
    let (collision, display) = surfaces(&collision_skin, &display_skin);
    if let Some(where_) = crossings(&collision)? {
        return Err(format!(
            "the base pose's collision surface crosses itself at {base} degrees: {where_} (floor at {:.0} mm; \
             waist {:.0} and hip {:.0} mm above it)",
            frames.floor_z,
            frames.landmarks.get("waist").copied().unwrap_or(f64::NAN),
            frames.landmarks.get("hip").copied().unwrap_or(f64::NAN),
        )
        .into());
    }
    let mut blob = Blob {
        bytes: Vec::new(),
        directory: Map::new(),
    };
    blob.floats("collision_vertices", xyz(&collision));
    blob.indices("collision_triangles", collision_triangles.iter().flatten().copied());
    blob.floats("display_vertices", xyz(&display));
    blob.indices("display_triangles", display_triangles.iter().flatten().copied());

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
    // Each arm's own frame and landmark heights at every admitted angle: what a drape places a sleeve by
    // when it starts with the arms in that pose. A frame depends only on its own arm's angle.
    let mut arm_frames: [Vec<Value>; 2] = [Vec::new(), Vec::new()];
    let arm_frame = |posed: &ftx_body::PosedBody, part: &str| -> Result<Value, Box<dyn Error>> {
        let frames = BodyFrames::new(&model, posed)?;
        let f = frames.frame(part).ok_or("an arm frame is missing")?;
        let vector = |v: Vec3| json!([v.x, v.y, v.z]);
        let heights: Map<String, Value> = ["shoulder_tip", "elbow", "wrist"]
            .iter()
            .filter_map(|name| frames.landmark_h(part, name).map(|h| ((*name).to_string(), json!(h))))
            .collect();
        Ok(json!({"origin": vector(f.origin), "up": vector(f.up), "front": vector(f.front), "lat": vector(f.lat),
                  "landmarks": heights, "radius_mm": frames.section_radius(part, 0.0)}))
    };
    for &angle in &requested {
        let (left_posed, left_c, left_d) = posed_at(angle, base)?;
        let (right_posed, right_c, right_d) = posed_at(base, angle)?;
        let (left_c, left_d) = surfaces(&left_c, &left_d);
        let (right_c, right_d) = surfaces(&right_c, &right_d);
        if crossings(&left_c)?.is_some() || crossings(&right_c)?.is_some() {
            refused.push(angle);
        } else {
            admitted.push(angle);
            samples.push([left_c, left_d, right_c, right_d]);
            arm_frames[0].push(arm_frame(&left_posed, "arm_L")?);
            arm_frames[1].push(arm_frame(&right_posed, "arm_R")?);
        }
    }
    if admitted.len() < 2 {
        return Err("fewer than two arm angles have a collision surface that does not cross itself".into());
    }
    for (side, first) in [("left", 0), ("right", 2)] {
        for (surface, base_points, which) in [("collision", &collision, first), ("display", &display, first + 1)] {
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
        "arm_frames": {"arm_L": arm_frames[0], "arm_R": arm_frames[1]},
        "arm_angles_refused_deg": refused,
        "hands": "wrist",
        "form": if upper { "upper" } else { "whole" },
        "trunk_cut_above_floor_mm": cut_mm,
        "skinning": "lbs",
        "floor_mm": frames.floor_z,
        "radius_h0_mm": RADIUS_RANGE_MM.0,
        "frames": part_frames,
        "landmarks": landmarks,
        "counts": {
            "collision_vertices": collision.len(), "collision_triangles": collision_triangles.len(),
            "display_vertices": display.len(), "display_triangles": display_triangles.len(),
        },
        "fit": {"iterations": fit.iterations, "mass_kg": fit.mass_kg},
        "arrays": blob.directory,
    });
    fs::write(out.join("posable.json"), serde_json::to_vec(&header)?)?;
    fs::write(out.join("posable.bin"), &blob.bytes)?;
    println!(
        "{} collision and {} display vertices, {} arm angles ({} refused), {} bytes",
        collision.len(),
        display.len(),
        admitted.len(),
        refused.len(),
        blob.bytes.len()
    );
    Ok(())
}

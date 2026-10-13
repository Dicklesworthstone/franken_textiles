//! Drape poses, the nappy and the skin mesh against the pinned Anny package: the nine cases of
//! `tests/fixtures/oracle/body/anny-0.6.1-poses.json` (`tools/body-model/capture_anny_poses.py`, which
//! implements the pose definition again in NumPy and runs the package with it).
//!
//! Needs the converted model, so ignored by default:
//! `cargo test -p ftx-body --release -- --include-ignored`.

use ftx_body::{
    AnnyModel, Legs, Phenotype, Pose, default_model_path, joint_rotations, nappy_local_changes,
    pose_deltas, skin_mesh,
};
use ftx_core::Vec3;
use serde_json::Value;

const TOL_M: f64 = 1e-9;
const TOL_REL: f64 = 1e-10;

fn v3(v: &Value) -> Vec3 {
    Vec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}

fn checksums(points: &[Vec3]) -> [f64; 3] {
    let n = points.len() as f64;
    let mut s = [0.0; 3];
    for (i, p) in points.iter().enumerate() {
        s[0] += p.x + p.y + p.z;
        s[1] += p.x * p.x + p.y * p.y + p.z * p.z;
        s[2] += (i as f64 + 1.0) * (p.x + 2.0 * p.y + 3.0 * p.z);
    }
    s[2] /= n;
    s
}

fn rel(what: &str, got: f64, want: f64, fails: &mut Vec<String>) -> f64 {
    let d = (got - want).abs() / want.abs().max(1e-300);
    if !(d <= TOL_REL || (got - want).abs() <= 1e-15) {
        fails.push(format!("{what}: {got} vs {want} (relative {d:.3e})"));
    }
    d
}

fn point(what: &str, got: Vec3, want: Vec3, fails: &mut Vec<String>) -> f64 {
    let d = (got - want).norm();
    if d.is_nan() || d > TOL_M {
        fails.push(format!("{what}: {got:?} vs {want:?} ({d:.3e} m)"));
    }
    d
}

#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn drape_poses_match_the_package() {
    let path = default_model_path().expect("no model path (set FTX_ANNY_MODEL or HOME)");
    let m = AnnyModel::load(&path, false).unwrap_or_else(|r| panic!("{r}"));
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/oracle/body/anny-0.6.1-poses.json"
        ))
        .expect("fixture"),
    )
    .expect("fixture JSON");
    let labels = &m.data().header.labels.bones;
    let mut fails = Vec::new();
    let (mut worst_m, mut worst_rel, mut worst_angle) = (0.0f64, 0.0f64, 0.0f64);
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let inp = &case["inputs"];
        let ph = &inp["phenotype"];
        let f = |k: &str| ph[k].as_f64().unwrap();
        let base = m.params(Phenotype {
            gender: f("gender"),
            age: f("age"),
            muscle: f("muscle"),
            weight: f("weight"),
            height: f("height"),
            proportions: f("proportions"),
        });
        let params = nappy_local_changes(&m, &base, inp["nappy"].as_f64().unwrap()).unwrap();
        let pose = Pose {
            arm_abduction_deg: inp["arm_abduction_deg"].as_f64(),
            legs: match inp["legs"].as_str().unwrap() {
                "standing" => Legs::Standing,
                "seated" => Legs::Seated,
                "squat" => Legs::Squat,
                other => panic!("unknown legs {other}"),
            },
        };
        let t = inp["ramp_t"].as_f64().unwrap();
        let rest = m.rest(&params).unwrap();
        // Joint axes and angles, and the local rotation vectors they give.
        let joints = joint_rotations(&m, &rest, &pose).unwrap();
        let want = case["joints"].as_array().unwrap();
        assert_eq!(joints.len(), want.len(), "{name}: joint count");
        for (j, w) in joints.iter().zip(want) {
            assert_eq!(
                labels[j.bone],
                w["bone"].as_str().unwrap(),
                "{name}: joint order"
            );
            worst_angle = worst_angle
                .max((j.axis - v3(&w["axis"])).norm())
                .max((j.angle - w["angle"].as_f64().unwrap()).abs());
            let r = rest.bone_frames[j.bone].m.transpose().mul_vec(j.axis) * (j.angle * t);
            worst_angle = worst_angle.max((r - v3(&w["rotvec_local"])).norm());
        }
        // The posed body.
        let deltas = pose_deltas(&m, &rest, &pose, t).unwrap();
        let posed = m.pose(&rest, &deltas).unwrap();
        let pv = &case["posed_vertices"];
        for (k, i) in [
            ("first_vertex", 0),
            ("nose_tip", 297),
            ("top_of_head", 881),
            ("right_hand_outermost", 2301),
            ("right_elbow_near_joint", 3370),
            ("back_or_buttock_rearmost", 4202),
            ("sole_lowest", 6148),
            ("left_hand_outermost", 8953),
            ("waist_front", 10725),
            ("left_knee_near_joint", 11206),
        ] {
            worst_m = worst_m.max(point(
                &format!("{name} {k}"),
                posed.vertices[i],
                v3(&pv["selected"][k]),
                &mut fails,
            ));
        }
        let c = checksums(&posed.vertices);
        for (k, key) in ["sum", "sum_sq", "index_weighted"].iter().enumerate() {
            worst_rel = worst_rel.max(rel(
                &format!("{name} posed {key}"),
                c[k],
                pv["checksums"][key].as_f64().unwrap(),
                &mut fails,
            ));
        }
        // The skin shell.
        let s = skin_mesh(&posed.vertices, &m.data().faces).unwrap();
        let ws = &case["skin"];
        assert_eq!(
            (s.vertices.len(), s.triangles.len()),
            (
                ws["vertices"].as_u64().unwrap() as usize,
                ws["triangles"].as_u64().unwrap() as usize
            ),
            "{name}: skin size"
        );
        assert_eq!(
            (s.report.shells_dropped, s.report.triangles_dropped),
            (
                ws["shells_dropped"].as_u64().unwrap() as usize,
                ws["triangles_dropped"].as_u64().unwrap() as usize
            ),
            "{name}: dropped shells"
        );
        let first: Vec<usize> = ws["first_source_vertices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(
            &s.source_vertex[..5],
            &first[..],
            "{name}: skin vertex order"
        );
        worst_rel = worst_rel
            .max(rel(
                &format!("{name} skin volume"),
                s.report.volume,
                ws["volume"].as_f64().unwrap(),
                &mut fails,
            ))
            .max(rel(
                &format!("{name} skin area"),
                s.report.area,
                ws["area"].as_f64().unwrap(),
                &mut fails,
            ));
        let sc = checksums(&s.vertices);
        for (k, key) in ["sum", "sum_sq", "index_weighted"].iter().enumerate() {
            worst_rel = worst_rel.max(rel(
                &format!("{name} skin {key}"),
                sc[k],
                ws["checksums"][key].as_f64().unwrap(),
                &mut fails,
            ));
        }
        println!(
            "{name}: skin {} vertices, {} triangles, {:.3} L, {:.4} m²",
            s.vertices.len(),
            s.triangles.len(),
            s.report.volume * 1e3,
            s.report.area
        );
    }
    println!(
        "worst: {worst_m:.2e} m (vertices), {worst_rel:.2e} relative, {worst_angle:.2e} (axes, angles, rotation vectors)"
    );
    assert!(
        worst_angle < 1e-12,
        "joint rotations differ by {worst_angle:e}"
    );
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

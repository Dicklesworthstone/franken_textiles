//! Drape poses, the nappy and the skin mesh against the pinned Anny package: the ten cases of
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
            leg_fraction: 1.0,
            arm_abduction_deg: inp["arm_abduction_deg"].as_f64(),
            arm_pose: None,
            legs: match inp["legs"].as_str().unwrap() {
                "standing" => Legs::Standing,
                "seated" => Legs::Seated,
                "squat" => Legs::Squat,
                other => panic!("unknown legs {other}"),
            },
            straight_arms: inp["straight_arms"].as_bool().unwrap_or(false),
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

#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn dqs_reduces_volume_collapse_on_deep_seated_and_squat_poses() {
    use ftx_body::{DEFAULT_POSE_AUDIT_PAIR_BUDGET, age_parameter_for_years, clothing_skin};

    let path = default_model_path().expect("no model path (set FTX_ANNY_MODEL or HOME)");
    let m = AnnyModel::load(&path, false).unwrap_or_else(|r| panic!("{r}"));
    for (label, years) in [("toddler_2.5y", 2.5), ("adult_30y", 30.0)] {
        let params = m.params(Phenotype {
            age: age_parameter_for_years(years),
            ..Phenotype::default()
        });
        let rest = m.rest(&params).unwrap();
        let rest_skin = skin_mesh(&rest.vertices, &m.data().faces).unwrap();
        let rest_collider = clothing_skin(&m, &rest.vertices).unwrap();
        for (pose_name, legs) in [
            ("standing", Legs::Standing),
            ("seated", Legs::Seated),
            ("squat", Legs::Squat),
        ] {
            let pose = Pose {
                legs,
                ..Pose::drape()
            };
            let deltas = pose_deltas(&m, &rest, &pose, 1.0).unwrap();
            let lbs = m.pose(&rest, &deltas).unwrap();
            let dqs = m.pose_dqs(&rest, &deltas).unwrap();
            let lbs_skin = skin_mesh(&lbs.vertices, &m.data().faces).unwrap();
            let dqs_skin = skin_mesh(&dqs.vertices, &m.data().faces).unwrap();
            let lbs_geo = lbs_skin
                .pose_geometry_against_rest(&rest_skin, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            let dqs_geo = dqs_skin
                .pose_geometry_against_rest(&rest_skin, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            let lbs_col = clothing_skin(&m, &lbs.vertices)
                .unwrap()
                .pose_geometry_against_rest(&rest_collider, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            let dqs_col = clothing_skin(&m, &dqs.vertices)
                .unwrap()
                .pose_geometry_against_rest(&rest_collider, DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            println!(
                "{label:12} {pose_name:8} | skin LBS {:+6.2}% -> DQS {:+6.2}% | clothing LBS {:+6.2}% ({:3} pairs) -> DQS {:+6.2}% ({:3} pairs)",
                lbs_geo.volume_change_fraction * 100.0,
                dqs_geo.volume_change_fraction * 100.0,
                lbs_col.volume_change_fraction * 100.0,
                lbs_col.self_intersections.intersecting_pairs,
                dqs_col.volume_change_fraction * 100.0,
                dqs_col.self_intersections.intersecting_pairs,
            );
            assert!(dqs_skin.report.volume > 0.0 && dqs_skin.report.volume.is_finite());
            if matches!(legs, Legs::Seated | Legs::Squat) {
                assert!(
                    dqs_geo.volume_change_fraction.abs() < lbs_geo.volume_change_fraction.abs(),
                    "{label} {pose_name}: DQS full-skin volume change ({:.2}%) should be smaller than LBS ({:.2}%)",
                    dqs_geo.volume_change_fraction * 100.0,
                    lbs_geo.volume_change_fraction * 100.0,
                );
                assert!(
                    dqs_col.volume_change_fraction.abs() < lbs_col.volume_change_fraction.abs(),
                    "{label} {pose_name}: DQS clothing-skin volume change ({:.2}%) should be smaller than LBS ({:.2}%)",
                    dqs_col.volume_change_fraction * 100.0,
                    lbs_col.volume_change_fraction * 100.0,
                );
                assert!(
                    !dqs_geo.lbs_volume_loss_advisory && !dqs_col.lbs_volume_loss_advisory,
                    "{label} {pose_name}: DQS should stay within the 3% volume-loss advisory threshold"
                );
            }
        }
    }
}

#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn the_palm_collider_keeps_the_palms_and_caps_each_finger_at_its_base() {
    use ftx_body::{
        DEFAULT_POSE_AUDIT_PAIR_BUDGET, HandCollider, age_parameter_for_years, clothing_skin,
        clothing_skin_with_hands,
    };

    let path = default_model_path().expect("no model path (set FTX_ANNY_MODEL or HOME)");
    let m = AnnyModel::load(&path, false).unwrap_or_else(|r| panic!("{r}"));
    // Synthetic cap centres carry no source vertex.
    let caps = |skin: &ftx_body::SkinMesh| {
        skin.source_vertex
            .iter()
            .filter(|&&source| source == usize::MAX)
            .count()
    };
    for (label, years) in [("toddler_2.5y", 2.5), ("adult_30y", 30.0)] {
        let params = m.params(Phenotype {
            age: age_parameter_for_years(years),
            ..Phenotype::default()
        });
        let rest = m.rest(&params).unwrap();
        let deltas = pose_deltas(&m, &rest, &Pose::drape(), 1.0).unwrap();
        let posed = m.pose(&rest, &deltas).unwrap();

        // Asking for the reference collider is asking for `clothing_skin`.
        let wrist = clothing_skin(&m, &posed.vertices).unwrap();
        let same = clothing_skin_with_hands(&m, &posed.vertices, HandCollider::Wrist).unwrap();
        assert_eq!(wrist.vertices, same.vertices, "{label}");
        assert_eq!(wrist.triangles, same.triangles, "{label}");
        assert_eq!(wrist.trim_roots, same.trim_roots, "{label}");

        // Each native collider keeps more of the hand than the one before it, with ten finger caps
        // in place of the two wrist caps.
        let mut kept = wrist.vertices.len();
        for (hands, first_root) in [
            (HandCollider::Palm, "finger1-1.R"),
            (HandCollider::Knuckle, "finger1-2.R"),
        ] {
            let name = hands.as_str();
            let skin = clothing_skin_with_hands(&m, &posed.vertices, hands).unwrap();
            assert!(skin.vertices.len() > kept, "{label} {name}");
            assert_eq!(caps(&skin), caps(&wrist) + 8, "{label} {name}");
            assert_eq!(skin.trim_roots[0], first_root, "{label} {name}");
            // Every kept vertex is where the model put it.
            for (vertex, &source) in skin.vertices.iter().zip(&skin.source_vertex) {
                if source != usize::MAX {
                    assert_eq!(*vertex, posed.vertices[source], "{label} {name}");
                }
            }
            // It is usable as a collider: closed and outward (checked when it is built) and its
            // surface does not cross itself.
            let audit = skin
                .audit_self_intersections(DEFAULT_POSE_AUDIT_PAIR_BUDGET)
                .unwrap();
            assert_eq!(audit.intersecting_pairs, 0, "{label} {name}: {audit:?}");
            println!(
                "{label:12} {name:8} {} vertices ({} at the wrist caps), {} pairs tested",
                skin.vertices.len(),
                wrist.vertices.len(),
                audit.pairs_tested
            );
            kept = skin.vertices.len();
        }
    }
}

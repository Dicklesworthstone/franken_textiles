//! Parity with the pinned Anny package (spec 05 §8): the 16 captured cases in
//! `tests/fixtures/oracle/body/anny-0.6.1-reference.json`, and the converted arrays against the
//! package's own hashes.
//!
//! These need the converted model (`tools/body-model/convert_anny.py`, ~150 MB, never committed), so
//! they are ignored by default. Run them with
//! `cargo test -p ftx-body --release -- --include-ignored` after converting; `FTX_ANNY_MODEL` overrides
//! the default path.

use std::sync::OnceLock;

use ftx_body::{Affine, AnnyModel, BodyParams, Phenotype, default_model_path};
use ftx_core::receipt::Sha256;
use ftx_core::{Mat3, Vec3};
use serde_json::Value;

/// Vertices and bone positions (spec 05 §8.8 recommends 1e-9 m for the f64 path; gate G1a is 5e-5 m).
const TOL_M: f64 = 1e-9;
/// Relative tolerance for anthropometry, checksums and coefficients.
const TOL_REL: f64 = 1e-10;

fn model() -> &'static AnnyModel {
    static MODEL: OnceLock<AnnyModel> = OnceLock::new();
    MODEL.get_or_init(|| {
        let path = default_model_path().expect("no model path (set FTX_ANNY_MODEL or HOME)");
        AnnyModel::load(&path, false).unwrap_or_else(|r| panic!("{r}"))
    })
}

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/oracle/body/anny-0.6.1-reference.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("fixture JSON")
}

fn v3(v: &Value) -> Vec3 {
    Vec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}

fn rows4(v: &Value) -> Vec<[f64; 4]> {
    (0..4)
        .map(|i| std::array::from_fn(|j| v[i][j].as_f64().unwrap()))
        .collect()
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

fn bbox(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
        (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(lo, hi), p| {
            (
                Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
            )
        },
    )
}

#[derive(Default)]
struct Worst {
    metres: f64,
    relative: f64,
    failures: Vec<String>,
}

impl Worst {
    fn point(&mut self, what: &str, got: Vec3, want: Vec3) {
        let d = (got - want).norm();
        self.metres = self.metres.max(d);
        if d.is_nan() || d > TOL_M {
            self.failures
                .push(format!("{what}: {got:?} vs {want:?} ({d:.3e} m)"));
        }
    }

    fn rel(&mut self, what: &str, got: f64, want: f64) {
        let d = (got - want).abs() / want.abs().max(1e-300);
        self.relative = self
            .relative
            .max(if want == 0.0 { (got - want).abs() } else { d });
        let ok = d <= TOL_REL || (got - want).abs() <= 1e-15; // false for NaN
        if !ok {
            self.failures
                .push(format!("{what}: {got} vs {want} (relative {d:.3e})"));
        }
    }

    fn affine(&mut self, what: &str, got: &Affine, want: &Value) {
        let w = rows4(want);
        let g = got.to_rows4();
        for i in 0..3 {
            for j in 0..4 {
                let d = (g[i][j] - w[i][j]).abs();
                self.metres = self.metres.max(d);
                if d.is_nan() || d > TOL_M {
                    self.failures.push(format!(
                        "{what}[{i}][{j}]: {} vs {} ({d:.3e})",
                        g[i][j], w[i][j]
                    ));
                }
            }
        }
    }
}

#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn sixteen_reference_cases_match_the_package() {
    let m = model();
    let fx = fixture();
    let selected: Vec<(String, usize)> = fx["model"]["selected_vertices"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap() as usize))
        .collect();
    let cases = fx["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    let mut worst = Worst::default();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let inp = &case["inputs"];
        let ph = &inp["phenotype"];
        let get = |k: &str| ph[k].as_f64().unwrap();
        let mut params: BodyParams = m.params(Phenotype {
            gender: get("gender"),
            age: get("age"),
            muscle: get("muscle"),
            weight: get("weight"),
            height: get("height"),
            proportions: get("proportions"),
        });
        for (label, v) in inp["local_changes"].as_object().unwrap() {
            let j = m
                .local_change_index(label)
                .unwrap_or_else(|| panic!("{name}: unknown local change {label}"));
            params.local_changes[j] = v.as_f64().unwrap();
        }
        let mut deltas = vec![Affine::IDENTITY; m.data().header.counts.bones];
        for (bone, spec) in inp["pose_local_bone"].as_object().unwrap() {
            deltas[m.bone_index(bone).unwrap()] = Affine::from_rows4(&rows4(&spec["matrix"]));
        }

        // Coefficients: every non-zero phenotype coefficient, by label.
        let coef = m.coefficients(&params).unwrap();
        let labels = &m.data().header.labels.blendshapes;
        let p = m.data().header.counts.phenotype_blendshapes;
        let want: Vec<(String, f64)> = case["nonzero_phenotype_coefficients"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e[0].as_str().unwrap().to_string(), e[1].as_f64().unwrap()))
            .collect();
        let got: Vec<(String, f64)> = (0..p)
            .filter(|&k| coef[k] != 0.0)
            .map(|k| (labels[k].clone(), coef[k]))
            .collect();
        assert_eq!(
            got.len(),
            want.len(),
            "{name}: non-zero phenotype coefficient count"
        );
        for ((gl, gv), (wl, wv)) in got.iter().zip(&want) {
            assert_eq!(gl, wl, "{name}");
            worst.rel(&format!("{name} coefficient {gl}"), *gv, *wv);
        }

        let rest = m.rest(&params).unwrap();
        let posed = m.pose(&rest, &deltas).unwrap();
        for (key, verts) in [
            ("rest_vertices", &rest.vertices),
            ("posed_vertices", &posed.vertices),
        ] {
            let w = &case[key];
            for (vname, idx) in &selected {
                worst.point(
                    &format!("{name} {key} {vname}"),
                    verts[*idx],
                    v3(&w["selected"][vname]),
                );
            }
            let (lo, hi) = bbox(verts);
            worst.point(&format!("{name} {key} bbox_min"), lo, v3(&w["bbox_min"]));
            worst.point(&format!("{name} {key} bbox_max"), hi, v3(&w["bbox_max"]));
            let cs = checksums(verts);
            for (i, k) in ["sum", "sum_sq", "index_weighted"].iter().enumerate() {
                worst.rel(
                    &format!("{name} {key} {k}"),
                    cs[i],
                    w["checksums"][k].as_f64().unwrap(),
                );
            }
        }
        for (bone, b) in case["bones"].as_object().unwrap() {
            let j = b["index"].as_u64().unwrap() as usize;
            assert_eq!(m.bone_index(bone), Some(j));
            worst.point(
                &format!("{name} {bone} rest head"),
                rest.bone_heads[j],
                v3(&b["rest_head"]),
            );
            worst.point(
                &format!("{name} {bone} rest tail"),
                rest.bone_tails[j],
                v3(&b["rest_tail"]),
            );
            worst.affine(
                &format!("{name} {bone} rest pose"),
                &rest.bone_frames[j],
                &b["rest_pose"],
            );
            worst.affine(
                &format!("{name} {bone} posed pose"),
                &posed.bone_poses[j],
                &b["posed_pose"],
            );
        }
        let bc = &case["bone_checksums"];
        let posed_t: Vec<Vec3> = posed.bone_poses.iter().map(|a| a.t).collect();
        for (key, pts) in [
            ("rest_heads", &rest.bone_heads),
            ("rest_tails", &rest.bone_tails),
            ("posed_translations", &posed_t),
        ] {
            let cs = checksums(pts);
            for (i, k) in ["sum", "sum_sq", "index_weighted"].iter().enumerate() {
                worst.rel(
                    &format!("{name} bones {key} {k}"),
                    cs[i],
                    bc[key][k].as_f64().unwrap(),
                );
            }
        }
        let a = m.anthropometry(&rest);
        let wa = &case["anthropometry_on_rest_vertices"];
        for (k, v) in [
            ("height", a.height),
            ("waist_circumference", a.waist_circumference),
            ("volume", a.volume),
            ("mass", a.mass),
            ("bmi", a.bmi),
        ] {
            worst.rel(&format!("{name} {k}"), v, wa[k].as_f64().unwrap());
        }
    }
    println!(
        "anny parity: 16 cases, worst absolute deviation {:.3e} m, worst relative {:.3e} (tolerances {TOL_M:e} m, {TOL_REL:e}; gate G1a 5e-5 m)",
        worst.metres, worst.relative
    );
    assert!(
        worst.failures.is_empty(),
        "{} mismatches:\n{}",
        worst.failures.len(),
        worst.failures.join("\n")
    );
}

fn sha256_hex(bytes: impl IntoIterator<Item = u8>) -> String {
    let mut h = Sha256::new();
    let mut buf = Vec::with_capacity(1 << 16);
    for b in bytes {
        buf.push(b);
        if buf.len() == buf.capacity() {
            h.update(&buf);
            buf.clear();
        }
    }
    h.update(&buf);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The loader reproduces the package's arrays byte for byte, in the package's dtypes (spec 05 §8.1):
/// this proves the converter and the reader agree, including the sparse blend-shape layout.
#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn loaded_arrays_hash_to_the_package_values() {
    let m = model();
    let d = m.data();
    let c = &d.header.counts;
    let want = |k: &str| d.header.package_sha256[k].clone();
    let f64s = |v: &[Vec3]| {
        v.iter()
            .flat_map(|p| [p.x, p.y, p.z])
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<u8>>()
    };
    assert_eq!(
        sha256_hex(f64s(&d.template_vertices)),
        want("template_vertices")
    );
    assert_eq!(
        sha256_hex(f64s(&d.template_bone_heads)),
        want("template_bone_heads")
    );
    assert_eq!(
        sha256_hex(f64s(&d.template_bone_tails)),
        want("template_bone_tails")
    );
    assert_eq!(
        sha256_hex(f64s(&d.bone_heads_blendshapes)),
        want("bone_heads_blendshapes")
    );
    assert_eq!(
        sha256_hex(f64s(&d.bone_tails_blendshapes)),
        want("bone_tails_blendshapes")
    );
    let faces: Vec<u8> = d
        .faces
        .iter()
        .flatten()
        .flat_map(|&i| i64::from(i).to_le_bytes())
        .collect();
    assert_eq!(sha256_hex(faces), want("faces"));
    let idx: Vec<u8> = d
        .vertex_bone_indices
        .iter()
        .flat_map(|&i| i64::from(i).to_le_bytes())
        .collect();
    assert_eq!(sha256_hex(idx), want("vertex_bone_indices"));
    let w: Vec<u8> = d
        .vertex_bone_weights
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    assert_eq!(sha256_hex(w), want("vertex_bone_weights"));
    let base: Vec<u8> = d
        .base_mesh_vertex_indices
        .iter()
        .flat_map(|&i| i64::from(i).to_le_bytes())
        .collect();
    assert_eq!(sha256_hex(base), want("base_mesh_vertex_indices"));
    let mask: Vec<u8> = d
        .phenotype_mask
        .iter()
        .flat_map(|&m| f64::from(m).to_le_bytes())
        .collect();
    assert_eq!(
        sha256_hex(mask),
        want("stacked_phenotype_blend_shapes_mask")
    );
    let rolls: Vec<u8> = d
        .bone_rolls
        .iter()
        .flat_map(|m: &Mat3| m.rows.into_iter().flatten())
        .flat_map(f64::to_le_bytes)
        .collect();
    assert_eq!(sha256_hex(rolls), want("bone_rolls_rotmat"));
    // Dense blend shapes, streamed row by row (374 MB of f64 without holding it).
    let mut h = Sha256::new();
    let mut row = vec![0u8; c.vertices * 24];
    for k in 0..c.blendshapes {
        row.fill(0);
        let (a, b) = (
            d.blendshape_row_ptr[k] as usize,
            d.blendshape_row_ptr[k + 1] as usize,
        );
        for (&vi, p) in d.blendshape_vertex[a..b]
            .iter()
            .zip(&d.blendshape_delta[a..b])
        {
            let o = vi as usize * 24;
            row[o..o + 8].copy_from_slice(&p.x.to_le_bytes());
            row[o + 8..o + 16].copy_from_slice(&p.y.to_le_bytes());
            row[o + 16..o + 24].copy_from_slice(&p.z.to_le_bytes());
        }
        h.update(&row);
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, want("blendshapes"));
}

/// Shape facts that should hold for any correct build: the default body is mirror-symmetric, adults
/// are taller than toddlers, and the identity pose only moves the root head to the origin.
#[test]
#[ignore = "needs the converted Anny model (tools/body-model/convert_anny.py)"]
fn body_properties() {
    let m = model();
    let rest = m.rest(&m.params(Phenotype::default())).unwrap();
    let left = m.bone_index("upperarm01.L").unwrap();
    let right = m.bone_index("upperarm01.R").unwrap();
    let (l, r) = (rest.bone_heads[left], rest.bone_heads[right]);
    assert!((l.x + r.x).abs() < 1e-12 && (l.y - r.y).abs() < 1e-12 && (l.z - r.z).abs() < 1e-12);
    let toddler = m.anthropometry(
        &m.rest(&m.params(Phenotype {
            age: 0.1325,
            gender: 1.0,
            ..Phenotype::default()
        }))
        .unwrap(),
    );
    let woman = m.anthropometry(
        &m.rest(&m.params(Phenotype {
            age: 0.7857,
            gender: 1.0,
            ..Phenotype::default()
        }))
        .unwrap(),
    );
    assert!(woman.height > toddler.height + 0.7, "{woman:?} {toddler:?}");
    let posed = m
        .pose(&rest, &vec![Affine::IDENTITY; m.data().header.counts.bones])
        .unwrap();
    let h0 = rest.bone_heads[0];
    let worst = rest
        .vertices
        .iter()
        .zip(&posed.vertices)
        .map(|(a, b)| ((*a - h0) - *b).norm())
        .fold(0.0, f64::max);
    assert!(
        worst < 3e-7,
        "identity pose moved a vertex {worst} m (float32 rolls allow 2e-7)"
    );
}

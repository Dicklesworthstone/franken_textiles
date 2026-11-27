//! Actual CLI and portable extraction agree on explicit source geometry and preserve its identity.
//! This synthetic closed prism verifies consumer plumbing; anatomical branching has separate
//! known-answer body tests and no physical-fit claim is made here.

use ftx_body::knit_shell::{
    AnatomicalCut, AnatomicalSection, AnatomicalShellRequest, AnatomicalShellSpec, body_mesh_sha256,
};
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use ftx_core::shaping::EaseShellMesh;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn workspace(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ftx-knit-shell-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

fn request() -> AnatomicalShellRequest {
    let mut vertices_m = Vec::new();
    for z in [0.0, 1.0] {
        for (x, y) in [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)] {
            vertices_m.push([x, y, z]);
        }
    }
    let mut triangles = Vec::new();
    for a in 0..4 {
        let b = (a + 1) % 4;
        triangles.extend([[a, b, b + 4], [a, b + 4, a + 4]]);
    }
    triangles.extend([[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]]);
    let vertices: Vec<_> = vertices_m.iter().copied().map(Vec3::from).collect();
    let indices: Vec<_> = triangles.iter().map(|t| t.map(|v| v as usize)).collect();
    let expected_body_mesh_sha256 = Some(body_mesh_sha256(&vertices, &indices).unwrap());
    AnatomicalShellRequest {
        vertices_m,
        triangles,
        spec: AnatomicalShellSpec {
            cuts: [("hem", 0.2, true), ("neck", 0.8, false)]
                .map(|(name, z, start)| AnatomicalCut {
                    name: name.into(),
                    is_start: start,
                    keep_positive: start,
                    section: AnatomicalSection::Plane {
                        origin_m: [0.0, 0.0, z],
                        normal: [0.0, 0.0, 1.0],
                    },
                })
                .to_vec(),
            radial_offsets_m: vec![0.0; 8],
            min_clearance_m: 0.0,
            allow_negative_ease: false,
        },
        expected_body_mesh_sha256,
    }
}

fn run(dir: &Path, args: &[&str], success: bool) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        result.status.success(),
        success,
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn explicit_anatomy_reaches_real_cli_and_portable_shell_files_identically() {
    let dir = workspace("parity");
    let input = request();
    let source = serde_json::to_string_pretty(&input).unwrap();
    fs::write(dir.join("source.json"), &source).unwrap();
    let output = run(
        &dir,
        &["knit", "shell", "--input", "source.json", "--out", "result"],
        true,
    );
    let portable = ftx_wasm::knit_shell::knit_shell_json(&source).unwrap();
    let stored = fs::read_to_string(dir.join("result/shell.json")).unwrap();
    assert_eq!(stored, portable);
    assert_eq!(
        fs::read_to_string(dir.join("result/source.json")).unwrap(),
        source
    );
    assert_eq!(
        output["result"]["shell_sha256"],
        sha256_hex(stored.as_bytes())
    );
    assert_eq!(
        output["result"]["source_body_sha256"],
        input.expected_body_mesh_sha256.unwrap()
    );
    let shell: EaseShellMesh = serde_json::from_str(&stored).unwrap();
    assert_eq!(shell.boundary_rings.len(), 2);
    assert!(
        shell
            .boundary_rings
            .iter()
            .all(|r| (r.perimeter_m - 0.8).abs() < 1e-12)
    );
    let obj = fs::read_to_string(dir.join("result/shell.obj")).unwrap();
    assert_eq!(
        obj.lines().filter(|l| l.starts_with("v ")).count(),
        shell.vertices_m.len()
    );
    assert_eq!(
        obj.lines().filter(|l| l.starts_with("f ")).count(),
        shell.triangles.len()
    );
    run(
        &dir,
        &["knit", "shell", "--input", "source.json", "--out", "result"],
        false,
    );
    assert_eq!(
        fs::read_to_string(dir.join("result/shell.json")).unwrap(),
        stored
    );
}

#[test]
fn changed_source_and_ambiguous_cli_sources_refuse_without_publishing() {
    let dir = workspace("refusals");
    let mut input = request();
    input.vertices_m[0][0] -= 0.001;
    let source = serde_json::to_string(&input).unwrap();
    fs::write(dir.join("changed.json"), &source).unwrap();
    let output = run(
        &dir,
        &[
            "knit",
            "shell",
            "--input",
            "changed.json",
            "--out",
            "changed",
        ],
        false,
    );
    assert!(
        output["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("SHA-256 mismatch")
    );
    assert!(
        ftx_wasm::knit_shell::knit_shell_json(&source)
            .unwrap_err()
            .message
            .contains("SHA-256 mismatch")
    );
    assert!(!dir.join("changed").exists());
    run(
        &dir,
        &[
            "knit",
            "shell",
            "--input",
            "changed.json",
            "--body-fit",
            "unused.json",
            "--out",
            "ambiguous",
        ],
        false,
    );
    assert!(!dir.join("ambiguous").exists());
    fs::write(dir.join("ease.json"), r#"{"torso_radial_mm":0.0,"limb_radial_mm":0.0,"min_clearance_mm":0.0,"allow_negative_ease":false}"#).unwrap();
    fs::write(
        dir.join("unaccepted.json"),
        r#"{"schema":"ftx-body-fit/1","status":"REJECTED","solution":{}}"#,
    )
    .unwrap();
    run(
        &dir,
        &[
            "knit",
            "shell",
            "--body-fit",
            "unaccepted.json",
            "--model",
            "deliberately-absent.ftxbody",
            "--region",
            "full_upper_body",
            "--ease",
            "ease.json",
            "--out",
            "rejected-fit",
        ],
        false,
    );
    assert!(!dir.join("rejected-fit").exists());
}

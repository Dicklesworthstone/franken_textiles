//! Real source origins enter the existing strip solver through the same native/portable adapter.
//! The local-linear domain and ideal fixture are explicit approximations, not measured acceptance.

use std::{fs, path::Path, process::Command};

use ftx_drape::strip::{self, Controls, Fixture, Request};
use ftx_pattern::{
    material_linearization::{Input, Report, linearize},
    materials::Catalog,
};
use serde_json::Value;

const INPUT: &str = include_str!("../../../examples/wang2011-origin-linearization.json");

fn request(id: &str) -> Input {
    let mut input: Input = serde_json::from_str(INPUT).unwrap();
    input.material.id = id.into();
    input
}

fn native(main: bool, path: &Path) -> std::process::Output {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["--json", "materials"]);
    }
    command.arg("linearize-origin").arg(path).output().unwrap()
}

#[test]
fn both_native_commands_and_portable_api_return_identical_source_locked_models() {
    let directory = std::env::temp_dir().join(format!(
        "ftx-origin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("input.json");
    fs::write(&path, INPUT).unwrap();
    let expected =
        serde_json::to_value(linearize(&serde_json::from_str(INPUT).unwrap()).unwrap()).unwrap();
    let portable: Value =
        serde_json::from_str(&ftx_wasm::materials::linearize_origin(INPUT).unwrap()).unwrap();
    assert_eq!(portable, expected);
    for main in [false, true] {
        let output = native(main, &path);
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
        let actual = if main { &parsed["result"] } else { &parsed };
        assert_eq!(actual, &expected);
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), INPUT);
    let mut refused = request("wang/2011/navy-sparkle-sweat");
    for consent in [true, false] {
        refused.material.allow_estimates = consent;
        let json = serde_json::to_string(&refused).unwrap();
        fs::write(&path, &json).unwrap();
        assert!(ftx_wasm::materials::linearize_origin(&json).is_err());
        for main in [false, true] {
            let output = native(main, &path);
            assert_eq!(output.status.code(), Some(if main { 3 } else { 2 }));
            let value: Value =
                serde_json::from_slice(if main { &output.stdout } else { &output.stderr }).unwrap();
            assert!(value["refusal"].is_object());
        }
    }
}

#[test]
fn source_material_swap_changes_actual_clamped_strip_response_with_fixed_geometry() {
    let fixture = Fixture {
        gauge_length_m: 0.1,
        width_m: 0.04,
        warp_axis: [0., 1.],
        cells: [2, 4],
    };
    let mut runs = Vec::new();
    for id in ["wang/2011/gray-interlock", "wang/2011/11oz-black-denim"] {
        let prepared = linearize(&request(id)).unwrap();
        let strip = Request {
            schema: "ftx-strip-experiment/1".into(),
            material: prepared.selection,
            fixture: fixture.clone(),
            extensions: vec![0.0, 0.005],
            controls: Controls::default(),
        };
        let report = strip::evaluate(&strip).unwrap();
        assert!(report.states[0].force_n.abs() < 1e-10);
        let state = &report.states[1];
        assert!(state.force_n > 0.0 && state.energy_j > 0.0);
        assert!(state.free_force_residual_n <= strip.controls.force_tolerance_n);
        assert!(state.grip_balance_residual_n <= strip.controls.force_tolerance_n);
        let replay = strip::evaluate(&report.input).unwrap();
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::to_value(replay).unwrap()
        );
        let mut outside = strip;
        outside.extensions = vec![0.05];
        assert!(strip::evaluate(&outside).is_err());
        runs.push(report);
    }
    assert_eq!(
        serde_json::to_value(&runs[0].input.fixture).unwrap(),
        serde_json::to_value(&runs[1].input.fixture).unwrap()
    );
    assert!(runs[1].states[1].force_n > 5.0 * runs[0].states[1].force_n);
    assert_ne!(
        runs[0].input.material.expected_sha256,
        runs[1].input.material.expected_sha256
    );
    assert_ne!(runs[0].states[1].positions_m, runs[1].states[1].positions_m);
}

#[test]
fn all_source_origins_remain_available_and_only_admissible_approximations_compile() {
    let catalog = Catalog::builtin().unwrap();
    let mut admitted = 0;
    for source in catalog
        .materials
        .iter()
        .filter(|m| m.id.starts_with("wang/2011/"))
    {
        let result = linearize(&request(&source.id));
        if source.id == "wang/2011/navy-sparkle-sweat" {
            assert!(result.is_err());
        } else {
            let report: Report = result.unwrap();
            let resolved = catalog.resolve(&report.selection).unwrap();
            assert!(resolved.membrane_response().unwrap().is_some());
            assert!(resolved.mechanics().is_err());
            assert_eq!(
                resolved.material().properties["areal_density"].value,
                source.properties["areal_density"].value
            );
            admitted += 1;
        }
    }
    assert_eq!(admitted, 9);
}

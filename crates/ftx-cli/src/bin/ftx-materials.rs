//! Material workbench companion to `ftx`: the same catalog, selection and drape resolution APIs.
//! This executable does no network access and never overwrites a pattern when binding a material.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::Pattern;
use ftx_pattern::material_calibration::{Input as CalibrationInput, calibrate};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_cost::{QuoteBook, Scope};
use ftx_pattern::materials::ThermalStack;
use ftx_pattern::textile_import::{catalog, resolve_fabric};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[path = "material_workbench/coverage.rs"]
mod thermal_coverage;
#[path = "material_workbench/projection.rs"]
mod thermal_projection;
#[path = "material_workbench/surface.rs"]
mod thermal_surface;
#[path = "material_workbench/thermal.rs"]
mod thermal_transient;

#[path = "material_workbench/cutting.rs"]
mod cutting;

const USAGE: &str = "ftx-materials catalog [SEARCH]
ftx-materials show ID
ftx-materials resolve PATTERN.json
ftx-materials bind PATTERN.json SELECTION.json NEW-PATTERN.json
ftx-materials price PATTERN.json QUOTES.json SCOPE.json
ftx-materials thermal STACK.json
ftx-materials calibrate SWATCHES.json [--allow-estimates]
ftx-materials thermal-transient STACK.json
ftx-materials thermal-surface SIM_REPORT.json CONDITIONS.json
ftx-materials thermal-coverage SIM_REPORT.json COVERAGE.json CONDITIONS.json
ftx-materials thermal-map SIM_REPORT.json BODY.obj PROJECTION.json
ftx-materials cut-plan PATTERN.json OPTIONS.json
ftx-materials cut-price PATTERN.json OPTIONS.json QUOTES.json MARKET.json
ftx-materials cut-svg PATTERN.json OPTIONS.json ROLL-INDEX NEW.svg

JSON output. `show` includes a portable selection and missing-property coverage.
`resolve` invokes the actual drape resolver without meshing; missing mechanics refuse.
`bind` preserves geometry and embeds a locked selection; its output path must not exist.
`price` is a dated offer sample, not a checkout or a worldwide market price.
`thermal` is a dry 1D layer calculation, not a human comfort or survival rating.
`calibrate` fits observed swatches, retaining the receipt and a portable locked selection.
`thermal-surface` calculates dry heat maps on frozen drape geometry, not whole-wearer comfort.
`thermal-coverage` couples mapped layers and bare body areas; generated geometry receipts are replayed.
`thermal-map` derives coverage polygons from actual cloth and body geometry under a local-normal closure.
`cut-plan` derives material-specific roll requirements from real cut instances and sourced width.
`cut-price` prices gross marker stock, including layout waste; missing offers remain unknown.
`cut-svg` writes a new source-locked planning preview, not device execution code.
Fitted coefficients are estimates: --allow-estimates explicitly admits them to simulation.";

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn io(path: &str, error: std::io::Error) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{path}: {error}"))
}

fn read<T: DeserializeOwned>(path: &str) -> Result<T, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| io(path, e))?
        .take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(path, e))?;
    if bytes.len() as u64 > MAX {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "material input exceeds 64 MiB",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{path}: {e}")))
}

fn selected(raw: &Value) -> Result<ftx_pattern::material_catalog::ResolvedMaterial, Refusal> {
    if !raw.is_object() || !raw["fabric"].is_object() {
        return Err(invalid(
            "pattern needs an object-valued fabric with a material selection",
        ));
    }
    if let Some(pieces) = raw["pieces"].as_array() {
        for p in pieces {
            // Match the drape resolver: a string is a cutting label, not a physical selection.
            if p.get("material").is_some_and(|m| !m.is_string())
                || p.get("material_id").is_some()
                || p["fabric"].get("material").is_some()
                || p["fabric"].get("material_id").is_some()
            {
                return Err(invalid(
                    "heterogeneous piece assignments need separate material requirements",
                ));
            }
        }
    }
    resolve_fabric(&raw["fabric"])?
        .ok_or_else(|| invalid("pattern has no explicit material selection"))
}

fn bind(raw: Value, selection: &Selection, out: &str) -> Result<Value, Refusal> {
    let pattern = Pattern::new(raw);
    let errors = pattern.structure_errors();
    if !errors.is_empty() {
        return Err(invalid(errors.join("; ")));
    }
    let snapshot = catalog()?.resolve(selection)?;
    let mut raw = pattern.raw().clone();
    let root = raw
        .as_object_mut()
        .ok_or_else(|| invalid("pattern must be an object"))?;
    let fabric = root.entry("fabric").or_insert_with(|| json!({}));
    let fabric = fabric
        .as_object_mut()
        .ok_or_else(|| invalid("fabric must be an object"))?;
    for key in [
        "areal_g_m2",
        "tensile_N_m",
        "bending_Nm",
        "material_id",
        "material",
    ] {
        fabric.remove(key);
    }
    fabric.insert(
        "material".into(),
        serde_json::to_value(snapshot.frozen_selection()).map_err(|e| invalid(e.to_string()))?,
    );
    // The same selection policy used by purchasing; don't bind a global material over heterogeneous pieces.
    selected(&raw)?;
    let mut bytes = serde_json::to_vec_pretty(&raw).map_err(|e| invalid(e.to_string()))?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(out))
        .map_err(|e| io(out, e))?;
    file.write_all(&bytes).map_err(|e| io(out, e))?;
    Ok(json!({"out":out,"material_sha256":snapshot.sha256(),
        "missing_mechanics":snapshot.material().missing_mechanics(),
        "simulation_ready":snapshot.mechanics().is_ok(),"geometry_changed":false}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StackInput {
    layers: Vec<Selection>,
    area_m2: f64,
    inside_c: f64,
    outside_c: f64,
    boundary_resistance_m2_k_w: f64,
}

fn calibration(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let input: CalibrationInput = read(path)?;
    let report = calibrate(&input)?;
    let snapshot = catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let mechanics = snapshot.mechanics();
    Ok(
        json!({"calibration":report,"selection":snapshot.frozen_selection(),
        "simulation_ready":mechanics.is_ok(),"mechanics_refusal":mechanics.err(),
        "fit_claim":false,"calibration_observations_are_not_independent_garment_validation":true}),
    )
}

fn run(args: &[String]) -> Result<Value, Refusal> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["cut-plan", pattern, options] => cutting::plan(pattern, options),
        ["cut-price", pattern, options, quotes, market] => {
            cutting::price(pattern, options, quotes, market)
        }
        ["cut-svg", pattern, options, index, out] => cutting::preview(pattern, options, index, out),
        ["thermal-surface", report, conditions] => thermal_surface::run(report, conditions),
        ["thermal-coverage", report, mapping, conditions] => {
            thermal_coverage::run(report, mapping, conditions)
        }
        ["thermal-map", report, body, settings] => thermal_projection::run(report, body, settings),
        ["thermal-transient", path] => thermal_transient::run(path),
        ["calibrate", path] => calibration(path, false),
        ["calibrate", path, "--allow-estimates"] => calibration(path, true),
        ["catalog"] | ["catalog", _] => {
            let needle = words.get(1).copied().unwrap_or("").to_lowercase();
            let mut db = catalog()?;
            db.materials.retain(|m| {
                m.id.to_lowercase().contains(&needle)
                    || m.name.to_lowercase().contains(&needle)
                    || m.composition
                        .iter()
                        .any(|c| c.fiber.to_lowercase().contains(&needle))
            });
            serde_json::to_value(db).map_err(|e| invalid(e.to_string()))
        }
        ["show", id] => {
            let snapshot = catalog()?.resolve(&Selection {
                id: (*id).into(),
                ..Selection::default()
            })?;
            Ok(
                json!({"snapshot":snapshot,"selection":snapshot.frozen_selection(),
                "missing_mechanics":snapshot.material().missing_mechanics(),
                "simulation_ready":snapshot.mechanics().is_ok()}),
            )
        }
        ["resolve", path] => {
            let pattern = Pattern::new(read(path)?);
            Ok(material::resolve(&pattern, &DrapeOptions::default())?.report)
        }
        ["bind", pattern, selection, out] => bind(read(pattern)?, &read(selection)?, out),
        ["price", pattern, quotes, scope] => {
            let m = selected(&read(pattern)?)?;
            let result = read::<QuoteBook>(quotes)?.evaluate(&m, &read::<Scope>(scope)?)?;
            serde_json::to_value(result).map_err(|e| invalid(e.to_string()))
        }
        ["thermal", path] => {
            let input: StackInput = read(path)?;
            let db = catalog()?;
            let layers = input
                .layers
                .iter()
                .map(|s| db.resolve(s))
                .collect::<Result<Vec<_>, _>>()?;
            // Enforce EACH layer's evidence policy before evaluation, rather than weakening all of
            // them when just one layer grants consent. The validated snapshots are immutable.
            for layer in &layers {
                layer.thermal_resistance()?;
            }
            let refs: Vec<_> = layers.iter().map(|l| l.material()).collect();
            let result = ThermalStack::evaluate(
                &refs,
                input.area_m2,
                input.inside_c,
                input.outside_c,
                input.boundary_resistance_m2_k_w,
                true,
            )?;
            Ok(
                json!({"dry_1d_stack":result,"layers":layers,"comfort_claim":false,
                "area_m2":input.area_m2,"inside_c":input.inside_c,"outside_c":input.outside_c,
                "boundary_resistance_m2_k_w":input.boundary_resistance_m2_k_w,
                "not_simulated":["wind","air gaps","moisture","compression","human thermoregulation"]}),
            )
        }
        _ => Err(invalid(USAGE)),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(2)
            }
        },
        Err(error) => {
            eprintln!("{}", json!({"refusal":error}));
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_produce_a_locked_selection_for_the_actual_drape_resolver() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/materials/synthetic-swatch.json"
        );
        let denied = run(&["calibrate".into(), path.into()]).unwrap();
        assert_eq!(denied["simulation_ready"], false);
        assert_eq!(denied["selection"]["allow_estimates"], false);
        let out = run(&["calibrate".into(), path.into(), "--allow-estimates".into()]).unwrap();
        assert_eq!(out["simulation_ready"], true);
        assert_eq!(out["fit_claim"], false);
        let selection: Selection = serde_json::from_value(out["selection"].clone()).unwrap();
        let frozen = catalog().unwrap().resolve(&selection).unwrap();
        assert_eq!(Some(frozen.sha256()), selection.expected_sha256.as_deref());
        let p = Pattern::new(json!({"fabric":{"material":selection}}));
        let physical = material::resolve(&p, &DrapeOptions::default()).unwrap();
        let q = physical.fabric.membrane(1.0).q;
        assert!((q[0][0] - 9000.0).abs() < 1e-8);
        assert!((q[1][1] - 4000.0).abs() < 1e-8);
        assert!((q[2][2] - 700.0).abs() < 1e-8);
        assert_eq!(physical.options.contact.thickness, 0.00035);
        assert_eq!(out["calibration"]["check"]["components"], 4);
    }

    #[test]
    fn calibration_does_not_accept_an_unrecognized_permission_flag() {
        assert!(
            run(&[
                "calibrate".into(),
                "unused.json".into(),
                "--allow-anything".into()
            ])
            .is_err()
        );
    }

    #[test]
    fn discovery_and_real_drape_refusals_are_exposed() {
        let rows = run(&["catalog".into(), "bamboo".into()]).unwrap();
        assert_eq!(rows["materials"].as_array().unwrap().len(), 2);
        let shown = run(&["show".into(), "fs_il019_natural".into()]).unwrap();
        assert_eq!(shown["simulation_ready"], false);
        assert!(
            !shown["selection"]["expected_sha256"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert!(run(&["show".into(), "misspelled".into()]).is_err());
    }

    #[test]
    fn binding_preserves_geometry_and_never_overwrites() {
        let path = std::env::temp_dir().join(format!(
            "ftx-material-bind-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let out = path.to_str().unwrap();
        let raw = json!({"schema":"fabric-pattern/1","units":"mm","style":"test", "seams":[],
            "fabric":{"areal_g_m2":200,"user_note":"preserve me"},
            "pieces":[{"id":"test","material":"shell","vertices":{"a":[0,0],"b":[10,0],"c":[0,10]},
                "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},
                    {"id":"ca","from":"c","to":"a"}],"cut":{"qty":1},"grain":[[0,0],[0,10]]}]});
        let selection = Selection {
            id: "fs_il019_natural".into(),
            ..Selection::default()
        };
        bind(raw.clone(), &selection, out).unwrap();
        let saved: Value = read(out).unwrap();
        assert_eq!(raw["pieces"], saved["pieces"]);
        assert_eq!(saved["fabric"]["user_note"], "preserve me");
        assert!(saved["fabric"].get("areal_g_m2").is_none());
        assert!(selected(&saved).is_ok());
        let before = std::fs::read(&path).unwrap();
        assert!(bind(raw, &selection, out).is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
        // Keep the test output: this repository forbids deleting files without explicit permission.
    }
}

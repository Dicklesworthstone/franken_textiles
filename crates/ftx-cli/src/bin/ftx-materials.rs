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
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::material_cost::{QuoteBook, Scope};
use ftx_pattern::materials::ThermalStack;
use ftx_pattern::textile_import::{catalog, resolve_fabric};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[path = "../materials/stock.rs"]
mod finite_stock;
#[path = "../materials/nonlinear.rs"]
mod nonlinear_calibration;
#[path = "../materials/scenario.rs"]
mod scenarios;
#[path = "../materials/screen.rs"]
mod screening;
#[path = "../materials/thermal.rs"]
mod steady_workbench;
#[path = "../materials/strip.rs"]
mod strips;
#[path = "material_workbench/coverage.rs"]
mod thermal_coverage;
#[path = "material_workbench/motion.rs"]
mod thermal_motion;
#[path = "material_workbench/projection.rs"]
mod thermal_projection;
#[path = "material_workbench/surface.rs"]
mod thermal_surface;
#[path = "material_workbench/thermal.rs"]
mod thermal_transient;

#[path = "material_workbench/bending.rs"]
mod bending_calibration;
#[path = "material_workbench/cutting.rs"]
mod cutting;
#[path = "material_workbench/friction.rs"]
mod friction_calibration;
#[path = "material_workbench/membrane_plasticity.rs"]
mod membrane_plasticity_calibration;
#[path = "material_workbench/relaxation.rs"]
mod relaxation_calibration;

const USAGE: &str = "ftx-materials catalog [SEARCH]
ftx-materials screen REQUEST.json
ftx-materials study PATTERN.json REQUEST.json
ftx-materials scenario-study PATTERN.json STUDY.json SCENARIO.json MEASUREMENTS.json MODEL.ftxbody
ftx-materials stock-plan PATTERN.json STOCK.json
ftx-materials stock-identities PATTERN.json STOCK.json
ftx-materials stock-price PATTERN.json STOCK.json PRICING.json
ftx-materials stock-residual PATTERN.json STOCK.json
ftx-materials stock-svg PATTERN.json STOCK.json STOCK-INDEX NEW.svg
ftx-materials show ID [--allow-estimates]
ftx-materials resolve PATTERN.json
ftx-materials bind PATTERN.json SELECTION.json NEW-PATTERN.json
ftx-materials bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json
ftx-materials price PATTERN.json QUOTES.json SCOPE.json
ftx-materials thermal STACK.json
ftx-materials thermal-at ID TEMPERATURE-C [--allow-estimates]
ftx-materials pressure-condition REQUEST.json
ftx-materials compression-stack REQUEST.json
ftx-materials calibrate SWATCHES.json [--allow-estimates]
ftx-materials calibrate-nonlinear SWATCHES.json [--allow-estimates]
ftx-materials calibrate-relaxation HISTORIES.json [--allow-estimates]
ftx-materials calibrate-membrane-plasticity HISTORIES.json [--allow-estimates]
ftx-materials strip-acquisition ACQUISITION.json
ftx-materials strip-experiment REQUEST.json
ftx-materials strip-history REQUEST.json
ftx-materials calibrate-strip REQUEST.json [--allow-estimates]
ftx-materials calibrate-bending COUPONS.json [--allow-estimates]
ftx-materials calibrate-friction SLIDING.json [--allow-estimates]
ftx-materials thermal-transient STACK.json
ftx-materials thermal-compression HISTORY.json
ftx-materials thermal-vapor STACK.json
ftx-materials thermal-vapor-surface SIM_REPORT.json CONDITIONS.json
ftx-materials thermal-steady STACK.json
ftx-materials thermal-steady-surface SIM_REPORT.json CONDITIONS.json
ftx-materials thermal-steady-coverage REQUEST.json
ftx-materials thermal-steady-coverage SIM_REPORT.json MAP.json CONDITIONS.json
ftx-materials thermal-surface SIM_REPORT.json CONDITIONS.json
ftx-materials thermal-coverage SIM_REPORT.json COVERAGE.json CONDITIONS.json
ftx-materials thermal-map SIM_REPORT.json BODY.obj PROJECTION.json
ftx-materials thermal-motion MOTION.json
ftx-materials cut-plan PATTERN.json OPTIONS.json
ftx-materials cut-price PATTERN.json OPTIONS.json QUOTES.json MARKET.json
ftx-materials cut-svg PATTERN.json OPTIONS.json ROLL-INDEX NEW.svg
ftx-materials sew-plan PATTERN.json OPTIONS.json
ftx-materials sew-stitches PATTERN.json OPTIONS.json
ftx-materials sew-svg PATTERN.json OPTIONS.json OPERATION-ID NEW.svg

JSON output. `show` includes a portable selection, missing-property coverage and separate steady/transient readiness.
`resolve` invokes the actual drape resolver without meshing; missing mechanics refuse.
`bind` preserves geometry and embeds a locked selection; its output path must not exist.
`bind-changes` atomically replaces named/default selections and optional exact pair-friction evidence, with separate mechanical readiness; its output path must not exist.
`price` is a dated offer sample, not a checkout or a worldwide market price.
`thermal` is a dry 1D layer calculation, not a human comfort or survival rating.
`thermal-compression` retains material heat state across qualified gap changes and thermal holds; mechanical work is not converted to heat.
`thermal-vapor` computes potential evaporative exchange from source-qualified vapor resistances and explicit boundary pressures; it does not infer sweat availability, condensation or wearer comfort.
`thermal-vapor-surface` evaluates independent sheet potentials on retained current drape geometry, without requiring dry heat or storage data; it does not establish actual wearer evaporation.
`thermal-at` evaluates a complete source-qualified effective sheet at one temperature, never extrapolating.
`pressure-condition` prepares a frozen material at a declared normal pressure from qualified observations; it does not infer contact pressure.
`compression-stack` infers common pressure from a rigid-face stack clearance and qualified branches, returning one frozen selection per layer; it is not a garment-contact or recovery simulation.
`thermal-steady` solves source-qualified conduction without requiring mass or heat capacity; its frozen request replays offline.
`thermal-steady-surface` evaluates source-locked drape triangles without requiring heat storage; no inferred body coverage.
`thermal-steady-coverage` recomputes layered body coverage and heat directly from retained drape materials, without invented storage.
`calibrate` fits observed swatches, retaining the receipt and a portable locked selection.
`calibrate-nonlinear` identifies signed nonlinear membrane curves from nominal forces without extrapolating.
`calibrate-membrane-plasticity` identifies permanent stretch and hardening from source-qualified loading cycles; unresolved fits retain their report without publishing a selection.
`strip-acquisition` preserves the complete retained source curves, protocol, metadata and hashes without inferring local strain or transverse width.
`strip-experiment` runs the production planar membrane with declared ideal grips and reports total reactions and equilibrium residuals.
`strip-history` retains production membrane history and inertia through prescribed grip loading, holds and unloading, with separate stored energy and physical dissipation.
`calibrate-strip` identifies bounded coefficients from grip forces and optional measured widths; rejected candidates retain their diagnostics without a selection. --allow-estimates controls admission of fitted coefficients; the request separately owns allow_estimated_observations.
`calibrate-bending` fits source-bearing moment/curvature coupons into the panel-bending tensor.
`calibrate-friction` identifies explicit pair/self/body kinetic coefficients from corrected steady sliding forces.
`thermal-surface` calculates dry heat maps on frozen drape geometry, not whole-wearer comfort.
`thermal-coverage` couples mapped layers and bare body areas; generated geometry receipts are replayed.
`thermal-map` derives coverage polygons from actual cloth and body geometry under a local-normal closure.
`thermal-motion` evolves a timed sequence of verified maps, conservatively remaps heat, and can resume saved states.
`cut-plan` derives material-specific roll requirements from real cut instances and sourced width.
`stock-plan` allocates every cutter to finite hide/remnant polygons, with defect exclusions and exact material matching.
`stock-residual` emits the next frozen request with accepted cuts retained as occupied physical stock.
`stock-svg` writes a new finite-stock review; stock boundaries/defects are not machine cut paths.
`cut-price` prices gross marker stock, including layout waste; missing offers remain unknown.
`cut-svg` writes a new source-locked planning preview, not device execution code.
`sew-plan` checks material thickness, allowances, local feed and multi-pass/reverse/backtack order.
`sew-stitches` expands all directed stitch travel in paired pattern frames; neither command authorizes hardware.
`sew-svg` writes a new operation preview with source/offset paths and needle stations; never a machine file.
Fitted coefficients are estimates: --allow-estimates explicitly admits them for commands listing that flag; forward strip requests retain their frozen selection's policy.";

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

fn simulation_readiness(material: &ResolvedMaterial) -> Result<(), Refusal> {
    material.mechanics()?;
    material.membrane_relaxation()?;
    material.membrane_plasticity()?;
    material.bending_response()?;
    material.bending_plasticity()?;
    Ok(())
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
        "simulation_ready":simulation_readiness(&snapshot).is_ok(),"geometry_changed":false}))
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
    let mechanics = simulation_readiness(&snapshot);
    Ok(
        json!({"calibration":report,"selection":snapshot.frozen_selection(),
        "simulation_ready":mechanics.is_ok(),"mechanics_refusal":mechanics.err(),
        "fit_claim":false,"calibration_observations_are_not_independent_garment_validation":true}),
    )
}

fn run(args: &[String]) -> Result<Value, Refusal> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["calibrate-nonlinear", path] => nonlinear_calibration::run(path, false),
        ["calibrate-nonlinear", path, "--allow-estimates"] => {
            nonlinear_calibration::run(path, true)
        }
        ["calibrate-friction", path] => friction_calibration::run(path, false),
        ["calibrate-friction", path, "--allow-estimates"] => friction_calibration::run(path, true),
        ["screen", path] => screening::run(path),
        ["strip-acquisition", path] => strips::acquisition(path),
        ["strip-experiment", path] => strips::experiment(path),
        ["strip-history", path] => strips::history(path),
        ["calibrate-strip", path] => strips::run(path, false),
        ["calibrate-strip", path, "--allow-estimates"] => strips::run(path, true),
        ["pressure-condition", path] => screening::pressure_condition(path),
        ["compression-stack", path] => screening::compression_stack(path),
        ["study", pattern, request] => screening::study(pattern, request),
        [
            "scenario-study",
            pattern,
            request,
            scenario,
            measurements,
            model,
        ] => scenarios::run(pattern, request, scenario, measurements, model),
        ["stock-plan", pattern, request] => finite_stock::plan(pattern, request),
        ["stock-identities", pattern, request] => finite_stock::identities(pattern, request),
        ["stock-price", pattern, request, pricing] => {
            finite_stock::price(pattern, request, pricing)
        }
        ["stock-residual", pattern, request] => finite_stock::residual(pattern, request),
        ["stock-svg", pattern, request, index, out] => {
            finite_stock::preview(pattern, request, index, out)
        }
        ["thermal-at", id, t] => thermal_transient::point(id, t, false),
        ["thermal-at", id, t, "--allow-estimates"] => thermal_transient::point(id, t, true),
        ["calibrate-bending", path] => bending_calibration::run(path, false),
        ["calibrate-bending", path, "--allow-estimates"] => bending_calibration::run(path, true),
        ["calibrate-membrane-plasticity", path] => {
            membrane_plasticity_calibration::run(path, false)
        }
        ["calibrate-membrane-plasticity", path, "--allow-estimates"] => {
            membrane_plasticity_calibration::run(path, true)
        }
        ["calibrate-relaxation", path] => relaxation_calibration::run(path, false),
        ["calibrate-relaxation", path, "--allow-estimates"] => {
            relaxation_calibration::run(path, true)
        }
        ["cut-plan", pattern, options] => cutting::plan(pattern, options),
        ["sew-plan", pattern, options] => {
            let pattern = Pattern::new(read::<Value>(pattern)?);
            let options = read::<ftx_export::sewing::SewOptions>(options)?;
            serde_json::to_value(ftx_export::sewing::SewPlan::build(&pattern, &options)?)
                .map_err(|e| invalid(e.to_string()))
        }
        ["sew-stitches", pattern, options] => {
            let pattern = Pattern::new(read::<Value>(pattern)?);
            let options = read::<ftx_export::sewing::SewOptions>(options)?;
            serde_json::to_value(ftx_export::sewing::program::SewProgram::build(
                &pattern, &options,
            )?)
            .map_err(|e| invalid(e.to_string()))
        }
        ["sew-svg", pattern, options, operation, out] => {
            let pattern = Pattern::new(read::<Value>(pattern)?);
            let options = read::<ftx_export::sewing::SewOptions>(options)?;
            let svg = ftx_export::sewing::preview::svg(&pattern, &options, operation)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out)
                .map_err(|e| io(out, e))?;
            file.write_all(svg.as_bytes()).map_err(|e| io(out, e))?;
            Ok(json!({"out":out,"operation":operation,
                "svg_sha256":ftx_core::receipt::sha256_hex(svg.as_bytes()),
                "hardware_execution_authorized":false}))
        }
        ["cut-price", pattern, options, quotes, market] => {
            cutting::price(pattern, options, quotes, market)
        }
        ["cut-svg", pattern, options, index, out] => cutting::preview(pattern, options, index, out),
        ["thermal-surface", report, conditions] => thermal_surface::run(report, conditions),
        ["thermal-coverage", report, mapping, conditions] => {
            thermal_coverage::run(report, mapping, conditions)
        }
        ["thermal-map", report, body, settings] => thermal_projection::run(report, body, settings),
        ["thermal-motion", schedule] => thermal_motion::run(schedule),
        ["thermal-transient", path] => thermal_transient::run(path),
        ["thermal-compression", path] => steady_workbench::compression_history(path),
        ["thermal-vapor", path] => steady_workbench::vapor(path),
        ["thermal-vapor-surface", report, conditions] => {
            steady_workbench::vapor_surface(report, conditions)
        }
        ["thermal-steady", path] => steady_workbench::stack(path),
        ["thermal-steady-surface", report, conditions] => {
            steady_workbench::surface(report, conditions)
        }
        ["thermal-steady-coverage", path] => steady_workbench::coverage_request(path),
        ["thermal-steady-coverage", report, map, conditions] => {
            steady_workbench::coverage(report, map, conditions)
        }
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
        ["show", id] | ["show", id, "--allow-estimates"] => {
            let snapshot = catalog()?.resolve(&Selection {
                id: (*id).into(),
                allow_estimates: words.len() == 3,
                ..Selection::default()
            })?;
            let steady = ftx_pattern::material_thermal::ConductionLaw::from_material(
                snapshot.material(),
                snapshot.allows_estimates(),
            )
            .map(|law| law.domain());
            let transient = ftx_pattern::material_thermal::ThermalLaw::from_material(
                snapshot.material(),
                snapshot.allows_estimates(),
            )
            .map(|law| law.domain());
            let readiness = |result: Result<Option<[f64; 2]>, Refusal>| match result {
                Ok(domain) => json!({"ready":true,"temperature_domain_c":domain}),
                Err(refusal) => json!({"ready":false,"refusal":refusal}),
            };
            Ok(
                json!({"snapshot":snapshot,"selection":snapshot.frozen_selection(),
                "missing_mechanics":snapshot.material().missing_mechanics(),
                "steady_conduction":readiness(steady),"transient_thermal":readiness(transient),
                "simulation_ready":simulation_readiness(&snapshot).is_ok()}),
            )
        }
        ["resolve", path] => {
            let pattern = Pattern::new(read(path)?);
            Ok(material::resolve(&pattern, &DrapeOptions::default())?.report)
        }
        ["bind", pattern, selection, out] => bind(read(pattern)?, &read(selection)?, out),
        ["bind-changes", pattern, changes, out] => screening::bind_changes(pattern, changes, out),
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
        let ids: Vec<_> = rows["materials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        for id in [
            "fiber/bamboo-viscose",
            "nf_bamboo_viscose_jersey_240",
            "blackbird/FT4148999",
        ] {
            assert!(ids.contains(&id), "missing bamboo search result: {id}");
        }
        assert!(!ids.contains(&"fs_il019_natural"));
        assert_eq!(run(&["catalog".into(), "BAMBOO".into()]).unwrap(), rows);
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

//! Inspect the built-in sourced textile catalog and freeze selections for downstream physics.
use super::{Args, bad_input, known_options};
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_cost::{QuoteBook, Scope};
use ftx_pattern::materials::ThermalStack;
use ftx_pattern::textile_import;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write, path::Path};

#[path = "bin/material_workbench/bending.rs"]
mod bending_calibration;
#[path = "bin/material_workbench/calibration.rs"]
mod calibration;
#[path = "bin/material_workbench/cutting.rs"]
mod cutting;
#[path = "bin/material_workbench/friction.rs"]
mod friction_calibration;
#[path = "bin/material_workbench/membrane_plasticity.rs"]
mod membrane_plasticity;
mod nonlinear;
#[path = "bin/material_workbench/relaxation.rs"]
mod relaxation;
mod scenario;
mod screen;
mod sewing_selection;
mod stock;
mod strip;
mod thermal;
#[path = "bin/material_workbench/coverage.rs"]
mod thermal_coverage;
#[path = "bin/material_workbench/motion.rs"]
mod thermal_motion;
#[path = "bin/material_workbench/projection.rs"]
mod thermal_projection;
#[path = "bin/material_workbench/surface.rs"]
mod thermal_surface;
#[path = "bin/material_workbench/thermal.rs"]
mod thermal_transient;

use calibration::{read, simulation_readiness};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn io(path: &str, error: std::io::Error) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{path}: {error}"))
}

fn selected(raw: &Value) -> Result<ftx_pattern::material_catalog::ResolvedMaterial, Refusal> {
    if !raw.is_object() || !raw["fabric"].is_object() {
        return Err(invalid(
            "pattern needs an object-valued fabric with a material selection",
        ));
    }
    if let Some(pieces) = raw["pieces"].as_array() {
        for p in pieces {
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
    textile_import::resolve_fabric(&raw["fabric"])?
        .ok_or_else(|| invalid("pattern has no explicit material selection"))
}

fn bind_material(raw: Value, selection: &Selection, out: &str) -> Result<Value, Refusal> {
    let pattern = Pattern::new(raw);
    let errors = pattern.structure_errors();
    if !errors.is_empty() {
        return Err(invalid(errors.join("; ")));
    }
    let snapshot = textile_import::catalog()?.resolve(selection)?;
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

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let action = args.positional.get(1).map(String::as_str).unwrap_or("list");
    if action == "inspect" {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials inspect SELECTION.json [--json]",
            ));
        };
        let selected = textile_import::catalog()?.resolve(&read::<Selection>(path)?)?;
        let result = selected.inspection()?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "linearize-origin" {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials linearize-origin REQUEST.json [--json]",
            ));
        };
        let result = calibration::linearize_origin(path)?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(action, "cut-plan" | "cut-job" | "cut-price" | "cut-svg") {
        known_options(args, "materials", &[])?;
        let result = match (action, &args.positional[2..]) {
            ("cut-plan", [pattern, options]) => cutting::plan(pattern, options)?,
            ("cut-job", [pattern, request]) => cutting::job(pattern, request)?,
            ("cut-price", [pattern, options, quotes, market]) => {
                cutting::price(pattern, options, quotes, market)?
            }
            ("cut-svg", [pattern, options, index, out]) => {
                cutting::preview(pattern, options, index, out)?
            }
            _ => {
                return Err(bad_input(
                    "usage: ftx materials cut-plan PATTERN.json OPTIONS.json | cut-job PATTERN.json REQUEST.json | cut-price PATTERN.json OPTIONS.json QUOTES.json MARKET.json | cut-svg PATTERN.json OPTIONS.json INDEX NEW.svg",
                ));
            }
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "strip-acquisition" {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials strip-acquisition ACQUISITION.json [--json]",
            ));
        };
        let result = strip::acquisition(path)?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "sew-select" {
        known_options(args, "materials", &[])?;
        let [pattern, request] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials sew-select PATTERN.json REQUEST.json [--json]",
            ));
        };
        return sewing_selection::run(pattern, request);
    }
    if matches!(action, "sew-plan" | "sew-stitches") {
        known_options(args, "materials", &[])?;
        let [pattern, options] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials sew-plan|sew-stitches PATTERN.json OPTIONS.json [--json]",
            ));
        };
        let result = sewing_selection::plan(pattern, options, action == "sew-stitches")?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "catalog" {
        known_options(args, "materials", &[])?;
        if args.positional.len() > 3 {
            return Err(bad_input("usage: ftx materials catalog [SEARCH] [--json]"));
        }
        let needle = args
            .positional
            .get(2)
            .map_or_else(String::new, |s| s.to_lowercase());
        let mut db = textile_import::catalog()?;
        db.materials.retain(|m| {
            m.id.to_lowercase().contains(&needle)
                || m.name.to_lowercase().contains(&needle)
                || m.composition
                    .iter()
                    .any(|c| c.fiber.to_lowercase().contains(&needle))
        });
        let result = serde_json::to_value(db).map_err(|e| bad_input(e.to_string()))?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "resolve" {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials resolve PATTERN.json [--json]",
            ));
        };
        let pattern = Pattern::new(read(path)?);
        let result = material::resolve(&pattern, &DrapeOptions::default())?.report;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "bind" {
        known_options(args, "materials", &[])?;
        let [pattern, selection, out] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials bind PATTERN.json SELECTION.json NEW-PATTERN.json [--json]",
            ));
        };
        let result = bind_material(read(pattern)?, &read(selection)?, out)?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "price" {
        known_options(args, "materials", &[])?;
        let [pattern, quotes, scope] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials price PATTERN.json QUOTES.json SCOPE.json [--json]",
            ));
        };
        let m = selected(&read(pattern)?)?;
        let evaluated = read::<QuoteBook>(quotes)?.evaluate(&m, &read::<Scope>(scope)?)?;
        let result = serde_json::to_value(evaluated).map_err(|e| bad_input(e.to_string()))?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "thermal" {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials thermal STACK.json [--json]",
            ));
        };
        let input: StackInput = read(path)?;
        let db = textile_import::catalog()?;
        let layers = input
            .layers
            .iter()
            .map(|s| db.resolve(s))
            .collect::<Result<Vec<_>, _>>()?;
        for layer in &layers {
            layer.thermal_resistance()?;
        }
        let refs: Vec<_> = layers.iter().map(|l| l.material()).collect();
        let dry = ThermalStack::evaluate(
            &refs,
            input.area_m2,
            input.inside_c,
            input.outside_c,
            input.boundary_resistance_m2_k_w,
            true,
        )?;
        let result = json!({
            "dry_1d_stack": dry,
            "layers": layers,
            "comfort_claim": false,
            "area_m2": input.area_m2,
            "inside_c": input.inside_c,
            "outside_c": input.outside_c,
            "boundary_resistance_m2_k_w": input.boundary_resistance_m2_k_w,
            "not_simulated": ["wind", "air gaps", "moisture", "compression", "human thermoregulation"]
        });
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "thermal-at" {
        known_options(args, "materials", &["allow-estimates"])?;
        let [id, temperature] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials thermal-at ID TEMPERATURE-C [--allow-estimates] [--json]",
            ));
        };
        let result = thermal_transient::point(
            id,
            temperature,
            args.options.contains_key("allow-estimates"),
        )?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "sew-svg" {
        known_options(args, "materials", &[])?;
        let [pattern, options, operation, out] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials sew-svg PATTERN.json OPTIONS.json OPERATION-ID NEW.svg [--json]",
            ));
        };
        let pattern = Pattern::new(read::<Value>(pattern)?);
        let options = read::<ftx_export::sewing::SewOptions>(options)?;
        let svg = ftx_export::sewing::preview::svg(&pattern, &options, operation)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)
            .map_err(|e| io(out, e))?;
        file.write_all(svg.as_bytes()).map_err(|e| io(out, e))?;
        let result = json!({"out":out,"operation":operation,
            "svg_sha256":ftx_core::receipt::sha256_hex(svg.as_bytes()),
            "hardware_execution_authorized":false});
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "sew-cell-svg" {
        known_options(args, "materials", &[])?;
        let [pattern, options, cell, operation, out] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials sew-cell-svg PATTERN.json OPTIONS.json CELL-ID OPERATION-ID NEW.svg [--json]",
            ));
        };
        let pattern = ftx_pattern::Pattern::new(read::<Value>(pattern)?);
        let options = read::<ftx_export::sewing::SewOptions>(options)?;
        let svg = ftx_export::sewing::preview::svg_cell(&pattern, &options, cell, operation)?;
        let io = |error| Refusal::new(RefusalCode::Io, format!("{out}: {error}"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)
            .map_err(io)?;
        file.write_all(svg.as_bytes()).map_err(io)?;
        let result = json!({"out":out,"cell":cell,"operation":operation,
            "svg_sha256":ftx_core::receipt::sha256_hex(svg.as_bytes()),
            "hardware_execution_authorized":false});
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(
        action,
        "strip-experiment" | "strip-history" | "cylinder-experiment"
    ) {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials strip-experiment|strip-history|cylinder-experiment REQUEST.json",
            ));
        };
        let result = match action {
            "strip-history" => strip::history(path)?,
            "cylinder-experiment" => strip::cylinder(path)?,
            _ => strip::experiment(path)?,
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(
        action,
        "calibrate"
            | "calibrate-bending"
            | "calibrate-friction"
            | "calibrate-nonlinear"
            | "calibrate-relaxation"
            | "calibrate-membrane-plasticity"
            | "calibrate-strip"
            | "calibrate-cylinder"
    ) {
        known_options(args, "materials", &["allow-estimates"])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials calibrate|calibrate-nonlinear SWATCHES.json | calibrate-bending COUPONS.json | calibrate-friction SLIDING.json | calibrate-relaxation HISTORIES.json | calibrate-membrane-plasticity HISTORIES.json | calibrate-strip|calibrate-cylinder REQUEST.json [--allow-estimates]",
            ));
        };
        let allow = args.options.contains_key("allow-estimates");
        let result = match action {
            "calibrate" => calibration::run(path, allow)?,
            "calibrate-bending" => bending_calibration::run(path, allow)?,
            "calibrate-friction" => friction_calibration::run(path, allow)?,
            "calibrate-relaxation" => relaxation::run(path, allow)?,
            "calibrate-membrane-plasticity" => membrane_plasticity::run(path, allow)?,
            "calibrate-strip" => strip::run(path, allow)?,
            "calibrate-cylinder" => strip::calibrate_cylinder(path, allow)?,
            _ => nonlinear::run(path, allow)?,
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "study-on-body" {
        known_options(args, "materials", &[])?;
        let [pattern, request, body, model] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials study-on-body PATTERN.json STUDY.json BODY.json MODEL.ftxbody",
            ));
        };
        let result = scenario::manufacturing(pattern, request, body, model)?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if action == "scenario-study" {
        known_options(args, "materials", &[])?;
        let [pattern, request, scenario, measurements, model] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials scenario-study PATTERN.json STUDY.json SCENARIO.json MEASUREMENTS.json MODEL.ftxbody",
            ));
        };
        let result = scenario::run(pattern, request, scenario, measurements, model)?;
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(
        action,
        "stock-plan" | "stock-residual" | "stock-svg" | "stock-identities" | "stock-price"
    ) {
        known_options(args, "materials", &[])?;
        let result = match (action, &args.positional[2..]) {
            ("stock-plan", [pattern, request]) => stock::plan(pattern, request)?,
            ("stock-identities", [pattern, request]) => stock::identities(pattern, request)?,
            ("stock-price", [pattern, request, pricing]) => {
                stock::price(pattern, request, pricing)?
            }
            ("stock-residual", [pattern, request]) => stock::residual(pattern, request)?,
            ("stock-svg", [pattern, request, index, out]) => {
                stock::preview(pattern, request, index, out)?
            }
            _ => {
                return Err(bad_input(
                    "usage: ftx materials stock-plan|stock-identities|stock-residual PATTERN.json STOCK.json | stock-price PATTERN.json STOCK.json PRICING.json | stock-svg PATTERN.json STOCK.json INDEX NEW.svg",
                ));
            }
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(
        action,
        "screen"
            | "study"
            | "pressure-condition"
            | "compression-stack"
            | "bind-changes"
            | "treatment"
            | "treatment-pattern"
    ) {
        known_options(args, "materials", &[])?;
        let result = match (action, &args.positional[2..]) {
            ("screen", [path]) => screen::run(path)?,
            ("pressure-condition", [path]) => screen::pressure_condition(path)?,
            ("compression-stack", [path]) => screen::compression_stack(path)?,
            ("treatment", [path]) => screen::treatment(path)?,
            ("treatment-pattern", [pattern, request]) => {
                screen::treatment_pattern(pattern, request, None)?
            }
            ("treatment-pattern", [pattern, request, out]) => {
                screen::treatment_pattern(pattern, request, Some(out))?
            }
            ("bind-changes", [pattern, changes, out]) => {
                screen::bind_changes(pattern, changes, out)?
            }
            ("study", [pattern, request]) => screen::study(pattern, request)?,
            _ => {
                return Err(bad_input(
                    "usage: ftx materials screen REQUEST.json | study PATTERN.json REQUEST.json | pressure-condition REQUEST.json | compression-stack REQUEST.json | treatment REQUEST.json | treatment-pattern PATTERN.json REQUEST.json [NEW-PATTERN.json] | bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json",
                ));
            }
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    if matches!(
        action,
        "thermal-transient"
            | "thermal-surface"
            | "thermal-coverage"
            | "thermal-map"
            | "thermal-motion"
            | "thermal-compression"
            | "thermal-vapor"
            | "thermal-vapor-coverage"
            | "thermal-vapor-coverage-history"
            | "thermal-vapor-surface"
            | "thermal-steady"
            | "thermal-steady-surface"
            | "thermal-steady-coverage"
    ) {
        known_options(args, "materials", &[])?;
        let result = match (action, &args.positional[2..]) {
            ("thermal-transient", [path]) => thermal_transient::run(path)?,
            ("thermal-surface", [report, conditions]) => thermal_surface::run(report, conditions)?,
            ("thermal-coverage", [report, mapping, conditions]) => {
                thermal_coverage::run(report, mapping, conditions)?
            }
            ("thermal-map", [report, body, settings]) => {
                thermal_projection::run(report, body, settings)?
            }
            ("thermal-motion", [schedule]) => thermal_motion::run(schedule)?,
            ("thermal-compression", [path]) => thermal::compression_history(path)?,
            ("thermal-vapor", [path]) => thermal::vapor(path)?,
            ("thermal-vapor-coverage", [path]) => thermal::vapor_coverage(path)?,
            ("thermal-vapor-coverage-history", [path]) => thermal::vapor_coverage_history(path)?,
            ("thermal-vapor-surface", [report, conditions]) => {
                thermal::vapor_surface(report, conditions)?
            }
            ("thermal-steady", [path]) => thermal::stack(path)?,
            ("thermal-steady-surface", [report, conditions]) => {
                thermal::surface(report, conditions)?
            }
            ("thermal-steady-coverage", [path]) => thermal::coverage_request(path)?,
            ("thermal-steady-coverage", [report, map, conditions]) => {
                thermal::coverage(report, map, conditions)?
            }
            _ => {
                return Err(bad_input(
                    "usage: ftx materials thermal-transient STACK.json | thermal-surface SIM_REPORT.json CONDITIONS.json | thermal-coverage SIM_REPORT.json COVERAGE.json CONDITIONS.json | thermal-map SIM_REPORT.json BODY.obj PROJECTION.json | thermal-motion MOTION.json | thermal-compression HISTORY.json | thermal-vapor STACK.json | thermal-vapor-coverage REQUEST.json | thermal-vapor-coverage-history HISTORY.json | thermal-vapor-surface SIM_REPORT.json CONDITIONS.json | thermal-steady STACK.json | thermal-steady-surface SIM_REPORT.json CONDITIONS.json | thermal-steady-coverage REQUEST.json | thermal-steady-coverage SIM_REPORT.json MAP.json CONDITIONS.json",
                ));
            }
        };
        let text = serde_json::to_string_pretty(&result).map_err(|e| bad_input(e.to_string()))?;
        return Ok((result, 0, text));
    }
    known_options(args, "materials", &["allow-estimates"])?;
    let catalog = textile_import::catalog()?;
    match action {
        "list" => {
            if args.positional.len() > 2 {
                return Err(bad_input("usage: ftx materials list"));
            }
            let mut entries: Vec<Value> = catalog
                .materials
                .iter()
                .map(|m| {
                    json!({"id":m.id, "name":m.name, "form":m.form,
                    "mechanics_missing":m.missing_mechanics(), "properties":m.properties.len()})
                })
                .collect();
            entries.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            let text = entries
                .iter()
                .map(|e| {
                    format!(
                        "{}  {}  ({} properties; {} mechanical gaps)",
                        e["id"].as_str().unwrap_or(""),
                        e["name"].as_str().unwrap_or(""),
                        e["properties"],
                        e["mechanics_missing"].as_array().map_or(0, Vec::len)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok((
                json!({"schema":"ftx-material-list/1","count":entries.len(),"materials":entries}),
                0,
                text,
            ))
        }
        "compare" => {
            if args.positional.len() != 4 {
                return Err(bad_input("usage: ftx materials compare FIRST_ID SECOND_ID"));
            }
            let permit = args.options.contains_key("allow-estimates");
            let select = |id: &str| {
                catalog.resolve(&Selection {
                    id: id.into(),
                    allow_estimates: permit,
                    ..Selection::default()
                })
            };
            let first = select(&args.positional[2])?;
            let second = select(&args.positional[3])?;
            let a = first.material();
            let b = second.material();
            let mut properties = serde_json::Map::new();
            for (key, x) in &a.properties {
                if let Some(y) = b.properties.get(key)
                    && x.unit == y.unit
                    && (permit
                        || (x.evidence != ftx_pattern::materials::Evidence::Estimate
                            && y.evidence != ftx_pattern::materials::Evidence::Estimate))
                {
                    properties.insert(key.clone(), json!({
                        "unit":x.unit,"first":x.value,"second":y.value,
                        "ratio_second_to_first": if x.value != 0.0 {Some(y.value / x.value)} else {None},
                        "first_evidence":x.evidence,"second_evidence":y.evidence,
                        "first_source":a.sources[&x.source],"second_source":b.sources[&y.source]
                    }));
                }
            }
            let output = json!({"schema":"ftx-material-comparison/1",
                "first":{"id":a.id,"sha256":first.sha256(),"mechanics_ready":first.mechanics().is_ok(),
                    "missing_mechanics":a.missing_mechanics()},
                "second":{"id":b.id,"sha256":second.sha256(),"mechanics_ready":second.mechanics().is_ok(),
                    "missing_mechanics":b.missing_mechanics()},
                "comparable_properties":properties,
                "note":"Observed properties; not an inferred fit, safety or thermal-comfort rating."});
            let text = serde_json::to_string_pretty(&output)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
            Ok((output, 0, text))
        }
        "show" | "freeze" => {
            let id = args
                .positional
                .get(2)
                .ok_or_else(|| bad_input("usage: ftx materials show|freeze ID"))?;
            if args.positional.len() != 3 {
                return Err(bad_input("exactly one material ID is required"));
            }
            let selected = catalog.resolve(&Selection {
                id: id.clone(),
                allow_estimates: args.options.contains_key("allow-estimates"),
                ..Selection::default()
            })?;
            let missing = selected.material().missing_mechanics();
            let result = if action == "freeze" {
                json!({"schema":"ftx-material-selection/1", "selection":selected.frozen_selection(),
                    "mechanics_missing":missing, "sha256":selected.sha256()})
            } else {
                let steady = ftx_pattern::material_thermal::ConductionLaw::from_material(
                    selected.material(),
                    selected.allows_estimates(),
                )
                .map(|law| law.domain());
                let transient = ftx_pattern::material_thermal::ThermalLaw::from_material(
                    selected.material(),
                    selected.allows_estimates(),
                )
                .map(|law| law.domain());
                let thermal_ready = steady.is_ok();
                let readiness = |result: Result<Option<[f64; 2]>, Refusal>| match result {
                    Ok(domain) => json!({"ready":true,"temperature_domain_c":domain}),
                    Err(refusal) => json!({"ready":false,"refusal":refusal}),
                };
                let sewing = match selected.unloaded_thickness() {
                    Ok(quantity) => json!({"ready":true,"value":quantity.value}),
                    Err(refusal) => json!({"ready":false,"refusal":refusal}),
                };
                json!({"schema":"ftx-material-detail/1", "material":selected.material(),
                    "mechanics_missing":missing, "sha256":selected.sha256(),
                    "mechanics_ready":selected.mechanics().is_ok(),
                    "sewing_thickness_m":sewing,
                    "steady_conduction":readiness(steady),"transient_thermal":readiness(transient),
                    "thermal_ready":thermal_ready})
            };
            let text = serde_json::to_string_pretty(&result)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
            Ok((result, 0, text))
        }
        _ => Err(bad_input(
            "usage: ftx materials [list|catalog [SEARCH]|show ID|freeze ID|compare FIRST_ID SECOND_ID|inspect SELECTION.json|resolve PATTERN.json|bind PATTERN.json SELECTION.json NEW-PATTERN.json|bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json|price PATTERN.json QUOTES.json SCOPE.json|screen REQUEST.json|study PATTERN.json REQUEST.json|study-on-body ...|scenario-study ...|calibrate* ...|strip-* ...|cylinder-experiment ...|thermal* ...|cut-* ...|stock-* ...|sew-* ...]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_is_source_backed_and_unknown_ids_refuse() {
        let c = textile_import::catalog().unwrap();
        let ids: Vec<String> = c.materials.iter().take(2).map(|m| m.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        let a = super::super::parse(&[
            "materials".into(),
            "compare".into(),
            ids[0].clone(),
            ids[1].clone(),
        ])
        .unwrap();
        let (comparison, status, _) = run(&a).unwrap();
        assert_eq!(status, 0);
        assert_eq!(comparison["first"]["id"], ids[0]);
        assert_eq!(comparison["second"]["id"], ids[1]);
        let unknown = super::super::parse(&[
            "materials".into(),
            "compare".into(),
            ids[0].clone(),
            "absent-material-identifier".into(),
        ])
        .unwrap();
        assert!(run(&unknown).is_err());
    }
    #[test]
    fn catalog_discovery_and_reproducible_selection() {
        let list = super::super::parse(&["materials".into(), "list".into()]).unwrap();
        let (v, status, _) = run(&list).unwrap();
        assert_eq!(status, 0);
        assert!(v["count"].as_u64().unwrap() > 0);
        let id = v["materials"][0]["id"].as_str().unwrap().to_owned();
        let show = super::super::parse(&["materials".into(), "show".into(), id.clone()]).unwrap();
        assert_eq!(run(&show).unwrap().0["material"]["id"], id);
        let freeze = super::super::parse(&["materials".into(), "freeze".into(), id]).unwrap();
        let frozen = run(&freeze).unwrap().0;
        assert!(frozen["selection"]["expected_sha256"].is_string());
        assert_eq!(frozen["sha256"], frozen["selection"]["expected_sha256"]);
    }

    #[test]
    fn thermal_readiness_matches_the_temperature_qualified_steady_consumer() {
        let id = "nist/7282/cotton-duck";
        let show = super::super::parse(&[
            "materials".into(),
            "show".into(),
            id.into(),
            "--allow-estimates".into(),
        ])
        .unwrap();
        let (report, status, _) = run(&show).unwrap();
        assert_eq!(status, 0);
        assert_eq!(report["thermal_ready"], true);
        assert_eq!(report["steady_conduction"]["ready"], true);
        assert!(report["steady_conduction"]["temperature_domain_c"].is_array());
        assert_eq!(report["transient_thermal"]["ready"], false);
        assert_eq!(report["mechanics_ready"], false);

        let unadmitted =
            super::super::parse(&["materials".into(), "show".into(), id.into()]).unwrap();
        let report = run(&unadmitted).unwrap().0;
        assert_eq!(report["thermal_ready"], false);
        assert_eq!(report["steady_conduction"]["ready"], false);
    }
}

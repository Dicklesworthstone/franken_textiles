//! Inspect the built-in sourced textile catalog and freeze selections for downstream physics.
use super::{Args, bad_input, known_options};
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::textile_import;
use serde_json::{Value, json};

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

use calibration::{read, simulation_readiness};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let action = args.positional.get(1).map(String::as_str).unwrap_or("list");
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
    if matches!(action, "strip-experiment" | "strip-history") {
        known_options(args, "materials", &[])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials strip-experiment|strip-history REQUEST.json",
            ));
        };
        let result = if action == "strip-history" {
            strip::history(path)?
        } else {
            strip::experiment(path)?
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
    ) {
        known_options(args, "materials", &["allow-estimates"])?;
        let [path] = &args.positional[2..] else {
            return Err(bad_input(
                "usage: ftx materials calibrate|calibrate-nonlinear SWATCHES.json | calibrate-bending COUPONS.json | calibrate-friction SLIDING.json | calibrate-relaxation HISTORIES.json | calibrate-membrane-plasticity HISTORIES.json | calibrate-strip ACQUISITION.json [--allow-estimates]",
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
        "thermal-compression"
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
                    "usage: ftx materials thermal-compression HISTORY.json | thermal-vapor STACK.json | thermal-vapor-coverage REQUEST.json | thermal-vapor-coverage-history HISTORY.json | thermal-vapor-surface SIM_REPORT.json CONDITIONS.json | thermal-steady STACK.json | thermal-steady-surface SIM_REPORT.json CONDITIONS.json | thermal-steady-coverage REQUEST.json | thermal-steady-coverage SIM_REPORT.json MAP.json CONDITIONS.json",
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
                let readiness = |result: Result<Option<[f64; 2]>, Refusal>| match result {
                    Ok(domain) => json!({"ready":true,"temperature_domain_c":domain}),
                    Err(refusal) => json!({"ready":false,"refusal":refusal}),
                };
                json!({"schema":"ftx-material-detail/1", "material":selected.material(),
                    "mechanics_missing":missing, "sha256":selected.sha256(),
                    "mechanics_ready":selected.mechanics().is_ok(),
                    "steady_conduction":readiness(steady),"transient_thermal":readiness(transient),
                    "thermal_ready":selected.thermal_resistance().is_ok()})
            };
            let text = serde_json::to_string_pretty(&result)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
            Ok((result, 0, text))
        }
        _ => Err(bad_input(
            "usage: ftx materials [list|show ID|freeze ID|compare ID ID|thermal-steady STACK.json|thermal-steady-surface SIM_REPORT.json CONDITIONS.json|thermal-steady-coverage REQUEST.json]",
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
}

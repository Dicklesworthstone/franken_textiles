//! Inspect the built-in sourced textile catalog and freeze selections for downstream physics.
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::textile_import;
use serde_json::{Value, json};
use super::{Args, bad_input, known_options};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "materials", &["allow-estimates"])?;
    let catalog = textile_import::catalog()?;
    let action = args.positional.get(1).map(String::as_str).unwrap_or("list");
    match action {
        "list" => {
            if args.positional.len() > 2 { return Err(bad_input("usage: ftx materials list")); }
            let mut entries: Vec<Value> = catalog.materials.iter().map(|m| {
                json!({"id":m.id, "name":m.name, "form":m.form,
                    "mechanics_missing":m.missing_mechanics(), "properties":m.properties.len()})
            }).collect();
            entries.sort_by(|a,b| a["id"].as_str().cmp(&b["id"].as_str()));
            let text = entries.iter().map(|e| format!("{}  {}  ({} properties; {} mechanical gaps)",
                e["id"].as_str().unwrap_or(""), e["name"].as_str().unwrap_or(""),
                e["properties"], e["mechanics_missing"].as_array().map_or(0, Vec::len)))
                .collect::<Vec<_>>().join("\n");
            Ok((json!({"schema":"ftx-material-list/1","count":entries.len(),"materials":entries}), 0, text))
        }
        "compare" => {
            if args.positional.len() != 4 {
                return Err(bad_input("usage: ftx materials compare FIRST_ID SECOND_ID"));
            }
            let permit = args.options.contains_key("allow-estimates");
            let select = |id: &str| catalog.resolve(&Selection {
                id: id.into(), allow_estimates: permit, ..Selection::default()
            });
            let first = select(&args.positional[2])?;
            let second = select(&args.positional[3])?;
            let a = first.material();
            let b = second.material();
            let mut properties = serde_json::Map::new();
            for (key, x) in &a.properties {
                if let Some(y) = b.properties.get(key) {
                    if x.unit == y.unit && (permit || (
                        x.evidence != ftx_pattern::materials::Evidence::Estimate &&
                        y.evidence != ftx_pattern::materials::Evidence::Estimate)) {
                        properties.insert(key.clone(), json!({
                            "unit":x.unit,"first":x.value,"second":y.value,
                            "ratio_second_to_first": if x.value != 0.0 {Some(y.value / x.value)} else {None},
                            "first_evidence":x.evidence,"second_evidence":y.evidence,
                            "first_source":a.sources[&x.source],"second_source":b.sources[&y.source]
                        }));
                    }
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
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument,e.to_string()))?;
            Ok((output,0,text))
        }
        "show" | "freeze" => {
            let id = args.positional.get(2).ok_or_else(|| bad_input("usage: ftx materials show|freeze ID"))?;
            if args.positional.len() != 3 { return Err(bad_input("exactly one material ID is required")); }
            let selected = catalog.resolve(&Selection {
                id:id.clone(),
                allow_estimates: args.options.contains_key("allow-estimates"),
                ..Selection::default()
            })?;
            let missing = selected.material().missing_mechanics();
            let result = if action == "freeze" {
                json!({"schema":"ftx-material-selection/1", "selection":selected.frozen_selection(),
                    "mechanics_missing":missing, "sha256":selected.sha256()})
            } else {
                json!({"schema":"ftx-material-detail/1", "material":selected.material(),
                    "mechanics_missing":missing, "sha256":selected.sha256(),
                    "mechanics_ready":selected.mechanics().is_ok(),
                    "thermal_ready":selected.thermal_resistance().is_ok()})
            };
            let text = serde_json::to_string_pretty(&result)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
            Ok((result, 0, text))
        }
        _ => Err(bad_input("usage: ftx materials [list|show ID|freeze ID|compare ID ID]")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

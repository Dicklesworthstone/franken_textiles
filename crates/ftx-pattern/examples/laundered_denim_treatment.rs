//! Emit a measured transition, panel, treatment request or design study with genuine source locks.
//! Example: cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 compensate

use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let code = args
        .first()
        .cloned()
        .unwrap_or_else(|| "1".into())
        .parse::<u8>()?;
    let input = ftx_pattern::material_catalog::laundered_denim::treatment_request(code)?;
    let output = match args.get(1).map(String::as_str).unwrap_or("transition") {
        "transition" => serde_json::to_value(input)?,
        "pattern" => json!({"schema":"fabric-pattern/1","units":"mm",
            "fabric":{"material":input.before},"pieces":[{"id":"panel","sa":0.,"cut":{"qty":1},
                "vertices":{"a":[0.,0.],"b":[100.,0.],"c":[100.,200.],"d":[0.,200.]},
                "edges":[{"id":"bottom","from":"a","to":"b"},
                    {"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},
                    {"id":"left","from":"d","to":"a"}],
                "grain":[[0.,0.],[0.,100.]]}],"seams":[]}),
        mode @ ("treat" | "compensate") => json!({
            "schema":"ftx-pattern-treatment/1","mode":mode,
            "transitions":{"wash":input},"pieces":{"panel":"wash"}}),
        "study" => {
            let operation = |mode| {
                json!({"op":"treatment","request":{
                "schema":"ftx-pattern-treatment/1","mode":mode,
                "transitions":{"wash":input},"pieces":{"panel":"wash"}}})
            };
            json!({"schema":"ftx-design-study/1",
                "designs":[
                    {"key":"washed","operations":[operation("treat")]},
                    {"key":"cutting-compensation","operations":[operation("compensate")]},
                    {"key":"compensated-then-washed","operations":[operation("compensate"),operation("treat")]}],
                "variants":[{"key":"retained-state","changes":{"schema":"ftx-material-bindings/1"}}],
                "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,
                    "extras":[],"seam_allowance_mm":0.0}})
        }
        _ => return Err("choose transition, pattern, treat, compensate or study".into()),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

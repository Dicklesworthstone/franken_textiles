//! Emit one measured treatment request with ordinary frozen selections and genuine content locks.
//! Example: cargo run -p ftx-pattern --example laundered_denim_treatment -- 1

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let code = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1".into())
        .parse::<u8>()?;
    let input = ftx_pattern::material_catalog::laundered_denim::treatment_request(code)?;
    println!("{}", serde_json::to_string_pretty(&input)?);
    Ok(())
}

//! Exact physical-stock pricing through the native authority; no I/O, clock or checkout.
use ftx_core::{Refusal, RefusalCode};
use ftx_export::cutting::{procurement::finite, stock};
use ftx_pattern::Pattern;

const MAX: usize = 64 * 1024 * 1024;
fn invalid(e: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.to_string())
}
fn budget(inputs: &[&str]) -> Result<(), Refusal> {
    if inputs
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .is_none_or(|n| n > MAX)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined stock-pricing inputs exceed 64 MiB",
        ));
    }
    Ok(())
}
fn plan(pattern: &str, stock: &str) -> Result<stock::Plan, Refusal> {
    let pattern = Pattern::from_json(pattern).map_err(invalid)?;
    let stock: stock::Request = serde_json::from_str(stock).map_err(invalid)?;
    stock.build(&pattern)
}
fn output(value: &impl serde::Serialize) -> Result<String, Refusal> {
    struct Limited(Vec<u8>);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX - self.0.len() {
                return Err(std::io::Error::other("stock-pricing result exceeds 64 MiB"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Limited(Vec::new());
    serde_json::to_writer(&mut sink, value).map_err(|e| {
        Refusal::new(
            if e.is_io() {
                RefusalCode::BudgetExceeded
            } else {
                RefusalCode::InvalidDocument
            },
            e.to_string(),
        )
    })?;
    String::from_utf8(sink.0).map_err(invalid)
}

/// Frozen physical-stock identities for reviewed offer binding, after a complete allocation.
///
/// # Errors
/// Combined input/output budgets, malformed input or a native stock planning refusal.
pub fn stock_identities(pattern: &str, stock: &str) -> Result<String, Refusal> {
    budget(&[pattern, stock])?;
    output(&finite::identities(&plan(pattern, stock)?)?)
}

/// Dated whole-item prices for the exact items selected by the production stock planner.
/// Unknown required prices remain unknown; no zero-price fallback, reservation or payment occurs.
///
/// # Errors
/// Combined input/output budgets, malformed data, planning or quote/binding-contract refusal.
pub fn stock_price(pattern: &str, stock: &str, pricing: &str) -> Result<String, Refusal> {
    budget(&[pattern, stock, pricing])?;
    let pricing = serde_json::from_str(pricing).map_err(invalid)?;
    output(&finite::evaluate(&plan(pattern, stock)?, &pricing)?)
}

/// Browser export of [`stock_identities`].
///
/// # Errors
/// Same refusal and bounded-output contract as [`stock_identities`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = stock_identities)]
pub fn web_stock_identities(pattern: &str, stock: &str) -> Result<String, wasm_bindgen::JsError> {
    stock_identities(pattern, stock).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser export of [`stock_price`].
///
/// # Errors
/// Same refusal and bounded-output contract as [`stock_price`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = stock_price)]
pub fn web_stock_price(
    pattern: &str,
    stock: &str,
    pricing: &str,
) -> Result<String, wasm_bindgen::JsError> {
    stock_price(pattern, stock, pricing).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

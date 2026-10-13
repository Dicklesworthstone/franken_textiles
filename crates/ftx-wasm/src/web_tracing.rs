//! Byte/JSON-only browser boundary for the local trace workshop.

use wasm_bindgen::prelude::*;

/// One immutable traced PNG, kept inside the worker until replaced or cancelled.
#[wasm_bindgen]
pub struct WebTrace {
    inner: crate::tracing::Sheet,
}

#[wasm_bindgen]
impl WebTrace {
    /// Decode and trace the supplied PNG with explicit JSON options.
    #[wasm_bindgen(constructor)]
    pub fn new(png: &[u8], options: &str) -> Result<WebTrace, JsError> {
        crate::tracing::Sheet::new(png, options)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Review names, millimetre scale targets and label evidence against the original draft.
    pub fn review(
        &self,
        names: &str,
        targets: &str,
        labels: &str,
    ) -> Result<WebTraceReview, JsError> {
        self.inner
            .review(names, targets, labels)
            .map(|inner| WebTraceReview { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }
}

/// Fully computed review files. Free after copying needed bytes into transferable buffers.
#[wasm_bindgen]
pub struct WebTraceReview {
    inner: crate::tracing::Review,
}

#[wasm_bindgen]
impl WebTraceReview {
    /// JSON report with separate scale and audit status; never a fit verdict.
    #[must_use]
    pub fn report(&self) -> String {
        self.inner.report.to_string()
    }

    /// Fixed relative file names, including the completion report.
    #[must_use]
    pub fn names(&self) -> String {
        serde_json::to_string(
            &self
                .inner
                .files
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
        )
        .expect("string array serialization cannot fail")
    }

    /// Bytes of a named review product, never a filesystem read.
    #[must_use]
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        self.inner
            .files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
    }

    /// All products as a deterministic ZIP archive.
    #[must_use]
    pub fn zip(&self) -> Vec<u8> {
        self.inner.zip()
    }
}

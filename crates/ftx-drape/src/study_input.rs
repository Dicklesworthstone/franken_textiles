//! One source-preparation boundary for native and browser material scenario studies.
//! Joint outfits reuse the production composer; candidates still use the ordinary fresh-state study.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use crate::{outfit, run::DrapeOptions};

/// A source pattern or frozen joint outfit, prepared once before evaluating material alternatives.
/// It retains no mesh, solver, material history, body or thermal state between candidates.
pub struct PreparedInput {
    pattern: Pattern,
    outfit: Option<Value>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl PreparedInput {
    /// Admit the existing fabric-pattern/1 path or compose an ftx-outfit-drape/1 source.
    /// All composed pieces get frozen explicit selections, so substitutions must name source-piece
    /// IDs such as `jacket::front`, not rely on replacing a default underneath explicit assignments.
    ///
    /// # Errors
    /// Any ordinary outfit composition refusal, including disabled self-contact, incomplete initial
    /// material/interface evidence, unresolvable attachment/casing names or oversized input.
    /// Ordinary patterns retain the existing study's per-consumer validation behavior.
    pub fn new(source: Value, options: &DrapeOptions) -> Result<Self, Refusal> {
        if source["schema"].as_str() != Some(outfit::SCHEMA) {
            return Ok(Self {
                pattern: Pattern::new(source),
                outfit: None,
            });
        }
        let input: outfit::Input = serde_json::from_value(source.clone())
            .map_err(|e| invalid(format!("joint study source: {e}")))?;
        let prepared = outfit::prepare(&input, options)?;
        // Retain exact frozen JSON for replay, not another catalog-dependent source selection.
        let pattern_json =
            serde_json::to_string(prepared.pattern().raw()).map_err(|e| invalid(e.to_string()))?;
        let mut receipt = prepared.receipt().clone();
        receipt["source_request"] = source;
        receipt["pattern_json"] = json!(pattern_json);
        receipt["basis"] = json!(
            "Joint contact composition; each material alternative starts a fresh combined drape. Layer heat uses only the explicitly requested coverage and boundary conditions."
        );
        receipt["fit_claim"] = json!(false);
        Ok(Self {
            pattern: Pattern::new(prepared.pattern().raw().clone()),
            outfit: Some(receipt),
        })
    }

    /// The actual frozen source passed to the existing binding, manufacturing and physical consumers.
    #[must_use]
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// Attach original garment correspondence and frozen replay data to the shared scenario record.
    /// An ordinary pattern leaves its descriptor unchanged, preserving existing output identities.
    ///
    /// # Errors
    /// A joint descriptor is not an object or would replace an existing outfit provenance record.
    pub fn annotate(&self, descriptor: &mut Value) -> Result<(), Refusal> {
        if let Some(outfit) = &self.outfit {
            let object = descriptor
                .as_object_mut()
                .ok_or_else(|| invalid("joint study descriptor must be an object"))?;
            if object.contains_key("outfit") {
                return Err(invalid("joint study cannot overwrite outfit provenance"));
            }
            object.insert("outfit".into(), outfit.clone());
        }
        Ok(())
    }
}

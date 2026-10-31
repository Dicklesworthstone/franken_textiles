//! Direct pattern alterations for the ordinary browser round, using the native operations and
//! lossless CLI serializer. A refused candidate never replaces its source.

use ftx_core::{Refusal, RefusalCode};
use ftx_export::PyJson;
use ftx_pattern::{Pattern, alter};
use serde::Deserialize;
use serde_json::{Value, json};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn document(text: &str) -> Result<(PyJson, Value, Pattern), Refusal> {
    if text.len() > 4 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "alteration patterns are limited to 4 MiB each",
        ));
    }
    let ordered = PyJson::parse(text).map_err(invalid)?;
    let value = ordered.to_value();
    let pieces = value["pieces"]
        .as_array()
        .ok_or_else(|| invalid("pattern pieces must be an array"))?;
    let edges: usize = pieces
        .iter()
        .map(|p| p["edges"].as_array().map_or(0, Vec::len))
        .sum();
    if pieces.len() > 128 || edges > 8192 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "direct alterations support at most 128 pieces and 8192 drafted edges",
        ));
    }
    let pattern = Pattern::new(value.clone());
    checked(&pattern)?;
    Ok((ordered, value, pattern))
}

fn checked(pattern: &Pattern) -> Result<(), Refusal> {
    let errors = pattern.structure_errors();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(invalid(format!(
            "alteration pattern has structure errors: {}",
            errors.join("; ")
        )))
    }
}

fn catalogue(value: &Value, pattern: &Pattern) -> Value {
    let pieces: Vec<_> = value["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            json!({"id":p["id"], "vertices":p["vertices"].as_object().map(|v|v.keys().collect::<Vec<_>>()),
                "edges":p["edges"].as_array().map(|e|e.iter().map(|v|&v["id"]).collect::<Vec<_>>())})
        })
        .collect();
    let instances: Vec<_> = pattern
        .instances
        .iter()
        .map(|i| json!({"id":i.id,"piece":i.piece,"edges":i.edges.iter().map(|e|&e.id).collect::<Vec<_>>()}))
        .collect();
    let seams: Vec<_> = pattern
        .seams()
        .iter()
        .filter_map(|s| {
            s.id()
                .map(|id| json!({"id":id,"a":s.side_ref("a"),"b":s.side_ref("b")}))
        })
        .collect();
    json!({"pieces":pieces,"instances":instances,"seams":seams})
}

/// Actual drafted pieces, vertices, seams and expanded instance edges for guided controls.
/// Geometry and source numbers are never round-tripped through the JavaScript display catalogue.
///
/// # Errors
/// Oversized, malformed or structurally invalid patterns.
pub fn targets(input: &str) -> Result<String, Refusal> {
    let (_, value, pattern) = document(input)?;
    Ok(catalogue(&value, &pattern).to_string())
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
enum Operation {
    SlashSpread {
        piece: String,
        axis: String,
        at: f64,
        add: f64,
    },
    Move {
        piece: String,
        vertex: String,
        dx: f64,
        dy: f64,
    },
    TrueSeam {
        seam: String,
        end: String,
    },
    CapEase {
        piece: String,
        seams: Vec<String>,
        target: f64,
    },
    TransferCurve {
        from: String,
        to: String,
    },
}

/// Apply one native operation and return exact `pattern_json`, its history sentence and targets.
/// The source and result must pass the ordinary structure checks; seam/fit/readiness checks still
/// belong to the subsequent ordinary round. No physical fit or manufacturing acceptance is inferred.
///
/// # Errors
/// Invalid input or operation, unbracketed native solves, structure errors or exceeded text budgets.
pub fn apply(input: &str, operation: &str, source: Option<&str>) -> Result<String, Refusal> {
    if operation.len() > 16 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "alteration request exceeds 16 KiB",
        ));
    }
    let (ordered, value, _) = document(input)?;
    let operation: Operation =
        serde_json::from_str(operation).map_err(|e| invalid(e.to_string()))?;
    if source.is_some() && !matches!(operation, Operation::TransferCurve { .. }) {
        return Err(invalid("a source pattern is only used by transfer-curve"));
    }
    let (result, op, piece, item) = match &operation {
        Operation::SlashSpread {
            piece,
            axis,
            at,
            add,
        } => (
            alter::slash_spread(&value, piece, axis, *at, *add)?,
            "slash-spread",
            Some(piece.as_str()),
            None,
        ),
        Operation::Move {
            piece,
            vertex,
            dx,
            dy,
        } => (
            alter::move_vertex(&value, piece, vertex, *dx, *dy)?,
            "move",
            Some(piece.as_str()),
            Some(vertex.as_str()),
        ),
        Operation::TrueSeam { seam, end } => (
            alter::true_seam(&value, seam, end)?,
            "true-seam",
            None,
            None,
        ),
        Operation::CapEase {
            piece,
            seams,
            target,
        } => (
            alter::cap_ease(&value, piece, seams, *target)?,
            "cap-ease",
            Some(piece.as_str()),
            None,
        ),
        Operation::TransferCurve { from, to } => {
            let (_, source, _) =
                document(source.ok_or_else(|| invalid("transfer-curve needs a source pattern"))?)?;
            let result = alter::transfer_curve(&value, &source, from, to)?;
            let (piece, edge) = to
                .split_once('/')
                .ok_or_else(|| invalid("target must be PIECE/EDGE"))?;
            (result, "transfer-curve", Some(piece), Some(edge))
        }
    };
    let pattern = Pattern::new(result.clone());
    checked(&pattern)?;
    let pattern_json =
        ftx_trace::alter_py(&ordered, &result, op, piece, item).dumps(Some(1)) + "\n";
    if pattern_json.len() > 4 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "altered pattern exceeds 4 MiB",
        ));
    }
    let why = result["meta"]["history"]
        .as_array()
        .and_then(|h| h.last())
        .and_then(|h| h["why"].as_str())
        .unwrap_or_default();
    Ok(
        json!({"pattern_json":pattern_json,"why":why,"targets":catalogue(&result,&pattern)})
            .to_string(),
    )
}

//! Explicit interface observations bind two exact material snapshots, never display names.
//! A symmetric scalar Coulomb coefficient is the currently supported interface law; no mixing rule.

use ftx_core::Refusal;
use ftx_pattern::material_friction::Interface;
use serde_json::Value;

use super::invalid;

pub(super) struct Interfaces {
    pub matrix: Option<Vec<Vec<f64>>>,
    pub report: Value,
}

/// Canonical hashes and same-sheet diagonal coefficients are already resolved from the records.
/// Validate every supplied pair even when self-contact is off, but require coverage only when on.
pub(super) fn resolve(
    raw: Option<&Value>,
    hashes: &[String],
    diagonal: &[f64],
    self_contact: bool,
) -> Result<Interfaces, Refusal> {
    let count = hashes.len();
    if count == 0 || count > 64 || diagonal.len() != count {
        return Err(invalid("contact supports 1..64 distinct source snapshots"));
    }
    let max_pairs = count * (count - 1) / 2;
    let mut pairs: Vec<Interface> = match raw {
        None => Vec::new(),
        Some(raw) => {
            let values = raw
                .as_array()
                .ok_or_else(|| invalid("fabric.interfaces must be a list"))?;
            if values.len() > max_pairs {
                return Err(invalid("too many or duplicate interface pairs"));
            }
            values
                .iter()
                .map(Interface::parse)
                .collect::<Result<_, _>>()?
        }
    };
    for pair in &mut pairs {
        pair.validate()?;
        pair.materials.sort();
    }
    pairs.sort_by(|a, b| a.materials.cmp(&b.materials));
    let mut matrix = vec![vec![None; count]; count];
    for (i, &mu) in diagonal.iter().enumerate() {
        matrix[i][i] = Some(mu);
    }
    for pair in &pairs {
        let index = |h: &String| {
            hashes.binary_search(h).map_err(|_| {
                invalid(format!(
                    "interface names unassigned or stale material snapshot {h}"
                ))
            })
        };
        let (a, b) = (index(&pair.materials[0])?, index(&pair.materials[1])?);
        if a == b || matrix[a][b].is_some() {
            return Err(invalid(
                "duplicate or same-sheet interface; diagonals belong to the sheet record",
            ));
        }
        matrix[a][b] = Some(pair.friction.value);
        matrix[b][a] = Some(pair.friction.value);
    }
    let matrix = if self_contact {
        Some(matrix.into_iter().map(|row| row.into_iter().map(|v| {
            v.ok_or_else(|| invalid("unlike sheets require complete pairwise friction evidence; no mixing rule is assumed"))
        }).collect()).collect::<Result<Vec<Vec<f64>>, Refusal>>()?)
    } else {
        None // no latent authorization to enable self-contact with an incomplete table
    };
    Ok(Interfaces {
        matrix,
        report: Value::Array(pairs.iter().map(Interface::report).collect()),
    })
}

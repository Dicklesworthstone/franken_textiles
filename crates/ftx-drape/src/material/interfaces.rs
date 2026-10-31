//! Explicit interface observations bind two exact material snapshots, never display names.
//! A symmetric scalar Coulomb coefficient is the currently supported interface law; no mixing rule.

use ftx_core::Refusal;
use ftx_pattern::material_friction::Interface;
use serde_json::Value;
use std::collections::BTreeSet;

use super::invalid;

pub(crate) struct Interfaces {
    pub matrix: Option<Vec<Vec<f64>>>,
    pub report: Value,
}

/// Only active sheets supply contact coefficients. All declared source pieces still authorize
/// and validate their supplied pair observations, including pieces explicitly excluded by sim:false.
pub(crate) fn resolve(
    raw: Option<&Value>,
    hashes: &[String],
    diagonal: &[Option<f64>],
    declared_hashes: &[String],
    self_contact: bool,
) -> Result<Interfaces, Refusal> {
    let count = hashes.len();
    if (count == 0 && self_contact)
        || count > 64
        || diagonal.len() != count
        || declared_hashes.len() > 64
        || hashes.windows(2).any(|pair| pair[0] >= pair[1])
        || declared_hashes.windows(2).any(|pair| pair[0] >= pair[1])
        || hashes
            .iter()
            .any(|hash| declared_hashes.binary_search(hash).is_err())
    {
        return Err(invalid("contact supports 1..64 distinct source snapshots"));
    }
    let max_pairs = declared_hashes.len() * declared_hashes.len().saturating_sub(1) / 2;
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
        matrix[i][i] = mu;
    }
    let mut seen = BTreeSet::new();
    for pair in &pairs {
        for hash in &pair.materials {
            if declared_hashes.binary_search(hash).is_err() {
                return Err(invalid(format!(
                    "interface names unassigned or stale material snapshot {hash}"
                )));
            }
        }
        if pair.materials[0] == pair.materials[1] || !seen.insert(&pair.materials) {
            return Err(invalid(
                "duplicate or same-sheet interface; diagonals belong to the sheet record",
            ));
        }
        if let (Ok(a), Ok(b)) = (
            hashes.binary_search(&pair.materials[0]),
            hashes.binary_search(&pair.materials[1]),
        ) {
            matrix[a][b] = Some(pair.friction.value);
            matrix[b][a] = Some(pair.friction.value);
        }
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

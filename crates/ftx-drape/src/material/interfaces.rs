//! Explicit interface observations bind two exact material snapshots, never display names.
//! A symmetric scalar Coulomb coefficient is the currently supported interface law; no mixing rule.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::materials::{Evidence, Quantity, Source};
use serde_json::{Value, json};

use super::invalid;

/// An authored unlike-sheet interface. Reuses the canonical observation/source vocabulary.
#[derive(Clone, Debug)]
struct Interface {
    /// Sorted for canonical assembly; the supplied order is physically irrelevant.
    materials: [String; 2],
    friction: Quantity,
    sources: BTreeMap<String, Source>,
    allow_estimates: bool,
}

fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}

impl Interface {
    fn parse(raw: &Value) -> Result<Self, Refusal> {
        let object = raw
            .as_object()
            .ok_or_else(|| invalid("interface must be an object"))?;
        if object
            .keys()
            .any(|k| !["materials", "friction", "sources", "allow_estimates"].contains(&k.as_str()))
        {
            return Err(invalid("unknown interface field"));
        }
        let materials: [String; 2] = serde_json::from_value(raw["materials"].clone())
            .map_err(|e| invalid(format!("interface needs two material snapshot hashes: {e}")))?;
        let friction = serde_json::from_value(raw["friction"].clone())
            .map_err(|e| invalid(format!("interface friction observation: {e}")))?;
        let sources = serde_json::from_value(raw["sources"].clone())
            .map_err(|e| invalid(format!("interface source records: {e}")))?;
        let allow_estimates = match object.get("allow_estimates") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid("allow_estimates must be boolean"))?,
        };
        Ok(Self {
            materials,
            friction,
            sources,
            allow_estimates,
        })
    }

    fn report(&self) -> Value {
        json!({"materials":self.materials, "friction":self.friction,
            "sources":self.sources, "allow_estimates":self.allow_estimates})
    }

    fn validate(&self) -> Result<(), Refusal> {
        let q = &self.friction;
        if q.unit != "1"
            || !q.value.is_finite()
            || q.value < 0.0
            || !text(&q.method)
            || !self.sources.contains_key(&q.source)
            || self.sources.is_empty()
            || self.sources.len() > 64
            || self.sources.iter().any(|(k, v)| {
                !text(k) || !text(&v.reference) || !text(&v.accessed_on) || !text(&v.note)
            })
            || q.conditions.len() > 64
            || q.conditions.iter().any(|(k, v)| !text(k) || !text(v))
            || q.bounds.is_some_and(|[lo, hi]| {
                !lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo > q.value || hi < q.value
            })
        {
            return Err(invalid(
                "interface friction needs finite nonnegative dimensionless observations, valid bounds and complete source/method metadata",
            ));
        }
        if q.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(invalid(
                "interface friction estimate needs its own explicit consent",
            ));
        }
        Ok(())
    }
}

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

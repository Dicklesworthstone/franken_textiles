//! Exhaustive material choices on disjoint authored targets, evaluated by the ordinary study.
//! Selection never estimates a pair law or prunes candidates with a scalar-property surrogate.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_pattern::{
    Pattern, material_binding::Changes, material_friction::Interface, material_screen::Candidate,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PreparedVariant, Target, Variant, budget, invalid};

/// Independent material alternatives for one authored placement. Choice keys identify lots/overrides,
/// so different choices may intentionally share a material ID.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Unique group identity, retained on every generated alternative.
    pub key: String,
    /// Existing default or exact source-piece assignment semantics; groups cannot share pieces.
    pub target: Target,
    /// One to 32 individually identified source-bearing selections, without implicit pruning.
    pub choices: Vec<Candidate>,
}

/// A finite design space over the same original pattern and shared process/scenario policies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// One to eight independent groups, with a complete Cartesian product of at most 32 rows.
    pub groups: Vec<Group>,
    /// Optional complete pool of existing exact lowercase-hash pair observations. Each combination
    /// replaces the interface table with exactly the pool entries whose two materials are assigned.
    /// Omitted preserves authored evidence; an empty pool explicitly clears it. No pair is inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interfaces: Option<Vec<Value>>,
}

fn key_valid(key: &str) -> bool {
    !key.trim().is_empty() && key.len() <= 256
}

impl Request {
    pub(super) fn variants(&self, pattern: &Pattern) -> Result<Vec<PreparedVariant>, Refusal> {
        if self.groups.is_empty() || self.groups.len() > 8 {
            return Err(invalid(
                "material combinations need one to eight choice groups",
            ));
        }
        let mut count = 1usize;
        let mut group_keys = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut has_default = false;
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("material combinations need source pieces"))?;
        for group in &self.groups {
            if !key_valid(&group.key) || !group_keys.insert(&group.key) {
                return Err(invalid(
                    "combination group keys must be unique, nonempty and at most 256 bytes",
                ));
            }
            if group.choices.is_empty() || group.choices.len() > 32 {
                return Err(invalid(
                    "each material choice group needs one to 32 choices",
                ));
            }
            count = count
                .checked_mul(group.choices.len())
                .ok_or_else(|| budget("material combination count overflow"))?;
            if count > 32 {
                return Err(budget(
                    "material combinations exceed 32 alternatives; no choices were evaluated",
                ));
            }
            let mut choice_keys = BTreeSet::new();
            if group
                .choices
                .iter()
                .any(|choice| !key_valid(&choice.key) || !choice_keys.insert(&choice.key))
            {
                return Err(invalid(
                    "choice keys within a group must be unique, nonempty and at most 256 bytes",
                ));
            }
            match &group.target {
                Target::Default => {
                    if has_default {
                        return Err(invalid(
                            "only one material choice group may replace the default",
                        ));
                    }
                    has_default = true;
                }
                Target::Pieces { ids } => {
                    if ids.is_empty()
                        || ids.len() > 4096
                        || ids.iter().any(|id| {
                            !targets.insert(id) || !pieces.iter().any(|piece| piece["id"] == *id)
                        })
                    {
                        return Err(invalid(
                            "material choice targets need disjoint, unique existing source-piece IDs",
                        ));
                    }
                }
            }
        }
        if let Some(pool) = &self.interfaces {
            if pool.len() > 2016 {
                return Err(budget(
                    "material combination interface pool exceeds 2016 pairs",
                ));
            }
            let mut pairs = BTreeSet::new();
            for raw in pool {
                let interface = Interface::parse(raw)?;
                interface.validate_observation()?;
                let mut pair = interface.materials;
                if pair.iter().any(|h| {
                    h.len() != 64 || !h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                }) {
                    return Err(invalid(
                        "material combination interfaces need exact lowercase SHA-256 snapshot identities",
                    ));
                }
                pair.sort();
                if pair[0] == pair[1] || !pairs.insert(pair) {
                    return Err(invalid(
                        "material combination interface pool has duplicate or same-material pairs",
                    ));
                }
            }
        }

        let mut groups: Vec<_> = self.groups.iter().collect();
        groups.sort_by(|a, b| a.key.cmp(&b.key));
        let mut combinations = vec![PreparedVariant {
            variant: Variant {
                key: String::new(),
                changes: Changes {
                    schema: "ftx-material-bindings/1".into(),
                    ..Changes::default()
                },
            },
            choices: BTreeMap::new(),
        }];
        for group in groups {
            let mut choices: Vec<_> = group.choices.iter().collect();
            choices.sort_by(|a, b| a.key.cmp(&b.key));
            let mut expanded = Vec::with_capacity(combinations.len() * choices.len());
            for previous in combinations {
                for choice in &choices {
                    let mut changes = previous.variant.changes.clone();
                    match &group.target {
                        Target::Default => changes.default = Some(choice.selection.clone()),
                        Target::Pieces { ids } => {
                            for id in ids {
                                changes.pieces.insert(id.clone(), choice.selection.clone());
                            }
                        }
                    }
                    let mut chosen = previous.choices.clone();
                    chosen.insert(group.key.clone(), choice.key.clone());
                    // JSON string escaping and a sorted map make arbitrary key text unambiguous.
                    let key = serde_json::to_string(&chosen).map_err(|e| invalid(e.to_string()))?;
                    expanded.push(PreparedVariant {
                        variant: Variant { key, changes },
                        choices: chosen,
                    });
                }
            }
            combinations = expanded;
        }
        debug_assert_eq!(combinations.len(), count);
        Ok(combinations)
    }

    pub(super) fn interfaces_for(&self, bound: &Value) -> Result<Option<Vec<Value>>, Refusal> {
        let Some(pool) = &self.interfaces else {
            return Ok(None);
        };
        let assigned: BTreeSet<_> = bound["pieces"]
            .as_array()
            .ok_or_else(|| {
                invalid("bound combination omitted its effective source-piece assignments")
            })?
            .iter()
            .map(|piece| {
                piece["material_sha256"].as_str().ok_or_else(|| {
                    invalid("bound combination omitted a material snapshot identity")
                })
            })
            .collect::<Result<_, _>>()?;
        let mut selected = Vec::new();
        for raw in pool {
            let interface = Interface::parse(raw)?;
            if interface
                .materials
                .iter()
                .all(|hash| assigned.contains(hash.as_str()))
            {
                selected.push(raw.clone());
            }
        }
        Ok(Some(selected))
    }
}

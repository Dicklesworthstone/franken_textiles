//! Shared retained-extra mass evidence and explicit point attachments on physical pattern instances.
//! Extras are absent from the cut sheets. Their total group mass is never multiplied by target count.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::{
    Pattern,
    materials::{Evidence, Quantity, Source},
};

/// One retained group absent from the physical cut pieces, shared by accounting and dynamics.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    /// Unique group identity for ONE assembly, not one copy at every attachment target.
    pub id: String,
    /// Total group mass in kg. Null remains unknown for accounting and cannot drive dynamics.
    pub mass: Option<Quantity>,
    /// Existing source vocabulary, scoped to this group.
    #[serde(default)]
    pub sources: BTreeMap<String, Source>,
    /// Consent applies only to this group's estimated mass.
    #[serde(default)]
    pub allow_estimates: bool,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl Extra {
    /// The same source admission used by both complete assembly accounting and attached inertia.
    ///
    /// # Errors
    /// Unknown, nonpositive/nonfinite or wrong-unit mass, invalid bounds, missing source/method,
    /// or estimated mass without this group's explicit consent.
    pub fn evaluate(&self) -> Result<(f64, Option<[f64; 2]>), Refusal> {
        let q = self.mass.as_ref().ok_or_else(|| {
            invalid(format!(
                "{}: mass of retained extra group is unknown",
                self.id
            ))
        })?;
        let source = self
            .sources
            .get(&q.source)
            .ok_or_else(|| invalid("extra mass needs its retained source"))?;
        if q.unit != "kg"
            || q.method.trim().is_empty()
            || q.source.trim().is_empty()
            || source.reference.trim().is_empty()
            || source.accessed_on.trim().is_empty()
            || source.note.trim().is_empty()
        {
            return Err(invalid(
                "extra mass needs kg, method and complete source identity",
            ));
        }
        if q.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(invalid(
                "extra mass is estimated; explicit group estimate consent is required",
            ));
        }
        if !q.value.is_finite() || q.value <= 0.0 {
            return Err(invalid(
                "mass and retained area must be finite and positive",
            ));
        }
        if let Some([lo, hi]) = q.bounds
            && (!lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo > q.value || hi < q.value)
        {
            return Err(invalid("mass observation has invalid reported bounds"));
        }
        Ok((q.value, q.bounds))
    }

    /// Compare the complete typed observation, including policy, bounds, conditions and sources.
    ///
    /// # Errors
    /// An observation cannot be represented as canonical JSON.
    pub fn same_observation(&self, other: &Self) -> Result<bool, Refusal> {
        Ok(
            serde_json::to_vec(self).map_err(|e| invalid(e.to_string()))?
                == serde_json::to_vec(other).map_err(|e| invalid(e.to_string()))?,
        )
    }
}

/// An explicit share of one group's total mass, attached to an existing named boundary vertex.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Full physical instance name; mirrored pieces must be named separately.
    pub instance: String,
    /// Vertex name in that physical instance (including unfolded-side suffixes where present).
    pub vertex: String,
    /// Fraction of TOTAL group mass, in (0,1]. All targets in the group must sum to one.
    pub fraction: f64,
}

/// One inventory group and all its explicit mass allocations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Source-bearing retained group, shared without duplication by mass accounting.
    pub extra: Extra,
    /// Named physical attachment points; no automatic mirror, quantity or nearest-point inference.
    pub targets: Vec<Target>,
}

/// Optional `pattern.attached_mass` extension, consumed by drape and complete assembly accounting.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachments {
    /// Must be `ftx-attached-mass/1`.
    pub schema: String,
    /// Each ID occurs exactly once. Empty preserves the ordinary sheet-only path.
    pub groups: Vec<Group>,
}

impl Attachments {
    /// Read and validate attachment topology without promoting unknown mass to a usable number.
    ///
    /// # Errors
    /// Invalid schema/budgets, repeated group/target, absent instance/boundary vertex, or fractions
    /// which do not allocate one complete group. Mass evidence is independently admitted by Extra.
    pub fn from_pattern(pattern: &Pattern) -> Result<Option<Self>, Refusal> {
        let Some(raw) = pattern.raw().get("attached_mass") else {
            return Ok(None);
        };
        let value: Self = serde_json::from_value(raw.clone())
            .map_err(|e| invalid(format!("attached mass: {e}")))?;
        value.validate(pattern)?;
        Ok(Some(value))
    }

    /// Check explicit targets against the expanded pattern, before a simulation mesh is needed.
    ///
    /// # Errors
    /// Same structural failures as [`Self::from_pattern`].
    pub fn validate(&self, pattern: &Pattern) -> Result<(), Refusal> {
        if self.schema != "ftx-attached-mass/1" || self.groups.len() > 1024 {
            return Err(invalid(
                "attached mass needs schema /1 and at most 1024 groups",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut target_count = 0usize;
        for group in &self.groups {
            if group.extra.id.trim().is_empty()
                || !ids.insert(&group.extra.id)
                || group.targets.is_empty()
                || group.targets.len() > 4096
            {
                return Err(invalid(
                    "attached mass needs unique nonempty group IDs and 1..4096 explicit targets per group",
                ));
            }
            target_count = target_count
                .checked_add(group.targets.len())
                .filter(|n| *n <= 65_536)
                .ok_or_else(|| invalid("attached mass target budget exceeded"))?;
            let mut targets = BTreeSet::new();
            let mut sum = 0.0;
            for target in &group.targets {
                if !target.fraction.is_finite()
                    || target.fraction <= 0.0
                    || target.fraction > 1.0
                    || !targets.insert((&target.instance, &target.vertex))
                {
                    return Err(invalid(
                        "attached mass needs unique targets with fractions in (0,1]",
                    ));
                }
                let instance = pattern.instance(&target.instance).ok_or_else(|| {
                    invalid(format!(
                        "attached mass names unknown instance {}",
                        target.instance
                    ))
                })?;
                if !instance.edges.iter().any(|e| e.from == target.vertex)
                    || instance.verts.get(&target.vertex).is_none()
                {
                    return Err(invalid(format!(
                        "attached mass needs an existing boundary vertex {}/{}",
                        target.instance, target.vertex
                    )));
                }
                sum += target.fraction;
            }
            // This admits only ordinary summation roundoff. The source fractions are retained;
            // the physical allocator reports its arithmetic residual rather than renormalizing.
            if !sum.is_finite()
                || (sum - 1.0).abs() > 16.0 * f64::EPSILON * group.targets.len() as f64
            {
                return Err(invalid(
                    "attached mass fractions must sum to one complete group",
                ));
            }
        }
        Ok(())
    }
}

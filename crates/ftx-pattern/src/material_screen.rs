//! Evidence-qualified material screening and interval-safe Pareto alternatives.
//!
//! This does not fit a material law, invent missing values or predict garment performance. Every
//! admitted alternative is a portable selection for the existing binding/physics consumers. Bounds
//! are reported intervals, not confidence intervals; nominal comparisons require explicit consent.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::{
    material_catalog::{ResolvedMaterial, Selection, property_unit},
    material_thermal::{ConductionLaw, ThermalLaw},
    materials::{Catalog, Form, Quantity},
};

/// A named scalar, in canonical units, with explicitly requested applicability qualifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    /// Existing property key; extension keys require an exact unit match.
    pub key: String,
    /// Canonical unit from `material_catalog`, or the exact unit of an extension property.
    pub unit: String,
    /// Exact required subset of observation conditions. Empty declares no condition restriction.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Optional exact test/aggregation method restriction; no method equivalence is inferred.
    #[serde(default)]
    pub method: Option<String>,
}

/// An inclusive scalar feasibility interval. At least one endpoint is required.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    /// Quantity to inspect, without deriving it from a fiber name or a different specimen.
    pub attribute: Attribute,
    /// Inclusive lower limit in the attribute's unit.
    pub minimum: Option<f64>,
    /// Inclusive upper limit in the attribute's unit.
    pub maximum: Option<f64>,
}

/// Whether smaller or larger observations are preferred.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Prefer smaller observations.
    Minimize,
    /// Prefer larger observations.
    Maximize,
}

/// One independent objective; there is no hidden weighting between objectives.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    /// Source-bearing scalar to compare.
    pub attribute: Attribute,
    /// Preferred direction.
    pub direction: Direction,
}

/// Explicit choice of nominal comparison or conservative use of reported bounds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Use reported nominal values; this is not an uncertainty-aware comparison.
    Nominal,
    /// Require reported bounds for every constraint/objective and compare entire intervals.
    ReportedBounds,
}

/// A compiler-readiness requirement, not physical validation or a successful garment simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Complete sheet mechanics, including optional bending, plasticity and relaxation laws.
    Mechanics,
    /// Compile the existing steady through-thickness conduction law without demanding heat capacity.
    SteadyConduction,
    /// Compile the existing transient thermal law, including reference areal mass and heat capacity.
    TransientThermal,
}

/// One explicitly identified candidate; different lots/overrides may share a material ID.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// Unique study key, retained through downstream pattern/material substitution.
    pub key: String,
    /// Existing canonical selection, optionally source-frozen or explicitly overridden.
    pub selection: Selection,
}

/// A bounded screening request against the existing material authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal `ftx-material-screen/1`.
    pub schema: String,
    /// Required: nominal values and interval-safe comparisons are deliberately not interchangeable.
    pub basis: Basis,
    /// Empty selects the provided catalog; otherwise only these exact selections are considered.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// Estimate policy for catalog-wide discovery only. Explicit candidates carry their own policy.
    #[serde(default)]
    pub allow_catalog_estimates: bool,
    /// Explicitly allow dominance across different reported methods/conditions. False keeps such
    /// observations incomparable, even if both pass the requested condition subset.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
    /// Optional case-insensitive identity/construction/composition discovery filter.
    #[serde(default)]
    pub query: String,
    /// Allowed forms; empty imposes no form restriction.
    #[serde(default)]
    pub forms: Vec<Form>,
    /// Inclusive feasibility intervals.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Independent objectives. With none, all eligible candidates are non-dominated.
    #[serde(default)]
    pub objectives: Vec<Objective>,
    /// Only requested consumers must compile; unavailable unrelated physics remains irrelevant.
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
}

/// One inspected alternative with its complete source-locked identity and explicit exclusions.
#[derive(Clone, Debug, Serialize)]
pub struct Alternative {
    /// Stable candidate key.
    pub key: String,
    /// Authored material identifier, including for a refused selection.
    pub material_id: String,
    /// Present only after successful resolution; directly usable by `material_binding::Changes`.
    pub selection: Option<Selection>,
    /// Requested, admitted observations in canonical units, with original method/conditions/source.
    pub readings: BTreeMap<String, Quantity>,
    /// Every encountered exclusion; missing data is never assigned a score or treated as zero.
    pub exclusions: Vec<Refusal>,
    /// Whether all filters, constraints, objectives and requested compiler checks passed.
    pub eligible: bool,
    /// Eligible candidates provably dominating this candidate under the chosen comparison basis.
    pub dominated_by: Vec<String>,
}

/// Reproducible source-bearing screening result, not an empirical "best textile" rating.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Report format version.
    pub schema: &'static str,
    /// Digest of the complete authored request, not a claim about source authenticity.
    pub request_sha256: String,
    /// Explicit comparison policy copied from the request.
    pub basis: Basis,
    /// Whether cross-protocol comparisons were explicitly admitted by the request.
    pub allow_mixed_protocols: bool,
    /// All candidates, including unavailable/excluded ones, ordered by key.
    pub alternatives: Vec<Alternative>,
    /// Eligible candidates not dominated on the requested objectives, ordered by key.
    pub frontier: Vec<String>,
    /// Always false: source comparison and compiler readiness do not validate physical behavior.
    pub physical_validation_claim: bool,
    /// Applicability and uncertainty limitations visible to every consumer.
    pub notes: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl Request {
    fn attributes(&self) -> Result<BTreeMap<String, Attribute>, Refusal> {
        if self.schema != "ftx-material-screen/1" {
            return Err(invalid("unsupported material screening schema"));
        }
        if self.candidates.len() > 1024
            || self.constraints.len() > 64
            || self.objectives.len() > 16
            || self.required_capabilities.len() > 3
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "screening permits at most 1024 candidates, 64 constraints, 16 objectives and 3 capabilities",
            ));
        }
        if !self.candidates.is_empty() && self.allow_catalog_estimates {
            return Err(invalid(
                "explicit candidates carry their own estimate policy; allow_catalog_estimates applies only to catalog discovery",
            ));
        }
        for constraint in &self.constraints {
            if (constraint.minimum.is_none() && constraint.maximum.is_none())
                || constraint.minimum.is_some_and(|v| !v.is_finite())
                || constraint.maximum.is_some_and(|v| !v.is_finite())
                || matches!((constraint.minimum, constraint.maximum), (Some(lo), Some(hi)) if lo > hi)
            {
                return Err(invalid("constraint needs finite, ordered inclusive bounds"));
            }
        }
        let mut attributes = BTreeMap::new();
        for a in self
            .constraints
            .iter()
            .map(|c| &c.attribute)
            .chain(self.objectives.iter().map(|o| &o.attribute))
        {
            if a.key.trim().is_empty()
                || a.unit.trim().is_empty()
                || a.method.as_ref().is_some_and(|s| s.trim().is_empty())
                || a.conditions
                    .iter()
                    .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
                || property_unit(&a.key).is_some_and(|u| u != a.unit)
            {
                return Err(invalid(format!(
                    "{}: attribute needs canonical units and nonempty qualifiers",
                    a.key
                )));
            }
            if attributes
                .insert(a.key.clone(), a.clone())
                .is_some_and(|old| old != *a)
            {
                return Err(invalid(format!(
                    "{}: conflicting unit, method or condition requirements",
                    a.key
                )));
            }
        }
        Ok(attributes)
    }

    /// Resolve, qualify and compare alternatives without changing catalog records or pattern geometry.
    ///
    /// # Errors
    /// Malformed request/catalog, duplicate keys or excessive work. Candidate-specific absent data,
    /// failed source locks and unsupported compiler inputs are retained as exclusions in the report.
    pub fn evaluate(&self, catalog: &Catalog) -> Result<Report, Refusal> {
        let attributes = self.attributes()?;
        catalog.validate()?;
        if self.candidates.is_empty() && catalog.materials.len() > 1024 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "catalog screening exceeds 1024 candidates",
            ));
        }
        let candidates = if self.candidates.is_empty() {
            catalog
                .materials
                .iter()
                .map(|m| Candidate {
                    key: m.id.clone(),
                    selection: Selection {
                        id: m.id.clone(),
                        allow_estimates: self.allow_catalog_estimates,
                        ..Selection::default()
                    },
                })
                .collect()
        } else {
            self.candidates.clone()
        };
        let mut keys = BTreeSet::new();
        for c in &candidates {
            if c.key.trim().is_empty() || !keys.insert(c.key.as_str()) {
                return Err(invalid("candidate keys must be nonempty and unique"));
            }
        }
        let mut alternatives: Vec<_> = candidates
            .iter()
            .map(|c| self.inspect(c, catalog, &attributes))
            .collect();
        alternatives.sort_by(|a, b| a.key.cmp(&b.key));
        // Compute against immutable rows: neither input order nor frontier insertion order can alter ties.
        let dominators: Vec<Vec<String>> = alternatives
            .iter()
            .map(|b| {
                alternatives
                    .iter()
                    .filter(|a| a.key != b.key && self.dominates(a, b))
                    .map(|a| a.key.clone())
                    .collect()
            })
            .collect();
        for (row, dominated_by) in alternatives.iter_mut().zip(dominators) {
            row.dominated_by = dominated_by;
        }
        Ok(Report {
            schema: "ftx-material-screen-report/1",
            request_sha256: sha256_hex(&serde_json::to_vec(self).map_err(|e| invalid(e.to_string()))?),
            basis: self.basis,
            allow_mixed_protocols: self.allow_mixed_protocols,
            frontier: alternatives.iter().filter(|a| a.eligible && a.dominated_by.is_empty()).map(|a| a.key.clone()).collect(),
            alternatives,
            physical_validation_claim: false,
            notes: vec![
                "Only requested scalar observations and compiler capabilities are compared. No fit, warmth rating, safety certification, robot authorization or market-population optimum is inferred.".into(),
                "Method/condition filters are exact restrictions, not proof of cross-specimen comparability. Reported bounds are not confidence intervals; absent bounds never become zero-width intervals in reported_bounds mode.".into(),
                "Dominance requires matching reported methods and complete observation conditions unless allow_mixed_protocols was explicitly requested. An unknown protocol shared by two records is still not empirical comparability evidence.".into(),
                "Prices belong to dated, currency/region/quantity-qualified quote and marker evaluation, not a timeless fiber-property score. Temperature-dependent curves are not reduced to unstated scalar temperatures.".into(),
            ],
        })
    }

    fn inspect(
        &self,
        candidate: &Candidate,
        catalog: &Catalog,
        attributes: &BTreeMap<String, Attribute>,
    ) -> Alternative {
        let mut row = Alternative {
            key: candidate.key.clone(),
            material_id: candidate.selection.id.clone(),
            selection: None,
            readings: BTreeMap::new(),
            exclusions: Vec::new(),
            eligible: false,
            dominated_by: Vec::new(),
        };
        let resolved = match catalog.resolve(&candidate.selection) {
            Ok(m) => m,
            Err(e) => {
                row.exclusions.push(e);
                return row;
            }
        };
        row.selection = Some(resolved.frozen_selection());
        let material = resolved.material();
        if !self.forms.is_empty() && !self.forms.contains(&material.form) {
            row.exclusions
                .push(invalid("material form excluded by request"));
        }
        let query = self.query.to_lowercase();
        if ![&material.id, &material.name]
            .into_iter()
            .chain(material.structure.values())
            .chain(material.composition.iter().map(|c| &c.fiber))
            .any(|s| s.to_lowercase().contains(&query))
        {
            row.exclusions.push(invalid(
                "identity/construction/composition does not match query",
            ));
        }
        for capability in &self.required_capabilities {
            if let Err(e) = check_capability(&resolved, *capability) {
                row.exclusions
                    .push(invalid(format!("{capability:?}: {}", e.message)));
            }
        }
        for a in attributes.values() {
            match reading(&resolved, a, self.basis) {
                Ok(q) => {
                    row.readings.insert(a.key.clone(), q);
                }
                Err(e) => row.exclusions.push(e),
            }
        }
        for c in &self.constraints {
            if let Some(q) = row.readings.get(&c.attribute.key) {
                let [lo, hi] = interval(q, self.basis);
                if c.minimum.is_some_and(|v| lo < v) || c.maximum.is_some_and(|v| hi > v) {
                    row.exclusions.push(invalid(format!(
                        "{}: observation interval [{lo}, {hi}] {} exceeds requested bounds",
                        c.attribute.key, c.attribute.unit
                    )));
                }
            }
        }
        row.eligible = row.exclusions.is_empty();
        row
    }

    fn dominates(&self, a: &Alternative, b: &Alternative) -> bool {
        if !a.eligible || !b.eligible {
            return false;
        }
        let mut strict = false;
        for o in &self.objectives {
            let aq = &a.readings[&o.attribute.key];
            let bq = &b.readings[&o.attribute.key];
            if !self.allow_mixed_protocols
                && (aq.method != bq.method || aq.conditions != bq.conditions)
            {
                return false;
            }
            let [al, ah] = interval(aq, self.basis);
            let [bl, bh] = interval(bq, self.basis);
            let (better, worse) = match o.direction {
                Direction::Minimize => (ah, bl),
                Direction::Maximize => (-al, -bh),
            };
            if better > worse {
                return false;
            }
            strict |= better < worse;
        }
        strict
    }
}

fn check_capability(m: &ResolvedMaterial, capability: Capability) -> Result<(), Refusal> {
    match capability {
        Capability::Mechanics => {
            m.mechanics()?;
            m.membrane_response()?;
            m.membrane_relaxation()?;
            m.membrane_plasticity()?;
            m.bending_response()?;
            m.bending_plasticity()?;
        }
        Capability::SteadyConduction => {
            ConductionLaw::from_material(m.material(), m.allows_estimates())?;
        }
        Capability::TransientThermal => {
            ThermalLaw::from_material(m.material(), m.allows_estimates())?;
        }
    }
    Ok(())
}

fn reading(m: &ResolvedMaterial, a: &Attribute, basis: Basis) -> Result<Quantity, Refusal> {
    m.material()
        .require(&a.key, &a.unit, m.allows_estimates())?;
    let q = &m.material().properties[&a.key];
    if a.conditions
        .iter()
        .any(|(k, v)| q.conditions.get(k) != Some(v))
        || a.method.as_ref().is_some_and(|s| s != &q.method)
    {
        return Err(invalid(format!(
            "{}: observation does not meet requested method/conditions",
            a.key
        )));
    }
    if matches!(basis, Basis::ReportedBounds) && q.bounds.is_none() {
        return Err(invalid(format!(
            "{}: reported bounds are required and absent",
            a.key
        )));
    }
    Ok(q.clone())
}

fn interval(q: &Quantity, basis: Basis) -> [f64; 2] {
    match basis {
        Basis::Nominal => [q.value, q.value],
        // Called only for readings admitted by `reading`; no unknown interval is fabricated.
        Basis::ReportedBounds => q.bounds.expect("admitted interval reading"),
    }
}

//! Shared retained-extra and fixed liquid mass on physical pattern instances.
//! Extras are absent from the cut sheets. Their total group mass is never multiplied by target count.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::{
    Pattern,
    materials::{Evidence, Form, Material, Quantity, Source},
};

/// Explicit mechanical mass law; the liquid has no relative motion or mass exchange.
pub const RETAINED_LIQUID_MODEL: &str = "dry_textile_plus_fixed_retained_liquid/1";

/// One independently qualified, spatially uniform loading on reference textile area.
/// The source observations remain in the complete material snapshot, not in this derived receipt.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct RetainedLiquidMass {
    /// Source dry substrate mass per reference area, excluding this liquid, kg/m².
    pub dry_textile_kg_m2: f64,
    /// Additional co-moving liquid per reference area, kg/m², conserved throughout mechanics.
    pub retained_liquid_kg_m2: f64,
    /// Dry substrate plus liquid, used for both inertia and gravity, kg/m².
    pub total_kg_m2: f64,
    /// Sum of source endpoints only when both observations supply complete bounds.
    pub reported_total_bounds_kg_m2: Option<[f64; 2]>,
}

fn wet_conditions(
    reference: &BTreeMap<String, String>,
    conditions: &BTreeMap<String, String>,
    loading: f64,
) -> Result<(), Refusal> {
    if ["specimen_id", "conditioning"]
        .iter()
        .any(|key| conditions.get(*key) != reference.get(*key))
        || conditions
            .get("retained_liquid_kg_m2")
            .and_then(|s| s.parse::<f64>().ok())
            != Some(loading)
    {
        return Err(invalid(
            "wet mechanics need independently qualified observations at the exact retained liquid loading, specimen and conditioning",
        ));
    }
    Ok(())
}

impl Material {
    /// Resolve optional fixed retained liquid independently of the sheet constitutive capabilities.
    /// The dry mass must explicitly exclude liquid; no moisture-regain or wet-total interpretation
    /// is inferred. This qualifies accounting, not the response of an unmeasured wet textile.
    ///
    /// # Errors
    /// Unsupported model/form, unadmitted or mismatched source observations, nonphysical numbers,
    /// or a missing declaration of the closed co-moving reference-area conservation regime.
    pub fn retained_liquid_mass(
        &self,
        allow_estimates: bool,
    ) -> Result<Option<RetainedLiquidMass>, Refusal> {
        let Some(model) = self.structure.get("mechanical_mass_model") else {
            return Ok(None);
        };
        self.validate()?;
        if model != RETAINED_LIQUID_MODEL || matches!(self.form, Form::Fiber | Form::Yarn) {
            return Err(invalid(
                "unsupported retained-liquid mass model or material form",
            ));
        }
        let dry = self.require("areal_density", "kg/m2", allow_estimates)?;
        let liquid = self.require("retained_liquid_areal_density", "kg/m2", allow_estimates)?;
        let dry_observation = &self.properties["areal_density"];
        let observation = &self.properties["retained_liquid_areal_density"];
        let conditions = &observation.conditions;
        for key in ["specimen_id", "conditioning"] {
            let identity = conditions
                .get(key)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| invalid(format!("retained liquid needs explicit {key}")))?;
            if dry_observation.conditions.get(key) != Some(identity) {
                return Err(invalid(format!(
                    "dry mass and retained liquid must match {key}"
                )));
            }
        }
        for q in [dry_observation, observation] {
            if q.conditions.get("reference_area_basis").map(String::as_str)
                != Some("dry_textile_reference_area")
            {
                return Err(invalid(
                    "retained liquid and dry mass need the same dry textile reference-area basis",
                ));
            }
        }
        if dry_observation
            .conditions
            .get("mass_basis")
            .map(String::as_str)
            != Some("dry_textile_excluding_retained_liquid")
            || conditions.get("mass_basis").map(String::as_str)
                != Some("additional_retained_liquid")
            || conditions.get("retention_basis").map(String::as_str)
                != Some("closed_co_moving_no_exchange")
            || conditions
                .get("qualification")
                .is_none_or(|v| v.trim().len() < 8 || v.len() > 4096)
        {
            return Err(invalid(
                "fixed retained liquid needs independent dry/additional mass bases and qualified closed co-moving no-exchange retention",
            ));
        }
        let total = dry + liquid;
        if dry <= 0.0
            || liquid < 0.0
            || !total.is_finite()
            || (liquid > 0.0 && (total <= dry || total <= liquid))
            || dry_observation.bounds.is_some_and(|b| b[0] <= 0.0)
            || observation.bounds.is_some_and(|b| b[0] < 0.0)
        {
            return Err(invalid(
                "dry and additional liquid mass must be physical and their sum representable",
            ));
        }
        let reported_total_bounds_kg_m2 = match (dry_observation.bounds, observation.bounds) {
            (Some(d), Some(w)) => {
                let sum = [d[0] + w[0], d[1] + w[1]];
                if sum.iter().any(|v| !v.is_finite())
                    || (0..2).any(|i| w[i] > 0.0 && (sum[i] <= d[i] || sum[i] <= w[i]))
                {
                    return Err(invalid(
                        "retained liquid source mass bounds are not representable",
                    ));
                }
                Some(sum)
            }
            _ => None,
        };
        Ok(Some(RetainedLiquidMass {
            dry_textile_kg_m2: dry,
            retained_liquid_kg_m2: liquid,
            total_kg_m2: total,
            reported_total_bounds_kg_m2,
        }))
    }

    /// Source-qualified sheet plus optional retained-liquid mass, before area multiplication.
    /// This can support incomplete mechanical records for assembly accounting.
    ///
    /// # Errors
    /// Missing/unadmitted source mass or any retained-liquid qualification failure.
    pub fn sheet_areal_mass(
        &self,
        allow_estimates: bool,
    ) -> Result<(f64, Option<[f64; 2]>), Refusal> {
        if let Some(mass) = self.retained_liquid_mass(allow_estimates)? {
            return Ok((mass.total_kg_m2, mass.reported_total_bounds_kg_m2));
        }
        let mass = self.require("areal_density", "kg/m2", allow_estimates)?;
        let bounds = self.properties["areal_density"].bounds;
        if mass <= 0.0 || bounds.is_some_and(|b| b[0] < 0.0) {
            return Err(invalid(
                "sheet areal mass must be positive with nonnegative reported bounds",
            ));
        }
        Ok((mass, bounds))
    }

    /// Nominal inertial/gravitational density for the same explicitly qualified wet mechanical
    /// state. Every present mechanical observation must name the liquid loading and same specimen
    /// and conditioning; copying dry coefficients without new qualification cannot activate it.
    ///
    /// # Errors
    /// The mass failures above or any unqualified mechanical observation/history/curve.
    pub fn mechanical_areal_density(&self, allow_estimates: bool) -> Result<f64, Refusal> {
        let Some(mass) = self.retained_liquid_mass(allow_estimates)? else {
            return self.require("areal_density", "kg/m2", allow_estimates);
        };
        let reference = &self.properties["retained_liquid_areal_density"].conditions;
        let check = |conditions: &BTreeMap<String, String>| {
            wet_conditions(reference, conditions, mass.retained_liquid_kg_m2)
        };
        for (key, q) in &self.properties {
            if matches!(
                key.as_str(),
                "warp_stiffness"
                    | "weft_stiffness"
                    | "poisson_warp_weft"
                    | "shear_stiffness"
                    | "thickness"
                    | "friction_skin"
                    | "friction_self"
            ) || matches!(
                key.as_str(),
                "bending_rigidity"
                    | "bending_warp"
                    | "bending_weft"
                    | "bending_coupling"
                    | "bending_twist"
            ) || key.starts_with("bending_yield_")
                || key.starts_with("bending_hardening_")
                || key.starts_with("bending_curvature_limit_")
                || key.starts_with("bending_accumulation_limit_")
                || key.starts_with("preferred_curvature_")
                || key.starts_with("membrane_yield_")
                || key.starts_with("membrane_hardening_")
                || key.starts_with("membrane_strain_limit_")
                || key.starts_with("membrane_accumulation_limit_")
            {
                check(&q.conditions)?;
            }
        }
        if let Some(response) = &self.membrane_response {
            for conditions in [
                &response.warp.conditions,
                &response.weft.conditions,
                &response.shear.conditions,
                &response.coupling.conditions,
            ] {
                check(conditions)?;
            }
            if let Some(area) = &response.area_response {
                for conditions in [
                    &area.conditions,
                    &area.quadratic.conditions,
                    &area.log_squared.conditions,
                ] {
                    check(conditions)?;
                }
            }
        }
        if let Some(law) = &self.membrane_relaxation {
            check(&law.conditions)?;
            if let Some(parallel) = &law.parallel_plasticity {
                check(&parallel.conditions)?;
            }
            for branch in &law.branches {
                check(&branch.stiffness_ratio.conditions)?;
                check(&branch.relaxation_time.conditions)?;
            }
            for branch in &law.orthotropic_branches {
                for q in [
                    &branch.stiffness.warp,
                    &branch.stiffness.weft,
                    &branch.stiffness.coupling,
                    &branch.stiffness.shear,
                    &branch.relaxation_time,
                ] {
                    check(&q.conditions)?;
                }
            }
        }
        Ok(mass.total_kg_m2)
    }

    /// Admit one mechanical observation for a consumer such as sewing clearance. Other incomplete
    /// capabilities remain independent; a sewing-only record need not contain a membrane law.
    ///
    /// # Errors
    /// Selected retained-liquid mass lacks qualification, or this observation has stale dry/wet
    /// specimen, conditioning or loading. Absent mass selectors preserve existing behavior.
    pub fn qualify_retained_liquid_property(
        &self,
        key: &str,
        allow_estimates: bool,
    ) -> Result<(), Refusal> {
        let Some(mass) = self.retained_liquid_mass(allow_estimates)? else {
            return Ok(());
        };
        let observation = self
            .properties
            .get(key)
            .ok_or_else(|| invalid(format!("missing wet mechanical observation {key}")))?;
        wet_conditions(
            &self.properties["retained_liquid_areal_density"].conditions,
            &observation.conditions,
            mass.retained_liquid_kg_m2,
        )
    }

    /// Dry transport/storage consumers cannot infer the thermal state of a fixed wet mass.
    ///
    /// # Errors
    /// Any selected mechanical mass extension: separately coupled liquid heat is not yet supported.
    pub fn require_dry_mass_transport(&self) -> Result<(), Refusal> {
        if self.structure.contains_key("mechanical_mass_model") {
            return Err(invalid(
                "fixed retained-liquid mechanics does not qualify dry thermal/vapor transport or liquid heat storage; use an independently qualified coupled liquid model",
            ));
        }
        Ok(())
    }
}

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

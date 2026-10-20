//! Prescribed-pressure needle stacks through the shared material-compression authority. Each ply
//! sees the same declared pressure; this is not inferred foot contact, compression history or a
//! machine command. Original material identities and unloaded assembly geometry stay unchanged.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_compression::{Input, Report, condition};
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde::{Deserialize, Serialize};

use super::{bad, positive, qualified_text, range, within};

/// Explicit independent-ply, uniform-pressure closure. It does not model ply interaction or recovery.
pub const MODEL: &str = "independent_ply_equal_pressure/1";
const MAX_LAYERS: usize = 128;
const MAX_OBSERVATIONS: usize = 65_536;

/// Additional machine envelope under the foot, separate from the existing unloaded entry limit.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootLimits {
    /// Permitted prescribed positive pressure interval, Pa; no force/contact-area inference.
    pub pressure_pa: [f64; 2],
    /// Maximum loaded stack in mm; does not replace SewingMachine.max_stack_mm.
    pub max_loaded_stack_mm: f64,
}

impl FootLimits {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !range(self.pressure_pa) || !positive(self.max_loaded_stack_mm) {
            return Err(bad("invalid presser-foot pressure or loaded-stack limits"));
        }
        Ok(())
    }
}

/// An operation-local prescribed-pressure calculation on exact existing source selections.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal [`MODEL`]; independent-ply additivity is an explicit approximation.
    pub model: String,
    /// Common pressure applied to every ply, Pa, also selected in each retained acquisition.
    pub pressure_pa: f64,
    /// Exact shared acquisition conditioning, not inferred from fabric family or a machine ID.
    pub conditioning: String,
    /// Exact characterized loading branch; no loading/unloading cycle is synthesized.
    pub loading_branch: String,
    /// Complete existing compression requests keyed by the ORIGINAL resolved material hash.
    /// Every distinct ply needs one entry, with no unused entries or unchanged-property fallback.
    pub materials: BTreeMap<String, Input>,
}

/// Additional process-qualification envelope for an operation that explicitly uses foot pressure.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    /// Qualified pressure interval, Pa.
    pub pressure_pa: [f64; 2],
    /// Qualified loaded-stack interval, mm; the existing stack_mm still checks unloaded thickness.
    pub loaded_stack_mm: [f64; 2],
    /// Exact acquisition conditioning admitted by the process source.
    pub conditioning: String,
    /// Exact loading branch admitted by the process source.
    pub loading_branch: String,
}

impl Qualification {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !range(self.pressure_pa)
            || !range(self.loaded_stack_mm)
            || !qualified_text(&self.conditioning)
            || !qualified_text(&self.loading_branch)
        {
            return Err(bad("invalid pressure-qualified sewing envelope"));
        }
        Ok(())
    }

    pub(super) fn admits(&self, receipt: &Receipt) -> bool {
        within(receipt.pressure_pa, self.pressure_pa)
            && within(receipt.stack_mm, self.loaded_stack_mm)
            && self.conditioning == receipt.conditioning
            && self.loading_branch == receipt.loading_branch
    }
}

/// One ply in the unchanged operation stack; order matches assembly layers or declared A/B groups.
#[derive(Clone, Debug, Serialize)]
pub struct Layer {
    /// Original material identity used by the pattern, seam qualification and material inventory.
    pub material_sha256: String,
    /// Derived pressure-state identity; never substituted into the original pattern or inventory.
    pub conditioned_material_sha256: String,
    /// Central loaded thickness in mm, not silently replaced by the upper envelope.
    pub nominal_thickness_mm: f64,
    /// Thickness used for limit checking, mm: reported upper bound when available, else central.
    pub thickness_mm: f64,
    /// Whether thickness_mm uses a source-reported upper bound; no confidence level is inferred.
    pub uses_thickness_upper_bound: bool,
    /// Bottom/top loaded bounds in mm for the selected stack order, starting at zero, not a bed frame.
    pub z_bounds_mm: [f64; 2],
}

/// Source-complete loaded stack alongside, not in place of, the original sewing assembly.
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// [`MODEL`].
    pub model: &'static str,
    /// Common applied pressure, Pa.
    pub pressure_pa: f64,
    /// Retained acquisition conditioning.
    pub conditioning: String,
    /// Retained characterized branch.
    pub loading_branch: String,
    /// Sum of central loaded ply thicknesses, mm.
    pub nominal_stack_mm: f64,
    /// Sum of admitted loaded ply thicknesses, mm; missing bounds remain explicitly per-ply missing.
    pub stack_mm: f64,
    /// Every physical ply, including repeated folded regions of one source sheet.
    pub layers: Vec<Layer>,
    /// Existing compression receipts, once per distinct original material rather than once per ply.
    pub materials: BTreeMap<String, Report>,
}

/// Shared across the whole sewing job, including repeated operation-local acquisitions.
#[derive(Default)]
pub(super) struct Budget(usize);

impl Request {
    pub(super) fn validate(&self, limits: Option<&FootLimits>) -> Result<(), Refusal> {
        let limits =
            limits.ok_or_else(|| bad("foot pressure needs explicit loaded machine limits"))?;
        limits.validate()?;
        if self.model != MODEL
            || !within(self.pressure_pa, limits.pressure_pa)
            || !qualified_text(&self.conditioning)
            || !qualified_text(&self.loading_branch)
            || self.materials.is_empty()
            || self.materials.len() > MAX_LAYERS
        {
            return Err(bad("invalid or out-of-envelope presser-foot request"));
        }
        Ok(())
    }

    pub(super) fn evaluate(
        &self,
        limits: Option<&FootLimits>,
        stack: &[String],
        originals: &BTreeMap<String, Selection>,
        budget: &mut Budget,
    ) -> Result<Receipt, Refusal> {
        self.validate(limits)?;
        if stack.is_empty() || stack.len() > MAX_LAYERS {
            return Err(bad("presser-foot stack needs 1..128 actual plies"));
        }
        let distinct: BTreeSet<_> = stack.iter().collect();
        if !distinct.iter().copied().eq(self.materials.keys()) {
            return Err(bad(
                "foot pressure needs exactly one characterization for every distinct stack material",
            ));
        }
        let db = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: Vec::new(),
        };
        let mut materials = BTreeMap::new();
        let mut thicknesses = BTreeMap::new();
        for (hash, input) in &self.materials {
            // Charge input-sized work before cloning, normalizing or conditioning any record.
            let cells = input
                .samples
                .iter()
                .try_fold(0usize, |sum, row| sum.checked_add(row.properties.len()));
            budget.0 = cells
                .and_then(|n| budget.0.checked_add(n))
                .filter(|&n| n <= MAX_OBSERVATIONS)
                .ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "sewing compression observation budget exceeded",
                    )
                })?;
            let selection = originals
                .get(hash)
                .ok_or_else(|| bad("missing original stack material"))?;
            let original = db.resolve(selection)?;
            let acquired = db.resolve(&Selection {
                id: input.material.id.clone(),
                record: Some(input.material.clone()),
                allow_estimates: original.allows_estimates(),
                ..Selection::default()
            })?;
            if original.sha256() != hash || acquired.sha256() != hash {
                return Err(bad(
                    "compression acquisition does not match the exact original material snapshot",
                ));
            }
            if input.pressure_pa != self.pressure_pa
                || input.conditioning != self.conditioning
                || input.loading_branch != self.loading_branch
            {
                return Err(bad(
                    "all plies must use the declared pressure, conditioning and loading branch",
                ));
            }
            let report = condition(input)?;
            let conditioned = db.resolve(&report.selection)?;
            // Acquisition interpolation consent cannot weaken the original material's consumption policy.
            let nominal =
                conditioned
                    .material()
                    .require("thickness", "m", original.allows_estimates())?
                    * 1000.0;
            let bounds = conditioned.material().properties["thickness"].bounds;
            let admitted = bounds.map_or(nominal, |b| b[1] * 1000.0);
            if !positive(nominal) || !positive(admitted) {
                return Err(bad("nonrepresentable loaded ply thickness"));
            }
            thicknesses.insert(
                hash.clone(),
                (
                    nominal,
                    admitted,
                    bounds.is_some(),
                    conditioned.sha256().to_owned(),
                ),
            );
            materials.insert(hash.clone(), report);
        }
        let mut receipt = Receipt {
            model: MODEL,
            pressure_pa: self.pressure_pa,
            conditioning: self.conditioning.clone(),
            loading_branch: self.loading_branch.clone(),
            nominal_stack_mm: 0.0,
            stack_mm: 0.0,
            layers: Vec::with_capacity(stack.len()),
            materials,
        };
        for hash in stack {
            let (nominal, admitted, upper, conditioned) = &thicknesses[hash];
            let bottom = receipt.stack_mm;
            receipt.stack_mm += admitted;
            receipt.nominal_stack_mm += nominal;
            if !receipt.stack_mm.is_finite() || !receipt.nominal_stack_mm.is_finite() {
                return Err(bad("non-finite pressure-conditioned stack thickness"));
            }
            receipt.layers.push(Layer {
                material_sha256: hash.clone(),
                conditioned_material_sha256: conditioned.clone(),
                nominal_thickness_mm: *nominal,
                thickness_mm: *admitted,
                uses_thickness_upper_bound: *upper,
                z_bounds_mm: [bottom, receipt.stack_mm],
            });
        }
        if receipt.stack_mm > limits.expect("validated foot limits").max_loaded_stack_mm {
            return Err(bad(
                "pressure-conditioned stack exceeds the loaded machine limit",
            ));
        }
        Ok(receipt)
    }
}

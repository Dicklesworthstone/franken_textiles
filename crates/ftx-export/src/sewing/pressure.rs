//! Needle stacks through the shared material-compression authority. Pressure is either prescribed
//! or inferred from a declared rigid-face gap. Both modes keep original material identities and
//! unloaded assembly geometry; neither is a spatial foot-contact solve or a machine command.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_compression::stack::{self, Direction};
use ftx_pattern::material_compression::{Input, Report, condition};
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog};
use serde::{Deserialize, Serialize};

use super::{bad, positive, qualified_text, range, within};

/// Explicit independent-ply, uniform-pressure closure. It does not model ply interaction or recovery.
pub const MODEL: &str = "independent_ply_equal_pressure/1";
/// Inverse fixed-area, parallel-face closure using the shared compression-stack implementation.
pub const CLEARANCE_MODEL: &str = "independent_ply_clearance/1";
const MAX_LAYERS: usize = 128;
const MAX_OBSERVATIONS: usize = 65_536;

/// Which inverse result must lie inside the independent machine and process envelopes.
/// Reported bounds are conditional intervals, not a statistical or physical safety certificate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureForceBasis {
    /// Existing central-value checks; does not establish robustness to thickness uncertainty.
    #[default]
    Nominal,
    /// Require complete, domain-supported reported thickness bounds through the inverse.
    ReportedBounds,
}

impl PressureForceBasis {
    fn is_nominal(&self) -> bool {
        *self == Self::Nominal
    }

    fn ranges(self, closure: &stack::Report) -> Option<stack::ReportedBounds> {
        match self {
            Self::Nominal => Some(stack::ReportedBounds {
                pressure_pa: [closure.pressure_pa; 2],
                normal_force_n: [closure.normal_force_n; 2],
            }),
            Self::ReportedBounds => closure.reported_bounds.clone(),
        }
    }
}

fn interval_within(values: [f64; 2], limits: [f64; 2]) -> bool {
    // The same arithmetic tolerance as the existing scalar gate, never an added process margin.
    values.iter().all(|&v| within(v, limits))
}

/// Additional machine envelope under the foot, separate from the existing unloaded entry limit.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootLimits {
    /// Permitted prescribed positive pressure interval, Pa; no force/contact-area inference.
    pub pressure_pa: [f64; 2],
    /// Maximum loaded stack in mm; does not replace SewingMachine.max_stack_mm.
    pub max_loaded_stack_mm: f64,
    /// Final normal-force envelope in N, mandatory in clearance mode; not inferred from pressure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_force_n: Option<[f64; 2]>,
}

impl FootLimits {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !range(self.pressure_pa)
            || !positive(self.max_loaded_stack_mm)
            || self.normal_force_n.is_some_and(|r| !range(r))
        {
            return Err(bad("invalid presser-foot pressure or loaded-stack limits"));
        }
        Ok(())
    }
}

/// An operation-local prescribed-pressure calculation on exact existing source selections.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// [`MODEL`] for prescribed pressure or [`CLEARANCE_MODEL`] for a prescribed gap.
    pub model: String,
    /// Prescribed common pressure, Pa. Must be absent when clearance is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_pa: Option<f64>,
    /// Prescribed final gap and reference area; input material pressures are the known initial state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Clearance>,
    /// Exact shared acquisition conditioning, not inferred from fabric family or a machine ID.
    pub conditioning: String,
    /// Exact characterized loading branch; no loading/unloading cycle is synthesized.
    pub loading_branch: String,
    /// Complete existing compression requests keyed by the ORIGINAL resolved material hash.
    /// Every distinct ply needs one entry, with no unused entries or unchanged-property fallback.
    pub materials: BTreeMap<String, Input>,
}

/// Geometry-controlled static closure of the ACTUAL operation stack, not a guessed pressure.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clearance {
    /// Positive final parallel-face gap in millimetres; central source thicknesses determine closure.
    pub gap_mm: f64,
    /// Fixed common reference/contact area in mm², explicitly supplied, not derived from stitch paths.
    pub reference_area_mm2: f64,
    /// Traverse the supplied source branches from their common known initial pressure.
    pub direction: Direction,
    /// Separate permission for model-inferred pressure/work; never relaxes sheet evidence policy.
    pub allow_estimates: bool,
    /// Opt into checking the complete pressure/force range from reported thickness bounds.
    /// Missing or domain-limited bounds refuse; they never fall back to nominal values.
    /// Other quantities (loaded ply geometry, work and heat) remain nominal-state calculations.
    #[serde(default, skip_serializing_if = "PressureForceBasis::is_nominal")]
    pub pressure_force_basis: PressureForceBasis,
}

/// Independent process evidence for geometry-controlled closure. Pressure-only recipes do not match.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearanceQualification {
    /// Qualified prescribed-gap range, mm.
    pub gap_mm: [f64; 2],
    /// Qualified common reference-area range, mm².
    pub reference_area_mm2: [f64; 2],
    /// Qualified final normal-force range, N.
    pub normal_force_n: [f64; 2],
    /// Qualified branch-traverse direction; no loading/unloading equivalence is inferred.
    pub direction: Direction,
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
    /// Must be present exactly when the operation requests clearance-controlled closure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<ClearanceQualification>,
}

impl Qualification {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if !range(self.pressure_pa)
            || !range(self.loaded_stack_mm)
            || !qualified_text(&self.conditioning)
            || !qualified_text(&self.loading_branch)
            || self.clearance.as_ref().is_some_and(|q| {
                !range(q.gap_mm) || !range(q.reference_area_mm2) || !range(q.normal_force_n)
            })
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
            && match (&self.clearance, &receipt.closure) {
                (None, None) => receipt.pressure_force_basis.is_nominal(),
                (Some(q), Some(r)) => {
                    within(r.input.gap_m * 1000.0, q.gap_mm)
                        && within(r.input.reference_area_m2 * 1e6, q.reference_area_mm2)
                        && receipt.pressure_force_basis.ranges(r).is_some_and(|b| {
                            interval_within(b.pressure_pa, self.pressure_pa)
                                && interval_within(b.normal_force_n, q.normal_force_n)
                        })
                        && r.input.direction == q.direction
                }
                _ => false,
            }
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
    /// [`MODEL`] or [`CLEARANCE_MODEL`]; the pressure's origin is never implicit.
    pub model: &'static str,
    /// Common applied pressure, Pa.
    pub pressure_pa: f64,
    /// Decision basis for pressure/force gates; omission retains the existing nominal contract.
    /// The closure receipt contains the actual interval and its qualification limitations.
    #[serde(skip_serializing_if = "PressureForceBasis::is_nominal")]
    pub pressure_force_basis: PressureForceBasis,
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
    /// Complete inverse/path receipt in clearance mode, with a source record for EVERY physical ply.
    /// In that mode `materials` is empty; repeated plies have distinct closure source identities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure: Option<Box<stack::Report>>,
}

/// Shared across the whole sewing job, including repeated operation-local acquisitions.
#[derive(Default)]
pub(super) struct Budget(usize);

impl Request {
    pub(super) fn validate(&self, limits: Option<&FootLimits>) -> Result<(), Refusal> {
        let limits =
            limits.ok_or_else(|| bad("foot pressure needs explicit loaded machine limits"))?;
        limits.validate()?;
        let mode_valid = match (self.model.as_str(), self.pressure_pa, &self.clearance) {
            (MODEL, Some(p), None) => within(p, limits.pressure_pa),
            (CLEARANCE_MODEL, None, Some(c)) => {
                positive(c.gap_mm / 1000.0)
                    && positive(c.reference_area_mm2 / 1e6)
                    && c.allow_estimates
                    && limits.normal_force_n.is_some()
            }
            _ => false,
        };
        if !mode_valid
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
            // Inverse closure visits every physical ply, including repeated folds of one source.
            let copies = if self.clearance.is_some() {
                stack.iter().filter(|h| *h == hash).count()
            } else {
                1
            };
            budget.0 = cells
                .and_then(|n| n.checked_mul(copies))
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
            if self.pressure_pa.is_some_and(|p| input.pressure_pa != p)
                || input.conditioning != self.conditioning
                || input.loading_branch != self.loading_branch
            {
                return Err(bad(
                    "all plies must use the declared pressure, conditioning and loading branch",
                ));
            }
            if self.clearance.is_some() {
                continue; // the shared inverse conditions the known initial and solved final states
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
        if let Some(clearance) = &self.clearance {
            return self.close(
                clearance,
                limits.expect("validated limits"),
                stack,
                originals,
                &db,
            );
        }
        let mut receipt = Receipt {
            model: MODEL,
            pressure_pa: self.pressure_pa.expect("validated prescribed pressure"),
            pressure_force_basis: PressureForceBasis::Nominal,
            conditioning: self.conditioning.clone(),
            loading_branch: self.loading_branch.clone(),
            nominal_stack_mm: 0.0,
            stack_mm: 0.0,
            layers: Vec::with_capacity(stack.len()),
            materials,
            closure: None,
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

    fn close(
        &self,
        clearance: &Clearance,
        limits: &FootLimits,
        stack: &[String],
        originals: &BTreeMap<String, Selection>,
        db: &Catalog,
    ) -> Result<Receipt, Refusal> {
        // The inverse authority supports at most 32 plies; refuse BEFORE cloning acquisitions.
        if stack.len() > 32 {
            return Err(bad("clearance closure supports at most 32 actual plies"));
        }
        let closed = stack::close(&stack::Input {
            schema: stack::SCHEMA.into(),
            gap_m: clearance.gap_mm / 1000.0,
            reference_area_m2: clearance.reference_area_mm2 / 1e6,
            direction: clearance.direction,
            allow_estimates: clearance.allow_estimates,
            layers: stack
                .iter()
                .enumerate()
                .map(|(i, hash)| stack::Layer {
                    id: format!("ply-{i}"),
                    preparation: self.materials[hash].clone(),
                })
                .collect(),
        })?;
        let checked = clearance
            .pressure_force_basis
            .ranges(&closed)
            .ok_or_else(|| {
                bad(format!(
                    "clearance pressure/force reported bounds are unavailable: {}",
                    closed.bounds_unavailable.join("; ")
                ))
            })?;
        if !interval_within(checked.pressure_pa, limits.pressure_pa)
            || !interval_within(
                checked.normal_force_n,
                limits.normal_force_n.expect("validated force limits"),
            )
        {
            return Err(bad(if clearance.pressure_force_basis.is_nominal() {
                "inferred foot pressure or normal force exceeds the machine envelope"
            } else {
                "inferred foot pressure or normal force reported bounds exceed the machine envelope"
            }));
        }
        let mut receipt = Receipt {
            model: CLEARANCE_MODEL,
            pressure_pa: closed.pressure_pa,
            pressure_force_basis: clearance.pressure_force_basis,
            conditioning: self.conditioning.clone(),
            loading_branch: self.loading_branch.clone(),
            nominal_stack_mm: 0.0,
            stack_mm: 0.0,
            layers: Vec::with_capacity(stack.len()),
            materials: BTreeMap::new(),
            closure: None,
        };
        for (hash, layer) in stack.iter().zip(&closed.layers) {
            let original = db.resolve(&originals[hash])?;
            let conditioned = db.resolve(&layer.conditioned.selection)?;
            let nominal =
                conditioned
                    .material()
                    .require("thickness", "m", original.allows_estimates())?
                    * 1000.0;
            let bounds = conditioned.material().properties["thickness"].bounds;
            let admitted = bounds.map_or(nominal, |b| b[1] * 1000.0);
            let bottom = receipt.stack_mm;
            receipt.nominal_stack_mm += nominal;
            receipt.stack_mm += admitted;
            if !positive(nominal)
                || !positive(admitted)
                || !receipt.stack_mm.is_finite()
                || !receipt.nominal_stack_mm.is_finite()
            {
                return Err(bad(
                    "nonrepresentable clearance-conditioned stack thickness",
                ));
            }
            receipt.layers.push(Layer {
                material_sha256: hash.clone(),
                conditioned_material_sha256: conditioned.sha256().to_owned(),
                nominal_thickness_mm: nominal,
                thickness_mm: admitted,
                uses_thickness_upper_bound: bounds.is_some(),
                z_bounds_mm: [bottom, receipt.stack_mm],
            });
        }
        if receipt.stack_mm > limits.max_loaded_stack_mm {
            return Err(bad(
                "clearance-conditioned stack exceeds the loaded machine limit",
            ));
        }
        receipt.closure = Some(Box::new(closed));
        Ok(receipt)
    }
}

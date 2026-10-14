//! One material resolution boundary for static draping, two-start studies and motion continuation.
//! Material observations own physical thickness and friction; caller options own numerical contact
//! tolerances and whether self-contact is enabled. Grain rotation remains in the existing scene builder.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::Pattern;
use ftx_pattern::materials::Mechanics;
use ftx_pattern::textile_import::resolve_fabric;
use serde_json::{Value, json};

use crate::elements::Membrane;
use crate::run::DrapeOptions;

/// Effective sheet coefficients; legacy defaults reproduce spec 04 §5.3 exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fabric {
    /// Reference areal density (kg/m²).
    pub areal_kg_m2: f64,
    /// Warp stiffness (N/m); also weft stiffness in legacy isotropic mode.
    pub tensile_n_m: f64,
    /// Scalar bending rigidity (N·m).
    pub bending_nm: f64,
    /// Weft stiffness, engineering shear stiffness and major Poisson ratio, when characterized.
    directional: Option<[f64; 3]>,
}

impl Default for Fabric {
    fn default() -> Self {
        Self {
            areal_kg_m2: 0.2,
            tensile_n_m: 8000.0,
            bending_nm: 3e-5,
            directional: None,
        }
    }
}

impl Fabric {
    // Private: only the validated common resolver supplies this bundle.
    fn from_mechanics(m: Mechanics) -> Self {
        Self {
            areal_kg_m2: m.areal_kg_m2,
            tensile_n_m: m.warp_n_m,
            bending_nm: m.bending_nm,
            directional: Some([m.weft_n_m, m.shear_n_m, m.poisson]),
        }
    }

    /// Loads the legacy three-number contract. Named/inline records must use [`resolve`] so contact
    /// and evidence cannot be lost. Missing legacy values keep their historical defaults.
    ///
    /// # Errors
    /// Material selection on the legacy path, or a present nonnumeric/nonpositive parameter.
    pub fn of(pattern: &Pattern) -> Result<Self, Refusal> {
        let f = &pattern.raw()["fabric"];
        if f.get("material").is_some() || f.get("material_id").is_some() {
            return Err(invalid(
                "material records require the shared material resolver",
            ));
        }
        let number = |key: &str, default: f64| -> Result<f64, Refusal> {
            let Some(v) = f.get(key) else {
                return Ok(default);
            };
            let n = v
                .as_f64()
                .ok_or_else(|| invalid(format!("fabric.{key} must be a number")))?;
            if !n.is_finite() || n <= 0.0 {
                return Err(invalid(format!("fabric.{key} must be finite and positive")));
            }
            Ok(n)
        };
        let d = Self::default();
        Ok(Self {
            areal_kg_m2: number("areal_g_m2", d.areal_kg_m2 * 1000.0)? / 1000.0,
            tensile_n_m: number("tensile_N_m", d.tensile_n_m)?,
            bending_nm: number("bending_Nm", d.bending_nm)?,
            directional: None,
        })
    }

    /// Material-frame membrane. Legacy bias shear remains exactly 0.2·E; sourced records use G12.
    #[must_use]
    pub fn membrane(&self, compression: f64) -> Membrane {
        let e = self.tensile_n_m;
        let [weft, shear, poisson] = self.directional.unwrap_or([e, 0.2 * e, 0.0]);
        Membrane::orthotropic(e, weft, poisson, shear, compression)
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// The physical/numerical inputs and their report are resolved together, before building any mesh.
pub struct MaterialInputs {
    /// Effective membrane, mass and bending parameters.
    pub fabric: Fabric,
    /// Caller numerical settings with material-owned contact coefficients applied.
    pub options: DrapeOptions,
    /// Evidence snapshot, effective bundle and explicit model limits.
    pub report: Value,
}

/// Resolves `pattern.fabric` for every ordinary drape consumer. Material selection supersedes caller
/// thickness/friction ONLY; barrier width, penalty, friction regularization and self-contact enablement
/// stay numerical run options. Body offset = t/2; same-cloth offset = t, not twice t.
///
/// # Errors
/// Invalid input/options, incomplete characterization or unsupported per-piece material assignment.
pub fn resolve(pattern: &Pattern, options: &DrapeOptions) -> Result<MaterialInputs, Refusal> {
    options.validate()?;
    let raw = pattern.raw();
    if let Some(f) = raw.get("fabric")
        && !f.is_object()
    {
        return Err(invalid("fabric must be an object"));
    }
    // A piece's `material` string is a cutting label (the exporters print it in cut tables, and the
    // reference drapes every piece in pattern.fabric), so only a per-piece characterized selection is refused.
    if let Some(pieces) = raw["pieces"].as_array() {
        for p in pieces {
            if p.get("material").is_some_and(|m| !m.is_string())
                || p.get("material_id").is_some()
                || p["fabric"].get("material").is_some()
                || p["fabric"].get("material_id").is_some()
            {
                return Err(invalid(
                    "per-piece materials need heterogeneous assembly; use a uniform pattern.fabric material until that is implemented",
                ));
            }
        }
    }
    let mut opts = *options;
    let Some(record) = resolve_fabric(&raw["fabric"])? else {
        return Ok(MaterialInputs {
            fabric: Fabric::of(pattern)?,
            options: opts,
            report: json!({"basis":"legacy_uncertain", "model":"orthotropic_green_strain_scalar_bending",
                "note":"Legacy defaults/declarations are not characterized textile properties."}),
        });
    };
    let mechanics = record.mechanics()?;
    let properties = &record.material().properties;
    let resistance = if properties.contains_key("thermal_resistance")
        || properties.contains_key("thermal_conductivity")
    {
        Some(record.thermal_resistance()?)
    } else {
        None
    };
    let fabric = Fabric::from_mechanics(mechanics);
    opts.contact.thickness = 0.5 * mechanics.thickness_m;
    opts.contact.mu = mechanics.friction_skin;
    if let Some(p) = &mut opts.self_contact {
        p.thickness = mechanics.thickness_m;
        p.mu = mechanics.friction_self;
    }
    opts.validate()?;
    let thermal_basis = if properties.contains_key("thermal_resistance") {
        "supplied_resistance"
    } else if resistance.is_some() {
        "dry_slab_thickness_over_conductivity"
    } else {
        "unknown"
    };
    Ok(MaterialInputs {
        fabric,
        options: opts,
        report: json!({
            "basis":"characterized_record", "record":record, "record_sha256":record.sha256(),
            "mechanics":mechanics, "model":"orthotropic_green_strain_scalar_bending",
            "contact_policy":"material owns thickness and friction; caller owns numerical tolerances and self-contact enablement",
            "body_offset_m":opts.contact.thickness,
            "self_contact_enabled":opts.self_contact.is_some(),
            "dry_material_resistance_m2_k_w":resistance, "thermal_basis":thermal_basis,
            "not_simulated":["yarn/loop-scale nonlinear response", "directional bending", "moisture and heat transport", "garment thermal comfort", "damage/fire", "robotic sewing"],
            "fit_claim":false
        }),
    })
}

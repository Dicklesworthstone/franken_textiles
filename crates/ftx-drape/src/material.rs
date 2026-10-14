//! One material resolution boundary for static draping, two-start studies and motion continuation.
//! Material observations own physical thickness and friction; caller options own numerical contact
//! tolerances and whether self-contact is enabled. Grain rotation remains in the existing scene builder.

use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::material_relaxation::CompiledRelaxation;
use ftx_pattern::material_response::CompiledMembrane;
use ftx_pattern::materials::Mechanics;
use ftx_pattern::textile_import::resolve_fabric;
use ftx_pattern::{Grain, Pattern, truthy};
use serde_json::{Value, json};

use crate::elements::Membrane;
use crate::run::DrapeOptions;

mod interfaces;
mod pieces;

/// Contact lookup in source-hash order, independent of piece order and mesh resolution.
pub(crate) struct ContactMaterials {
    pub piece_ids: Vec<usize>,
    pub surfaces: Vec<crate::contact::SurfaceMaterial>,
    pub friction: Option<Vec<Vec<f64>>>,
}

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
    /// Validate the effective mass, bending and constitutive inputs before building geometry.
    ///
    /// # Errors
    /// Nonphysical/non-finite density, bending or a non-positive-definite membrane.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.areal_kg_m2.is_finite()
            || self.areal_kg_m2 <= 0.0
            || !self.bending_nm.is_finite()
            || self.bending_nm < 0.0
        {
            return Err(invalid(
                "fabric needs finite positive areal density and nonnegative bending",
            ));
        }
        self.membrane(1.0).validate()
    }

    pub(crate) fn requires_grain(&self) -> bool {
        self.directional.is_some()
    }

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
            if !n.is_finite() || n < 0.0 || (key != "bending_Nm" && n == 0.0) {
                return Err(invalid(format!("fabric.{key} must be finite and physical")));
            }
            Ok(n)
        };
        let d = Self::default();
        let fabric = Self {
            areal_kg_m2: number("areal_g_m2", d.areal_kg_m2 * 1000.0)? / 1000.0,
            tensile_n_m: number("tensile_N_m", d.tensile_n_m)?,
            bending_nm: number("bending_Nm", d.bending_nm)?,
            directional: None,
        };
        fabric.validate()?;
        Ok(fabric)
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

/// The same material axis for both grain wire formats; scale before mirroring the instance.
pub(crate) fn grain_direction(piece: &Value, required: bool) -> Result<Vec2, Refusal> {
    let mut d = if let Some(g) = piece.get("grain") {
        let points = serde_json::from_value::<Grain>(g.clone())
            .ok()
            .and_then(|g| g.points())
            .ok_or_else(|| invalid("grain must contain two finite points"))?;
        points[1] - points[0]
    } else if required {
        return Err(invalid(
            "characterized fabric requires an explicit nonzero grainline",
        ));
    } else {
        Vec2::new(0.0, 1.0)
    };
    if let Some(scale) = piece.get("sim_scale").filter(|v| truthy(v)) {
        let s: Vec2 = serde_json::from_value(scale.clone())
            .map_err(|_| invalid("sim_scale must be [x,y]"))?;
        if !s.is_finite() || s.x <= 0.0 || s.y <= 0.0 {
            return Err(invalid("sim_scale must be finite and positive"));
        }
        d = Vec2::new(d.x * s.x, d.y * s.y);
    }
    if !d.is_finite() || !d.norm2().is_finite() || d.norm2() == 0.0 {
        return Err(invalid(
            "grain must have finite nonzero length after sim_scale",
        ));
    }
    Ok(d)
}

fn check_grains(pattern: &Pattern, fabric: &Fabric) -> Result<(), Refusal> {
    for piece in pattern.raw()["pieces"].as_array().into_iter().flatten() {
        if piece["sim"] != false && piece.get("placement").is_some_and(truthy) {
            grain_direction(piece, fabric.requires_grain())?;
        }
    }
    Ok(())
}

/// The physical/numerical inputs and their report are resolved together, before building any mesh.
pub struct MaterialInputs {
    /// Effective membrane, mass and bending parameters.
    pub fabric: Fabric,
    /// Caller numerical settings with material-owned contact coefficients applied.
    pub options: DrapeOptions,
    /// Evidence snapshot, effective bundle and explicit model limits.
    pub report: Value,
    // Indexed by source piece, not vertex: seam welding must never merge material identities.
    pub(crate) piece_fabrics: Option<Vec<Fabric>>,
    pub(crate) response: Option<CompiledMembrane>,
    pub(crate) piece_responses: Option<Vec<Option<CompiledMembrane>>>,
    pub(crate) relaxation: Option<CompiledRelaxation>,
    pub(crate) piece_relaxations: Option<Vec<Option<CompiledRelaxation>>>,
    pub(crate) contact_materials: Option<ContactMaterials>,
    mechanics: Option<Mechanics>,
}

/// Resolves `pattern.fabric` for every ordinary drape consumer. Material selection supersedes caller
/// thickness/friction ONLY; barrier width, penalty, friction regularization and self-contact enablement
/// stay numerical run options. Body offset = t/2; same-cloth offset = t, not twice t.
///
/// # Errors
/// Invalid input/options, incomplete characterization, or unsupported layer/interface assignments.
pub fn resolve(pattern: &Pattern, options: &DrapeOptions) -> Result<MaterialInputs, Refusal> {
    options.validate()?;
    let raw = pattern.raw();
    if let Some(f) = raw.get("fabric")
        && !f.is_object()
    {
        return Err(invalid("fabric must be an object"));
    }
    pieces::resolve(pattern, resolve_uniform(pattern, options)?)
}

// This helper does not resolve piece selectors: each explicitly selected sheet uses this same
// canonical evidence/units/thermal policy before its coefficients enter the element field.
fn resolve_uniform(pattern: &Pattern, options: &DrapeOptions) -> Result<MaterialInputs, Refusal> {
    let raw = pattern.raw();
    let mut opts = *options;
    let Some(record) = resolve_fabric(&raw["fabric"])? else {
        let fabric = Fabric::of(pattern)?;
        check_grains(pattern, &fabric)?;
        return Ok(MaterialInputs {
            fabric,
            options: opts,
            report: json!({"basis":"legacy_uncertain", "model":"orthotropic_green_strain_scalar_bending",
                "note":"Legacy defaults/declarations are not characterized textile properties."}),
            piece_fabrics: None,
            response: None,
            piece_responses: None,
            relaxation: None,
            piece_relaxations: None,
            contact_materials: None,
            mechanics: None,
        });
    };
    let mechanics = record.mechanics()?;
    let response = record.membrane_response()?;
    let relaxation = record.membrane_relaxation()?;
    let properties = &record.material().properties;
    let thermal = if properties.contains_key("thermal_resistance")
        || properties.contains_key("thermal_conductivity")
    {
        Some(record.thermal_resistance())
    } else {
        None
    };
    // Thermal characterization is optional for mechanical drapes; retain why it is unavailable.
    // An explicit thermal consumer still applies its own evidence/coverage gates.
    let resistance = thermal.as_ref().and_then(|r| r.as_ref().ok()).copied();
    let thermal_failure = thermal.as_ref().and_then(|r| r.as_ref().err());
    let fabric = Fabric::from_mechanics(mechanics);
    fabric.validate()?;
    check_grains(pattern, &fabric)?;
    opts.contact.thickness = 0.5 * mechanics.thickness_m;
    opts.contact.mu = mechanics.friction_skin;
    if let Some(p) = &mut opts.self_contact {
        p.thickness = mechanics.thickness_m;
        p.mu = mechanics.friction_self;
    }
    opts.validate()?;
    let thermal_basis = if thermal_failure.is_some() {
        "unavailable"
    } else if properties.contains_key("thermal_resistance") {
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
            "mechanics":mechanics,
            "model": if response.is_some() {"tabulated_green_strain_scalar_bending"} else {"orthotropic_green_strain_scalar_bending"},
            "nonlinear_strain_range": response.as_ref().map(CompiledMembrane::strain_range),
            "relaxation_law_sha256": relaxation.as_ref().map(CompiledRelaxation::fingerprint),
            "membrane_stiffness_basis": "equilibrium; optional Maxwell branches add transient response",
            "contact_policy":"material owns thickness and friction; caller owns numerical tolerances and self-contact enablement",
            "body_offset_m":opts.contact.thickness,
            "self_contact_enabled":opts.self_contact.is_some(),
            "dry_material_resistance_m2_k_w":resistance, "thermal_basis":thermal_basis,
            "thermal_unavailable":thermal_failure,
            "not_simulated":["yarn/loop-scale homogenization", "directional bending", "plastic set/damage", "moisture and heat transport", "garment thermal comfort", "damage/fire", "robotic sewing"],
            "fit_claim":false
        }),
        piece_fabrics: None,
        response,
        piece_responses: None,
        relaxation,
        piece_relaxations: None,
        contact_materials: None,
        mechanics: Some(mechanics),
    })
}

#[cfg(test)]
mod grain_tests {
    use super::*;

    #[test]
    fn grain_wire_formats_share_checked_simulation_scaling() {
        let a = json!({"grain":[[1,2],[4,6]], "sim_scale":[2,0.5]});
        let b = json!({"grain":{"p0":[1,2],"p1":[4,6]}, "sim_scale":[2,0.5]});
        assert_eq!(grain_direction(&a, true).unwrap(), Vec2::new(6., 2.));
        assert_eq!(
            grain_direction(&a, true).unwrap(),
            grain_direction(&b, true).unwrap()
        );
        for scale in [
            json!([0, 1]),
            json!([-1, 1]),
            json!([1]),
            json!([1e308, 1e308]),
        ] {
            let mut raw = a.clone();
            raw["sim_scale"] = scale;
            assert!(grain_direction(&raw, true).is_err());
        }
    }
}

//! One material resolution boundary for static draping, two-start studies and motion continuation.
//! Material observations own physical thickness and friction; caller options own numerical contact
//! tolerances and whether self-contact is enabled. Grain rotation remains in the existing scene builder.

use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::material_bending::BendingTensor;
use ftx_pattern::material_membrane_plasticity::PlasticMembrane;
use ftx_pattern::material_plasticity::PlasticBending;
use ftx_pattern::material_relaxation::CompiledRelaxation;
use ftx_pattern::material_response::{CompiledMembrane, StrainDomain};
use ftx_pattern::materials::Mechanics;
use ftx_pattern::textile_import::resolve_fabric;
use ftx_pattern::{Grain, Pattern, truthy};
use serde_json::{Value, json};

use crate::elements::Membrane;
use crate::knit::{AsymmetricKnitBending, CoupledKnitMembrane, KnitHysteresisBank};
use crate::run::DrapeOptions;

pub(crate) mod interfaces;
mod pieces;

/// Contact lookup in source-hash order, independent of piece order and mesh resolution.
pub(crate) struct ContactMaterials {
    pub piece_ids: Vec<Option<usize>>,
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
    panel_bending: Option<BendingTensor>,
    fitted_domain: Option<StrainDomain>,
}

impl Default for Fabric {
    fn default() -> Self {
        Self {
            areal_kg_m2: 0.2,
            tensile_n_m: 8000.0,
            bending_nm: 3e-5,
            directional: None,
            panel_bending: None,
            fitted_domain: None,
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
            panel_bending: None,
            fitted_domain: None,
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
            panel_bending: None,
            fitted_domain: None,
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

    /// Explicit grain-frame panel bending; None retains scalar hinges everywhere.
    #[must_use]
    pub fn bending_response(&self) -> Option<BendingTensor> {
        self.panel_bending
    }

    /// Physical Green-strain bounds of an identified scalar law, when present.
    #[must_use]
    pub fn strain_domain(&self) -> Option<StrainDomain> {
        self.fitted_domain
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

/// Solver laws compiled from the same admitted source record as mass, contact and grain.
#[derive(Clone)]
pub(crate) struct KnitMaterial {
    pub membrane: CoupledKnitMembrane,
    pub bending: Option<AsymmetricKnitBending>,
    pub hysteresis: Option<KnitHysteresisBank>,
}

impl KnitMaterial {
    fn from_compiled(law: &ftx_pattern::material_knit::CompiledKnit) -> Result<Self, Refusal> {
        let m = &law.membrane;
        let membrane = CoupledKnitMembrane {
            q11: m.q11,
            q22: m.q22,
            q12: m.q12,
            q66: m.q66,
            c1111: m.c1111,
            c2222: m.c2222,
            c1122: m.c1122,
            e1_jam: m.e1_jam,
            e2_jam: m.e2_jam,
            gamma_jam: m.gamma_jam,
            kappa_jam: m.kappa_jam,
        };
        let bending = law.bending.as_ref().map(|b| AsymmetricKnitBending {
            d_plus: b.d_plus,
            d_minus: b.d_minus,
            d12: b.d12,
            d66: b.d66,
            kappa0: b.kappa0,
            delta_kappa: b.delta_kappa,
        });
        let hysteresis = law.hysteresis.as_ref().map(|h| KnitHysteresisBank {
            thresholds: h.thresholds.clone(),
            weights: h.weights.clone(),
            huber_eps: h.huber_eps,
        });
        membrane.validate()?;
        if let Some(b) = bending {
            b.validate()?;
        }
        if let Some(h) = &hysteresis {
            h.validate()?;
        }
        Ok(Self {
            membrane,
            bending,
            hysteresis,
        })
    }
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
    pub(crate) piece_fabrics: Option<Vec<Option<Fabric>>>,
    pub(crate) response: Option<CompiledMembrane>,
    pub(crate) piece_responses: Option<Vec<Option<CompiledMembrane>>>,
    pub(crate) knit: Option<KnitMaterial>,
    pub(crate) piece_knits: Option<Vec<Option<KnitMaterial>>>,
    pub(crate) relaxation: Option<CompiledRelaxation>,
    pub(crate) piece_relaxations: Option<Vec<Option<CompiledRelaxation>>>,
    pub(crate) membrane_plasticity: Option<PlasticMembrane>,
    pub(crate) piece_membrane_plasticities: Option<Vec<Option<PlasticMembrane>>>,
    pub(crate) plasticity: Option<PlasticBending>,
    pub(crate) piece_plasticities: Option<Vec<Option<PlasticBending>>>,
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
    let inputs = pieces::resolve(pattern, options)?;
    let permanent = inputs.piece_membrane_plasticities.as_ref().map_or_else(
        || inputs.membrane_plasticity.is_some(),
        |pieces| pieces.iter().any(Option::is_some),
    );
    if permanent && inputs.options.compression != 1.0 {
        return Err(invalid(
            "permanent in-plane set requires compression factor 1; softened Green-strain plasticity is not characterized",
        ));
    }
    if inputs.report["basis"] != "legacy_uncertain"
        && (inputs.options.stick_slip.is_some() || inputs.options.self_stick_slip.is_some())
    {
        return Err(invalid(
            "uniform stick-slip friction cannot override a characterized material record's friction evidence",
        ));
    }
    if inputs.report["basis"] != "legacy_uncertain" && inputs.options.construction.is_active() {
        return Err(invalid(
            "uncalibrated construction mass/bending multipliers cannot override a characterized material record",
        ));
    }
    let active_knits = inputs
        .piece_knits
        .as_deref()
        .unwrap_or_else(|| std::slice::from_ref(&inputs.knit));
    let has_knit_bending = active_knits.iter().flatten().any(|k| k.bending.is_some());
    let has_knit_history = active_knits
        .iter()
        .flatten()
        .any(|k| k.hysteresis.is_some());
    if has_knit_history && inputs.options.integrator != crate::TimeIntegrator::ImplicitEuler {
        return Err(invalid(
            "knit yarn-slip history requires ImplicitEuler; other integrators have no accepted-history contract",
        ));
    }
    if has_knit_bending && inputs.options.damping.rayleigh_bending_s > 0.0 {
        return Err(invalid(
            "Rayleigh bending damping is not characterized for asymmetric physical-curvature knit bending",
        ));
    }
    let has_panel_bending = inputs.piece_fabrics.as_ref().map_or_else(
        || inputs.fabric.panel_bending.is_some(),
        |pieces| {
            pieces
                .iter()
                .any(|f| f.is_some_and(|fab| fab.panel_bending.is_some()))
        },
    );
    if (has_panel_bending || has_knit_bending)
        && inputs.options.bending_model == crate::BendingModel::DiscreteShells
    {
        return Err(invalid(
            "orthotropic panel-bending patches do not implement BendingModel::DiscreteShells",
        )
        .repair("Use BendingModel::Quadratic with orthotropic characterized bending materials, or use an isotropic scalar bending law with DiscreteShells."));
    }
    if permanent && inputs.options.compression_model == crate::CompressionModel::Principal {
        return Err(invalid(
            "membrane plasticity does not implement CompressionModel::Principal",
        )
        .repair("Use CompressionModel::AxisAligned with membrane plasticity, or remove the plastic law to use Principal compression softening."));
    }
    Ok(inputs)
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
            knit: None,
            piece_knits: None,
            relaxation: None,
            piece_relaxations: None,
            membrane_plasticity: None,
            piece_membrane_plasticities: None,
            plasticity: None,
            piece_plasticities: None,
            contact_materials: None,
            mechanics: None,
        });
    };
    let mechanics = record.mechanics_for_contacts(opts.self_contact.is_some())?;
    let retained_liquid = record
        .material()
        .retained_liquid_mass(record.allows_estimates())?;
    let knit_response = record.coupled_knit()?;
    let response = if knit_response.is_some() {
        None
    } else {
        record.membrane_response()?
    };
    let knit = knit_response
        .as_ref()
        .map(KnitMaterial::from_compiled)
        .transpose()?;
    if knit.is_some() && retained_liquid.is_some() {
        return Err(invalid(
            "retained-liquid mechanical binding is not implemented for coupled knit shell materials",
        ));
    }
    let relaxation = record.membrane_relaxation()?;
    let membrane_plasticity = record.membrane_plasticity()?;
    let plasticity = record.bending_plasticity()?;
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
    let mut fabric = Fabric::from_mechanics(mechanics);
    fabric.panel_bending = record.bending_response()?;
    fabric.fitted_domain = ftx_pattern::material_calibration::fitted_domain(record.material())?;
    fabric.validate()?;
    check_grains(pattern, &fabric)?;
    opts.contact.thickness = 0.5 * mechanics.thickness_m;
    opts.contact.mu = mechanics.friction_skin;
    if let Some(p) = &mut opts.self_contact {
        p.thickness = mechanics.thickness_m;
        p.mu = mechanics
            .friction_self
            .ok_or_else(|| invalid("self-contact requires same-sheet friction evidence"))?;
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
    let mut not_simulated = vec![
        "yarn/loop-scale homogenization",
        "plastic set/damage",
        "moisture and heat transport",
        "garment thermal comfort",
        "damage/fire",
        "robotic sewing",
    ];
    let knit_bending = knit.as_ref().and_then(|k| k.bending);
    if knit_bending.is_some() {
        not_simulated.extend([
            "conforming shell discretization",
            "measured seam bending",
            "yarn migration and damage",
            "pore-opening and rib-flattening optics",
        ]);
    } else if fabric.panel_bending.is_none() {
        not_simulated.push("directional bending");
    } else {
        not_simulated.extend([
            "conforming shell discretization",
            "nonlinear curvature response",
            "measured seam bending",
        ]);
        if !fabric.panel_bending.is_some_and(|d| d.uses_normal()) {
            not_simulated.push("finite-stretch shell curvature");
        }
    }
    if plasticity.is_some() {
        not_simulated
            .retain(|s| *s != "plastic set/damage" && *s != "nonlinear curvature response");
        not_simulated.extend([
            "membrane plasticity/damage",
            "seam plasticity",
            "rate/temperature-dependent creasing",
        ]);
    }
    if membrane_plasticity.is_some() {
        not_simulated.retain(|s| *s != "plastic set/damage" && *s != "membrane plasticity/damage");
        not_simulated.extend(["membrane damage/rupture", "plastic-work thermal feedback"]);
        if plasticity.is_none() {
            not_simulated.push("bending plasticity");
        }
    }
    let mut inputs = MaterialInputs {
        fabric,
        options: opts,
        report: json!({
            "basis":"characterized_record", "record":record, "record_sha256":record.sha256(),
            "mechanics":mechanics,
            "model": if knit.is_some() {
                if knit_bending.is_some() {
                    "coupled_knit_green_strain_asymmetric_physical_curvature"
                } else if fabric.panel_bending.is_some() {
                    "coupled_knit_green_strain_tensor_panel_bending"
                } else {
                    "coupled_knit_green_strain_scalar_bending"
                }
            } else {
                match (response.is_some(),fabric.panel_bending.is_some()) {
                    (false,false)=>"orthotropic_green_strain_scalar_bending",
                    (true,false)=>"tabulated_green_strain_scalar_bending",
                    (false,true)=>"orthotropic_green_strain_tensor_panel_bending",
                    (true,true)=>"tabulated_green_strain_tensor_panel_bending"
                }
            },
            "bending_model": if knit_bending.is_some() {
                "asymmetric_knit_physical_curvature"
            } else {
                fabric.panel_bending.map_or("scalar_hinges", |law| law.model())
            },
            "panel_bending_matrix_nm":fabric.panel_bending.map(|p| p.matrix()),
            "bending_scope":"explicit tensor replaces interior source-panel hinges; scalar bending_rigidity remains the separate seam approximation",
            "nonlinear_strain_range": response.as_ref().map(CompiledMembrane::strain_range),
            "fitted_strain_range": fabric.fitted_domain.map(StrainDomain::ranges),
            "relaxation_law_sha256": relaxation.as_ref().map(CompiledRelaxation::fingerprint),
            "membrane_stiffness_basis": "equilibrium; optional Maxwell branches add transient response",
            "contact_policy":"material owns thickness and friction; caller owns numerical tolerances and self-contact enablement",
            "body_offset_m":opts.contact.thickness,
            "self_contact_enabled":opts.self_contact.is_some(),
            "dry_material_resistance_m2_k_w":resistance, "thermal_basis":thermal_basis,
            "thermal_unavailable":thermal_failure,
            "not_simulated":not_simulated,
            "fit_claim":false
        }),
        piece_fabrics: None,
        response,
        piece_responses: None,
        knit,
        piece_knits: None,
        relaxation,
        piece_relaxations: None,
        membrane_plasticity,
        piece_membrane_plasticities: None,
        plasticity,
        piece_plasticities: None,
        contact_materials: None,
        mechanics: Some(mechanics),
    };
    if let Some(law) = &knit_response {
        inputs.report["knit_response"] = json!({
            "model": "coupled_knit_shell/1",
            "law_sha256": law.fingerprint(),
            "evidence": law.evidence(),
            "membrane": law.membrane,
            "bending": law.bending,
            "hysteresis": law.hysteresis,
            "axes": "u follows the explicit grain/wale axis; v is the course axis; shear and twist use engineering components",
            "bending_scope": "physical material curvature with signed material-normal preference; scalar seam hinges remain separate",
            "history_initialization": "pristine zero backstrain; accepted converged implicit-Euler steps only",
            "qualification": "source evidence is preserved; this model selection does not establish physical garment or fit validation"
        });
        inputs.report["membrane_stiffness_basis"] =
            json!("coupled equilibrium Green-strain law; optional additive yarn-slip history");
    }
    if let Some(range) = inputs
        .response
        .as_ref()
        .and_then(CompiledMembrane::area_ratio_range)
    {
        inputs.report["physical_area_ratio_range"] = json!(range);
        inputs.report["area_response_model"] = json!(ftx_pattern::material_response::AREA_MODEL);
    }
    if let Some(mass) = retained_liquid {
        inputs.report["retained_liquid_mass"] = json!({
            "model":ftx_pattern::material_mass::RETAINED_LIQUID_MODEL,
            "mass":mass,
            "basis":"source-qualified fixed co-moving liquid on dry textile reference area; conserved through sewing, draping and motion",
            "not_simulated":["liquid exchange/drainage/evaporation","liquid relative momentum","unqualified wet mechanical changes","liquid thermal storage/transport"]});
    }
    if let Some(law) = &inputs.plasticity {
        inputs.report["bending_plasticity_law_sha256"] = json!(law.fingerprint());
        inputs.report["bending_history_initialization"] =
            json!("pristine_relative_to_authored_preferred_curvature");
    }
    if let Some(law) = &inputs.membrane_plasticity {
        inputs.report["membrane_plasticity_law_sha256"] = json!(law.fingerprint());
        inputs.report["membrane_history_initialization"] = json!("pristine_material_frame");
        inputs.report["membrane_stiffness_basis"] = json!(
            "elastic source tensor with permanent Green-strain set and directional hardening"
        );
        if let Some(identity) = law.parallel_material_sha256() {
            inputs.report["parallel_membrane_network"] = json!({
                "model":ftx_pattern::material_relaxation::parallel::MODEL,
                "material_sha256":identity,
                "strain_basis":"total_physical_green_strain",
                "plastic_flow_stress":"elastoplastic_equilibrium_branch_only",
                "maxwell_stress":"sum_of_branch_stiffness_times_total_minus_own_viscous_strain",
                "same_branch_viscoplasticity":false});
            inputs.report["membrane_stiffness_basis"] = json!(
                "elastic tangent of elastoplastic equilibrium branch; qualified parallel Maxwell branches add transient stress"
            );
        }
    }
    Ok(inputs)
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

    #[test]
    fn resolve_refuses_incompatible_constitutive_options_before_scene_setup() {
        let orthotropic_selection: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-panel-bending.json"
        ))
        .unwrap();
        let nonlinear_record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        let nonlinear_selection = json!({
            "id": nonlinear_record["id"],
            "record": nonlinear_record,
            "allow_estimates": true
        });
        let relaxation_selection: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-relaxation.json"
        ))
        .unwrap();
        let plasticity_selection: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-membrane-plasticity.json"
        ))
        .unwrap();

        let pattern_with_selection = |selection: &Value| {
            Pattern::new(json!({
                "schema": "fabric-pattern/1",
                "units": "mm",
                "fabric": { "material": selection },
                "pieces": [{
                    "id": "panel",
                    "cut": {"qty": 1},
                    "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 100], "d": [0, 100]},
                    "edges": [
                        {"id": "bottom", "from": "a", "to": "b"},
                        {"id": "right", "from": "b", "to": "c"},
                        {"id": "top", "from": "c", "to": "d"},
                        {"id": "left", "from": "d", "to": "a"}
                    ],
                    "grain": [[50, 10], [50, 90]],
                    "placement": {"part": "torso"}
                }],
                "seams": []
            }))
        };

        let ortho_pat = pattern_with_selection(&orthotropic_selection);
        assert!(resolve(&ortho_pat, &DrapeOptions::default()).is_ok());

        let ds_opts = DrapeOptions {
            bending_model: crate::BendingModel::DiscreteShells,
            ..DrapeOptions::default()
        };
        let ds_err = resolve(&ortho_pat, &ds_opts)
            .err()
            .expect("DiscreteShells with orthotropic bending must refuse");
        assert_eq!(ds_err.code, RefusalCode::InvalidDocument);
        assert!(ds_err.message.contains("DiscreteShells"), "{ds_err}");

        let mut rb_opts = DrapeOptions::default();
        rb_opts.damping.rayleigh_bending_s = 1e-4;
        assert!(
            resolve(&ortho_pat, &rb_opts).is_ok(),
            "objective Rayleigh bending damping is supported on orthotropic panel-bending patches"
        );

        let pr_default_opts = DrapeOptions {
            compression_model: crate::CompressionModel::Principal,
            ..DrapeOptions::default()
        };
        for (label, sel) in [
            ("nonlinear", &nonlinear_selection),
            ("relaxation", &relaxation_selection),
        ] {
            let pat = pattern_with_selection(sel);
            assert!(
                resolve(&pat, &pr_default_opts).is_ok(),
                "{label} must remain admitted under CompressionModel::Principal"
            );
        }

        let pl_pat = pattern_with_selection(&plasticity_selection);
        let pl_base = DrapeOptions {
            compression: 1.0,
            ..DrapeOptions::default()
        };
        assert!(resolve(&pl_pat, &pl_base).is_ok());
        let pr_opts = DrapeOptions {
            compression_model: crate::CompressionModel::Principal,
            ..pl_base
        };
        let pr_err = resolve(&pl_pat, &pr_opts)
            .err()
            .expect("Principal compression with membrane plasticity must refuse");
        assert_eq!(pr_err.code, RefusalCode::InvalidDocument);
        assert!(pr_err.message.contains("Principal"), "{pr_err}");

        // Per-piece active characterized sheet must also fail fast; excluded (`sim: false`) piece must not.
        let mut linear_selection = orthotropic_selection.clone();
        linear_selection["id"] = json!("synthetic-linear-scalar");
        let rec = linear_selection["record"].as_object_mut().unwrap();
        rec["id"] = json!("synthetic-linear-scalar");
        rec.remove("structure");
        let props = rec["properties"].as_object_mut().unwrap();
        for k in [
            "bending_warp",
            "bending_weft",
            "bending_coupling",
            "bending_twist",
        ] {
            props.remove(k);
        }
        let per_piece_pattern = |first_selection: Option<&Value>,
                                 second_sim: bool,
                                 second_selection: &Value| {
            let mut first = json!({
                "id": "active_panel",
                "cut": {"qty": 1},
                "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 100], "d": [0, 100]},
                "edges": [
                    {"id": "bottom", "from": "a", "to": "b"},
                    {"id": "right", "from": "b", "to": "c"},
                    {"id": "top", "from": "c", "to": "d"},
                    {"id": "left", "from": "d", "to": "a"}
                ],
                "grain": [[50, 10], [50, 90]],
                "placement": {"part": "torso"}
            });
            if let Some(sel) = first_selection {
                first["fabric"] = json!({ "material": sel });
            }
            Pattern::new(json!({
                "schema": "fabric-pattern/1",
                "units": "mm",
                "pieces": [
                    first,
                    {
                        "id": "second_panel",
                        "sim": second_sim,
                        "cut": {"qty": 1},
                        "fabric": { "material": second_selection },
                        "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 100], "d": [0, 100]},
                        "edges": [
                            {"id": "bottom", "from": "a", "to": "b"},
                            {"id": "right", "from": "b", "to": "c"},
                            {"id": "top", "from": "c", "to": "d"},
                            {"id": "left", "from": "d", "to": "a"}
                        ],
                        "grain": [[50, 10], [50, 90]],
                        "placement": {"part": "torso"}
                    }
                ],
                "seams": []
            }))
        };
        let active_ortho = per_piece_pattern(Some(&linear_selection), true, &orthotropic_selection);
        let active_ds_err = resolve(&active_ortho, &ds_opts)
            .err()
            .expect("active per-piece DiscreteShells must refuse");
        assert_eq!(active_ds_err.code, RefusalCode::InvalidDocument);
        assert!(
            active_ds_err.message.contains("DiscreteShells"),
            "{active_ds_err}"
        );
        assert!(resolve(&active_ortho, &rb_opts).is_ok());
        let active_pl = per_piece_pattern(Some(&linear_selection), true, &plasticity_selection);
        let active_pr_err = resolve(&active_pl, &pr_opts)
            .err()
            .expect("active per-piece Principal compression with plasticity must refuse");
        assert_eq!(active_pr_err.code, RefusalCode::InvalidDocument);
        assert!(
            active_pr_err.message.contains("Principal"),
            "{active_pr_err}"
        );

        let excluded_ortho = per_piece_pattern(None, false, &orthotropic_selection);
        assert!(resolve(&excluded_ortho, &ds_opts).is_ok());
        let excluded_pl = per_piece_pattern(None, false, &plasticity_selection);
        assert!(resolve(&excluded_pl, &pr_opts).is_ok());
    }
}

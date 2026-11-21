//! Bounded, deterministic inverse strip experiments. Every objective evaluation solves the
//! actual gripped membrane geometry. CHECK observations never choose coefficients or minima.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec3, linalg::lstsq, receipt::sha256_hex};
use ftx_pattern::{
    material_calibration::{Partition, fitted_domain},
    material_catalog::Selection,
    material_response::{CompiledMembrane, StrainDomain},
    materials::{Evidence, Form, Material, Quantity, Source},
    textile_import,
};
use serde::{Deserialize, Serialize};

use super::experiment::{
    Controls, Fixture, SCOPE, State, invalid, solve_law, solve_law_with_solver,
};
use super::measurement::{self, Extensometer, ExtensometerPrediction};
use crate::{Membrane, Solver};

/// Caller-declared nonlinear shapes identified through the same bounded strip experiment.
pub mod family;

/// Optional fixed or jointly identified physical-area coefficients in the same experiment.
pub mod area;

/// Independently fitted signed tangent intervals through the same strip experiment.
pub mod nonlinear;
use nonlinear::Nonlinear;

const KEYS: [&str; 4] = [
    "warp_stiffness",
    "weft_stiffness",
    "poisson_warp_weft",
    "shear_stiffness",
];

/// An optional independently observed mid-gauge transverse width.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Width {
    /// Distance between opposite side material points at the reference mid-gauge, m.
    pub value_m: f64,
    /// Positive residual weighting scale in metres, not a confidence interval.
    pub scale_m: f64,
}

/// Total force observation. Grip extension is a boundary displacement, never a local strain.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Globally unique acquisition-row ID, retained verbatim.
    pub id: String,
    /// Change of grip separation divided by the undeformed gauge length.
    pub extension_fraction: f64,
    /// Total measured tensile force in N. Signed instrument noise is retained.
    pub force_n: f64,
    /// Positive force residual weighting scale, N, not inferred uncertainty.
    pub scale_n: f64,
    /// Optional transverse observation; missing width never means no contraction.
    #[serde(default)]
    pub mid_width: Option<Width>,
    /// Additional explicitly located material-point channels, at most eight per observed state.
    /// Omission preserves the original midpoint-only observation and serialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensometers: Vec<Extensometer>,
}

/// A matched specimen/cut/fixture acquisition under one method and conditioning state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Unique strip/cut acquisition identifier.
    pub id: String,
    /// The actual fixture and material-axis orientation.
    pub fixture: Fixture,
    /// Source key in the input material, naming these acquisition observations.
    pub source: String,
    /// Measured, published or explicitly admitted estimated observations.
    pub evidence: Evidence,
    /// Entire strip acquisition participates in FIT or is independently held out as CHECK.
    pub partition: Partition,
    /// Independent quasistatic elastic loading states; no unloading-history inference.
    pub observations: Vec<Observation>,
}

/// Jacobian differentiation method used during FIT search and excitation certification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityMode {
    /// Central finite differences via additional forward equilibrium solves (default).
    #[default]
    FiniteDifference,
    /// Exact implicit-function-theorem (IFT) adjoint pullback reusing the converged static LDLᵀ
    /// factor. Supported for homogeneous scalar orthotropic identification only; refuses
    /// fail-closed for nonlinear, declared-shape or area-response models.
    Adjoint,
}

impl SensitivityMode {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    const fn is_default(&self) -> bool {
        matches!(self, Self::FiniteDifference)
    }
}

const fn is_zero(value: &usize) -> bool {
    *value == 0
}

/// Numerical and model-acceptance policy, separate from source evidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Maximum scaled RMS residual, independently for FIT and CHECK.
    pub max_rms_scaled: f64,
    /// Maximum absolute scaled residual, independently for FIT and CHECK.
    pub max_peak_scaled: f64,
    /// Maximum coarse-to-doubled-mesh change divided by each observation's scale.
    pub max_mesh_scaled: f64,
    /// Minimum column-normalized sensitivity Gram pivot for every fitted parameter.
    pub min_excitation_pivot: f64,
    /// Per-start Levenberg-Marquardt iteration cap, 1..80.
    pub max_iterations: usize,
    /// Aggregate independent forward solves, including rejected trials, sensitivities and checks.
    pub max_forward_solves: usize,
    /// Parameter-sensitivity differentiation method; omitted when default `finite_difference`.
    #[serde(default, skip_serializing_if = "SensitivityMode::is_default")]
    pub sensitivity: SensitivityMode,
}

/// Reproducible limited-model identification. Published coefficients remain model estimates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// `ftx-strip-calibration/1`.
    pub schema: String,
    /// Matched specimen and all acquisition sources; unrelated properties are preserved.
    pub material: Material,
    /// Actual loading, gauge, grip and conditioning protocol; disclose absent source conditions.
    pub method: String,
    /// Explicit same-specimen test conditions; no guessed rate or humidity.
    pub conditions: BTreeMap<String, String>,
    /// Caller-supplied acquisition/analysis date for deterministic receipts.
    pub fitted_on: String,
    /// Complete strip acquisitions (1..16, at most 64 observed states in total).
    pub experiments: Vec<Experiment>,
    /// Scalar bounds are [warp N/m, weft N/m, major Poisson ratio, shear N/m]. With
    /// response_family, these are [warp scale, weft scale, shear scale, coupling N/m].
    /// The entire box must be stable; no bound is silently repaired.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_bounds: Vec<[f64; 2]>,
    /// Optional declared piecewise-linear shapes. Their knots and relative shape are fixed;
    /// only three dimensionless amplitudes and signed normal coupling are identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_family: Option<family::Family>,
    /// Optional independent signed tangent intervals, mutually exclusive with response_family.
    /// Its own tangent/coupling bounds replace the omitted scalar parameter_bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonlinear: Option<Nonlinear>,
    /// Explicitly preserve a qualified area law or identify it from declared J support.
    /// Omission never permits an existing area law to be discarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_response: Option<area::Area>,
    /// Acknowledge the planar ideal-grip elastic model; this does not assert it fits a real knit.
    pub admit_planar_elastic_model: bool,
    /// Explicit permission for estimated input observations, independently of fitted outputs.
    #[serde(default)]
    pub allow_estimated_observations: bool,
    /// Explicit permission to replace existing scalar or nonlinear equilibrium evidence.
    #[serde(default)]
    pub replace_existing: bool,
    /// Explicit admission of the resulting estimated coefficients in the returned frozen selection.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Production solver tolerances.
    #[serde(default)]
    pub controls: Controls,
    /// Declared model, numerical-refinement and work limits.
    pub policy: Policy,
}

/// Residual summary with no assumed probability distribution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quality {
    /// Measured scalar channels, not number of physical specimens.
    pub components: usize,
    /// Root-mean-square residual divided by the explicit measurement scales.
    pub rms_scaled: f64,
    /// Largest absolute scaled residual.
    pub peak_scaled: f64,
}

/// Every attempt is retained; an exhausted search is never promoted to a characterized material.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Start {
    /// Uniform normalized starting coordinate in the declared parameter box.
    pub initial_fraction: f64,
    /// Final physical coefficients in the documented order.
    pub parameters: Vec<f64>,
    /// Squared scaled FIT residual at this start's final point.
    pub fit_sse: f64,
    /// Actual first-order objective stationarity reached before the iteration limit.
    pub converged: bool,
    /// Objective iterations attempted.
    pub iterations: usize,
}

/// One full predicted experiment state, preserving its observation and partition identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    /// Acquisition ID.
    pub experiment: String,
    /// Exact acquisition-row ID.
    pub observation: String,
    /// FIT or independently held-out CHECK.
    pub partition: Partition,
    /// Production equilibrium on the doubled mesh.
    pub state: State,
    /// Predicted minus measured total force, N.
    pub force_residual_n: f64,
    /// Predicted minus measured width, m, only when measured.
    pub width_residual_m: Option<f64>,
    /// Actual spatial observations; omitted when the input uses only legacy force/midpoint channels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensometers: Vec<ExtensometerPrediction>,
}

/// Bounded numerical recovery during FIT exploration; every attempted solve remains charged.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchRecovery {
    /// Exploratory equilibria refused by the source domain or numerical convergence gate.
    pub refused_forward_solves: usize,
    /// Halvings of a finite-difference interval after at least one refused probe.
    pub sensitivity_halvings: usize,
    /// Sensitivity columns using the admitted center and one admissible neighboring state.
    pub one_sided_sensitivity_columns: usize,
}

/// Reviewable candidate, diagnostics and conditional material publication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    /// `ftx-strip-calibration-report/1`.
    pub schema: String,
    /// Complete source observations, bounds and acceptance policy for exact replay.
    pub input: Input,
    /// SHA-256 identity of the serialized input; not evidence of measurement quality.
    pub input_sha256: String,
    /// Best FIT candidate, whether or not accepted.
    pub parameters: Vec<f64>,
    /// Physical meaning of nonlinear family parameters; absent for legacy scalar reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_descriptors: Option<Vec<family::ParameterDescriptor>>,
    /// Independent interval or joint area-family bounds in parameter order; legacy bounds stay in input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_bounds: Vec<[f64; 2]>,
    /// Whether every publication gate passed.
    pub accepted: bool,
    /// Specific remaining numerical, identifiability or model-adequacy gates.
    pub reasons: Vec<String>,
    /// Model estimate only when accepted; unknown bending/contact/thermal data remain absent.
    pub material: Option<Material>,
    /// Normal source-locked selection, only when accepted, with the caller's estimate policy.
    pub selection: Option<Selection>,
    /// Exact Rust serialization of the frozen selection; source hashes must survive browser replay.
    pub selection_json: Option<String>,
    /// Three independently initialized deterministic bounded local searches.
    pub starts: Vec<Start>,
    /// Column-normalized local FIT sensitivity pivot; not global uniqueness or confidence.
    pub excitation_pivot: f64,
    /// All attempted equilibrium solves across the complete operation.
    pub forward_solves: usize,
    /// Total IFT adjoint linear solves executed (omitted when zero under default finite differences).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub adjoint_solves: usize,
    /// Present only when a numerical FIT probe was refused. These counts do not relax any
    /// stationarity, excitation, held-out, source-domain or publication requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_recovery: Option<SearchRecovery>,
    /// FIT quality re-evaluated with the chosen coefficients on the doubled mesh.
    pub fit: Quality,
    /// CHECK quality, evaluated only after coefficients are fixed, on the doubled mesh.
    pub check: Option<Quality>,
    /// Largest observed doubled-mesh prediction change in measurement-scale units.
    pub mesh_change_peak_scaled: f64,
    /// Every declared coarse/refined experiment re-solved with the final FIT domain enforced.
    /// False when an earlier gate prevented replay or a domain-limited replay failed.
    pub domain_replay_verified: bool,
    /// Local FIT element strain envelope including the explicitly simulated undeformed state.
    pub fitted_green_strain_range: [[f64; 2]; 3],
    /// Actual coarse/refined FIT element J envelope including the undeformed reference.
    /// Absent for legacy separable models; CHECK observations cannot enlarge this support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fitted_area_ratio_range: Option<[f64; 2]>,
    /// Prediction for every input observation, in stable input order.
    pub predictions: Vec<Prediction>,
    /// Explicit modeling limits; no empirical garment or global-optimum claim.
    pub model_scope: String,
}

struct Budget<'a> {
    used: usize,
    adjoint_solves: usize,
    limit: usize,
    // Replacing coefficients does not erase the original specimen's characterized support.
    inherited_domain: Option<StrainDomain>,
    family: Option<family::Prepared>,
    input: &'a Input,
    recovery: SearchRecovery,
}

// Only execution of a prevalidated trial membrane can be recoverable. Errors from source
// compilation, input validation, measurement arithmetic or work admission always stay fatal.
enum ForwardFailure {
    Fatal(Refusal),
    Trial(Refusal),
}

impl ForwardFailure {
    fn refusal(self) -> Refusal {
        match self {
            Self::Fatal(refusal) | Self::Trial(refusal) => refusal,
        }
    }
}

impl From<Refusal> for ForwardFailure {
    fn from(refusal: Refusal) -> Self {
        Self::Fatal(refusal)
    }
}

fn intersect(a: StrainDomain, b: StrainDomain) -> Result<StrainDomain, Refusal> {
    let a = a.ranges();
    let b = b.ranges();
    StrainDomain::new(std::array::from_fn(|i| {
        [a[i][0].max(b[i][0]), a[i][1].min(b[i][1])]
    }))
}

impl Budget<'_> {
    fn admit(&mut self) -> Result<(), Refusal> {
        if self.used >= self.limit {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "strip calibration exhausted its complete forward-solve budget; no material is published",
            ));
        }
        self.used += 1;
        Ok(())
    }

    fn solve(
        &mut self,
        fixture: &Fixture,
        parameters: &[f64],
        extension: f64,
        controls: &Controls,
    ) -> Result<State, Refusal> {
        self.trial(fixture, parameters, extension, controls)
            .map_err(ForwardFailure::refusal)
    }

    fn trial(
        &mut self,
        fixture: &Fixture,
        parameters: &[f64],
        extension: f64,
        controls: &Controls,
    ) -> Result<State, ForwardFailure> {
        self.trial_with_solver(fixture, parameters, extension, controls)
            .map(|(state, _)| state)
    }

    fn trial_with_solver(
        &mut self,
        fixture: &Fixture,
        parameters: &[f64],
        extension: f64,
        controls: &Controls,
    ) -> Result<(State, Solver), ForwardFailure> {
        self.admit()?;
        let base = base_parameters(self.input, parameters);
        let mut response = if let Some(family) = &self.family {
            Some(family.response(four(base)?))
        } else if let Some(model) = &self.input.nonlinear {
            // Trial curves remain unpublished estimates; CHECK never supplies this temporary key.
            let source = self
                .input
                .experiments
                .iter()
                .find(|e| e.partition == Partition::Fit)
                .map(|e| &e.source)
                .ok_or_else(|| invalid("strip acquisition source is missing"))?;
            Some(model.response(base, None, source, &self.input.conditions)?)
        } else if let Some(mode) = &self.input.area_response {
            // Scalar coordinates supply the base Q exactly; area energy remains a separate term.
            let source = self
                .input
                .experiments
                .iter()
                .find(|e| e.partition == Partition::Fit)
                .map(|e| &e.source)
                .ok_or_else(|| invalid("strip acquisition source is missing"))?;
            Some(area::scalar_response(
                four(base)?,
                mode.scalar_range().ok_or_else(|| {
                    invalid("scalar area base requires explicit Green-strain support")
                })?,
                source,
                &self.input.conditions,
            )?)
        } else {
            None
        };
        if let (Some(mode), Some(response)) = (&self.input.area_response, &mut response) {
            // The family candidate coupling is an unpublished coordinate, not measured evidence.
            response.coupling.conditions = self.input.conditions.clone();
            let source = response.coupling.source.clone();
            mode.attach(
                response,
                area_template(self.input),
                &parameters[base.len()..],
                &source,
                &self.input.conditions,
                None,
            )?;
        }
        let response = response
            .as_ref()
            .map(|response| {
                let sources = self
                    .family
                    .as_ref()
                    .map_or(&self.input.material.sources, |f| f.sources());
                response.compile(sources, true)
            })
            .transpose()?;
        let [warp, weft, nu, shear] = match &response {
            Some(response) => response.reference_moduli()?,
            None => four(parameters)?,
        };
        let membrane = Membrane::orthotropic(warp, weft, nu, shear, 1.0);
        // In particular, an unrepresentable tensor must not become an admissibility retry.
        membrane.validate()?;
        let result = solve_law_with_solver(
            fixture,
            membrane,
            response.as_ref(),
            self.inherited_domain,
            extension,
            controls,
        );
        result.map_err(|mut refusal| {
            if self.input.area_response.is_some() {
                refusal.message = format!(
                    "area-strip solve {} at extension {extension}, cells {:?}, warp axis {:?}: {}",
                    self.used, fixture.cells, fixture.warp_axis, refusal.message,
                );
            }
            match refusal.code {
                RefusalCode::OutOfRange | RefusalCode::NotConverged => {
                    ForwardFailure::Trial(refusal)
                }
                _ => ForwardFailure::Fatal(refusal),
            }
        })
    }

    fn within_inherited_domain(&self, domain: StrainDomain) -> Result<StrainDomain, Refusal> {
        let Some(inherited) = self.inherited_domain else {
            return Ok(domain);
        };
        intersect(inherited, domain)
    }

    fn replay(
        &mut self,
        fixture: &Fixture,
        parameters: &[f64],
        domain: StrainDomain,
        response: Option<&CompiledMembrane>,
        extension: f64,
        controls: &Controls,
    ) -> Result<State, Refusal> {
        self.admit()?;
        let [warp, weft, nu, shear] = match response {
            Some(response) => response.reference_moduli()?,
            None => four(parameters)?,
        };
        solve_law(
            fixture,
            Membrane::orthotropic(warp, weft, nu, shear, 1.0),
            response,
            Some(self.within_inherited_domain(domain)?),
            extension,
            controls,
        )
    }
}

fn four(parameters: &[f64]) -> Result<[f64; 4], Refusal> {
    parameters
        .try_into()
        .map_err(|_| invalid("scalar/declared-shape strip fit requires four parameters"))
}

fn bounds(input: &Input) -> Vec<[f64; 2]> {
    let mut bounds = input
        .nonlinear
        .as_ref()
        .map_or_else(|| input.parameter_bounds.clone(), Nonlinear::bounds);
    if let Some(area) = &input.area_response {
        bounds.extend_from_slice(area.bounds());
    }
    bounds
}

fn base_parameters<'a>(input: &Input, parameters: &'a [f64]) -> &'a [f64] {
    let extra = input
        .area_response
        .as_ref()
        .map_or(0, |area| area.bounds().len());
    &parameters[..parameters.len() - extra]
}

fn area_template(input: &Input) -> Option<&ftx_pattern::material_response::AreaResponse> {
    input
        .material
        .membrane_response
        .as_ref()
        .and_then(|r| r.area_response.as_ref())
}

fn physical<const N: usize>(input: &Input, p: [f64; N]) -> [f64; N] {
    let bounds = bounds(input);
    std::array::from_fn(|i| bounds[i][0] + p[i] * (bounds[i][1] - bounds[i][0]))
}

fn validate(input: &Input) -> Result<(), Refusal> {
    input.material.validate()?;
    input.controls.validate()?;
    if input
        .material
        .membrane_response
        .as_ref()
        .is_some_and(|response| response.area_response.is_some())
        && input.area_response.is_none()
    {
        return Err(invalid(
            "strip identification currently fits separable membrane laws and cannot identify or discard an existing area response; use an area-capable characterization model",
        ));
    }
    if input.response_family.is_some() && input.nonlinear.is_some() {
        return Err(invalid(
            "response_family and independent nonlinear intervals are mutually exclusive",
        ));
    }
    if let Some(mode) = &input.area_response {
        mode.validate_base(input.response_family.is_none() && input.nonlinear.is_none())?;
        let check_sources: Vec<_> = input
            .experiments
            .iter()
            .filter(|e| e.partition == Partition::Check)
            .filter_map(|e| {
                input
                    .material
                    .sources
                    .get(&e.source)
                    .map(|s| s.reference.as_str())
            })
            .collect();
        mode.validate(
            area_template(input),
            &input.conditions,
            &input.material.sources,
            &check_sources,
        )?;
    }
    let policy = &input.policy;
    if input.schema != "ftx-strip-calibration/1"
        || !input.admit_planar_elastic_model
        || input.method.trim().is_empty()
        || input.fitted_on.trim().is_empty()
        || matches!(input.material.form, Form::Fiber | Form::Yarn | Form::Fill)
        || input.experiments.is_empty()
        || input.experiments.len() > 16
        || (input.material.membrane_response.is_some()
            && input.response_family.is_none()
            && input.nonlinear.is_none()
            && input.area_response.is_none())
        || input.material.membrane_relaxation.is_some()
        || input
            .material
            .structure
            .contains_key("membrane_plasticity_model")
    {
        return Err(invalid(
            "strip calibration needs its schema, explicit planar elastic model consent, sheet specimen, protocol/date and 1..16 experiments; history laws cannot be erased and nonlinear replacement needs a declared response family",
        ));
    }
    if !input.replace_existing
        && (input.material.membrane_response.is_some()
            || KEYS
                .iter()
                .any(|key| input.material.properties.contains_key(*key)))
    {
        return Err(invalid(
            "strip calibration would replace existing stiffness observations; explicit replace_existing is required",
        ));
    }
    if input
        .experiments
        .iter()
        .map(|experiment| experiment.observations.len())
        .sum::<usize>()
        > 64
    {
        return Err(invalid(
            "strip calibration admits at most 64 observed states",
        ));
    }
    if [
        policy.max_rms_scaled,
        policy.max_peak_scaled,
        policy.max_mesh_scaled,
        policy.min_excitation_pivot,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || policy.min_excitation_pivot >= 1.0
        || !(1..=80).contains(&policy.max_iterations)
        || !(1..=32_768).contains(&policy.max_forward_solves)
    {
        return Err(invalid(
            "invalid strip residual, refinement, sensitivity or bounded-work policy",
        ));
    }
    if policy.sensitivity == SensitivityMode::Adjoint
        && (input.response_family.is_some()
            || input.nonlinear.is_some()
            || input.area_response.is_some())
    {
        return Err(invalid(
            "adjoint strip sensitivity supports homogeneous scalar orthotropic identification only; nonlinear, response_family and area_response models require finite_difference sensitivity",
        ));
    }
    if let Some(model) = &input.nonlinear {
        if !input.parameter_bounds.is_empty() {
            return Err(invalid(
                "nonlinear strip identification uses tangent/coupling bounds; omit scalar parameter_bounds",
            ));
        }
        model.validate()?;
        if input.conditions.len() > 60 {
            return Err(invalid(
                "nonlinear strip conditions leave room for fitted-domain and model metadata",
            ));
        }
    } else {
        if input.parameter_bounds.len() != 4 {
            return Err(invalid(
                "scalar/declared-shape strip identification needs exactly four parameter bounds",
            ));
        }
        for (i, &[lo, hi]) in input.parameter_bounds.iter().enumerate() {
            if !lo.is_finite()
                || !hi.is_finite()
                || hi <= lo
                || !((hi - lo).is_finite())
                || (if input.response_family.is_some() {
                    i < 3
                } else {
                    i != 2
                } && lo <= 0.0)
            {
                return Err(invalid(
                    "strip parameter bounds must be finite, nonempty and positive for all stiffnesses",
                ));
            }
        }
        let nu = input.parameter_bounds[2][0]
            .abs()
            .max(input.parameter_bounds[2][1].abs());
        if input.response_family.is_none()
            && nu * nu * input.parameter_bounds[1][1] / input.parameter_bounds[0][0] >= 1.0
        {
            return Err(invalid(
                "the entire declared orthotropic parameter box must be positive-definite",
            ));
        }
    }
    let mut ids = BTreeSet::new();
    let mut acquisition_ids = BTreeSet::new();
    let mut fit_channels = 0;
    for experiment in &input.experiments {
        experiment.fixture.validate()?;
        if experiment.fixture.cells.iter().any(|n| *n > 16)
            || experiment.fixture.cells.iter().product::<usize>() > 128
        {
            return Err(invalid(
                "inverse strip meshes must permit doubling within the 512-cell forward limit",
            ));
        }
        if experiment.id.trim().is_empty()
            || !acquisition_ids.insert(&experiment.id)
            || !input.material.sources.contains_key(&experiment.source)
            || experiment.observations.is_empty()
            || (experiment.evidence == Evidence::Estimate && !input.allow_estimated_observations)
        {
            return Err(invalid(
                "strip acquisition needs a unique ID, retained source, observations and admitted evidence",
            ));
        }
        let mut extensions = BTreeSet::new();
        for point in &experiment.observations {
            let extension_bits = if point.extension_fraction == 0.0 {
                0.0_f64.to_bits()
            } else {
                point.extension_fraction.to_bits()
            };
            if point.id.trim().is_empty()
                || !ids.insert(&point.id)
                || !point.extension_fraction.is_finite()
                || !(0.0..=2.0).contains(&point.extension_fraction)
                || !extensions.insert(extension_bits)
                || !point.force_n.is_finite()
                || !point.scale_n.is_finite()
                || point.scale_n <= 0.0
                || point.mid_width.as_ref().is_some_and(|w| {
                    !w.value_m.is_finite()
                        || w.value_m <= 0.0
                        || !w.scale_m.is_finite()
                        || w.scale_m <= 0.0
                })
            {
                return Err(invalid(
                    "strip observation needs a unique ID/extension, finite force and positive residual scales/observed width",
                ));
            }
            if point.extensometers.len() > 8 {
                return Err(invalid(
                    "strip observation supports at most eight declared extensometers",
                ));
            }
            let mut channel_ids = BTreeSet::new();
            for channel in &point.extensometers {
                channel.operator.validate()?;
                if channel.id.trim().is_empty()
                    || !channel_ids.insert(&channel.id)
                    || !channel.value_m.is_finite()
                    || (channel.operator.is_chord() && channel.value_m <= 0.0)
                    || !channel.scale_m.is_finite()
                    || channel.scale_m <= 0.0
                {
                    return Err(invalid(
                        "strip extensometers need unique nonempty channel IDs, finite readings, positive chord lengths and positive residual scales",
                    ));
                }
            }
            if experiment.partition == Partition::Fit {
                fit_channels +=
                    1 + usize::from(point.mid_width.is_some()) + point.extensometers.len();
            }
        }
    }
    if fit_channels < bounds(input).len() {
        return Err(invalid(
            "strip calibration requires at least one measured FIT channel per fitted coefficient",
        ));
    }
    for fit in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for check in input
            .experiments
            .iter()
            .filter(|e| e.partition == Partition::Check)
        {
            let fit_source = &input.material.sources[&fit.source].reference;
            let check_source = &input.material.sources[&check.source].reference;
            // Reversing both material axes leaves an orthotropic sheet and this acquisition
            // unchanged; a sign flip cannot create independent validation evidence.
            let same_direction = fit.fixture.warp_axis == check.fixture.warp_axis
                || fit.fixture.warp_axis == check.fixture.warp_axis.map(|v| -v);
            if fit_source == check_source
                && fit.fixture.gauge_length_m == check.fixture.gauge_length_m
                && fit.fixture.width_m == check.fixture.width_m
                && same_direction
            {
                return Err(invalid(
                    "FIT and CHECK reuse the same source acquisition, fixture and direction; renamed rows or changed meshes do not establish independent held-out evidence",
                ));
            }
        }
    }
    Ok(())
}

struct Evaluation {
    residuals: Vec<f64>,
    sse: f64,
    adjoint_states: Vec<(State, Solver)>,
}

fn observe_area(range: &mut Option<[f64; 2]>, state: &State) {
    if let (Some(range), Some(actual)) = (range, state.area_ratio_range) {
        range[0] = range[0].min(actual[0]);
        range[1] = range[1].max(actual[1]);
    }
}

fn extensometer_predictions(
    observation: &Observation,
    coarse: (&Fixture, &State),
    refined: (&Fixture, &State),
    errors: &mut Vec<f64>,
    mesh_peak: &mut f64,
) -> Result<Vec<ExtensometerPrediction>, Refusal> {
    observation
        .extensometers
        .iter()
        .map(|channel| {
            let coarse_m = measurement::evaluate(&channel.operator, coarse.0, coarse.1)?;
            let predicted_m = measurement::evaluate(&channel.operator, refined.0, refined.1)?;
            let residual_m = predicted_m - channel.value_m;
            errors.push(residual_m / channel.scale_m);
            *mesh_peak = mesh_peak.max((predicted_m - coarse_m).abs() / channel.scale_m);
            Ok(ExtensometerPrediction {
                id: channel.id.clone(),
                operator: channel.operator.clone(),
                predicted_m,
                residual_m,
            })
        })
        .collect()
}

fn evaluate<const N: usize>(
    input: &Input,
    p: [f64; N],
    budget: &mut Budget<'_>,
) -> Result<Evaluation, ForwardFailure> {
    let mut residuals = Vec::new();
    let keep_solvers = input.policy.sensitivity == SensitivityMode::Adjoint;
    let mut adjoint_states = Vec::new();
    let parameters = physical(input, p);
    for experiment in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        for point in &experiment.observations {
            let (state, solver) = if keep_solvers {
                match budget.trial_with_solver(
                    &experiment.fixture,
                    &parameters,
                    point.extension_fraction,
                    &input.controls,
                ) {
                    Ok((state, solver)) => (state, Some(solver)),
                    Err(failure @ ForwardFailure::Trial(_)) => {
                        budget.recovery.refused_forward_solves += 1;
                        return Err(failure);
                    }
                    Err(failure) => return Err(failure),
                }
            } else {
                match budget.trial(
                    &experiment.fixture,
                    &parameters,
                    point.extension_fraction,
                    &input.controls,
                ) {
                    Ok(state) => (state, None),
                    Err(failure @ ForwardFailure::Trial(_)) => {
                        budget.recovery.refused_forward_solves += 1;
                        return Err(failure);
                    }
                    Err(failure) => return Err(failure),
                }
            };
            residuals.push((state.force_n - point.force_n) / point.scale_n);
            if let Some(width) = &point.mid_width {
                residuals.push((state.mid_width_m - width.value_m) / width.scale_m);
            }
            for channel in &point.extensometers {
                let predicted =
                    measurement::evaluate(&channel.operator, &experiment.fixture, &state)?;
                residuals.push((predicted - channel.value_m) / channel.scale_m);
            }
            if let Some(solver) = solver {
                adjoint_states.push((state, solver));
            }
        }
    }
    let sse = residuals.iter().map(|v| v * v).sum::<f64>();
    if !sse.is_finite() {
        return Err(invalid("strip objective overflow").into());
    }
    Ok(Evaluation {
        residuals,
        sse,
        adjoint_states,
    })
}

fn adjoint_jacobian<const N: usize>(
    input: &Input,
    p: [f64; N],
    budget: &mut Budget<'_>,
    eval: &mut Evaluation,
) -> Result<Vec<[f64; N]>, Refusal> {
    if N != 4
        || input.response_family.is_some()
        || input.nonlinear.is_some()
        || input.area_response.is_some()
    {
        return Err(invalid(
            "adjoint strip Jacobian requires homogeneous scalar orthotropic identification",
        ));
    }
    let phys = physical(input, p);
    let [e1, e2, nu12, g12] = four(&phys)?;
    let param_bounds = bounds(input);
    let widths = [
        param_bounds[0][1] - param_bounds[0][0],
        param_bounds[1][1] - param_bounds[1][0],
        param_bounds[2][1] - param_bounds[2][0],
        param_bounds[3][1] - param_bounds[3][0],
    ];
    let mut rows = Vec::with_capacity(eval.residuals.len());
    let mut solved = eval.adjoint_states.iter_mut();
    let scaled_row = |dq: [f64; 4], scale: f64| -> Result<[f64; N], Refusal> {
        let d_ortho = crate::adjoint::orthotropic_chain_rule(e1, e2, nu12, g12, dq)?;
        let row = std::array::from_fn(|k| d_ortho[k] * widths[k] / scale);
        if row.iter().any(|v| !v.is_finite()) {
            return Err(invalid("strip adjoint sensitivity overflow"));
        }
        Ok(row)
    };
    for experiment in input
        .experiments
        .iter()
        .filter(|e| e.partition == Partition::Fit)
    {
        let [nx, ny] = experiment.fixture.cells;
        let n_verts = (nx + 1) * (ny + 1);
        let mut upper_weights = vec![Vec3::ZERO; n_verts];
        for (i, w) in upper_weights.iter_mut().enumerate() {
            if i / (nx + 1) == ny {
                *w = Vec3::new(0.0, 1.0, 0.0);
            }
        }
        for point in &experiment.observations {
            let (state, solver) = solved.next().ok_or_else(|| {
                invalid("missing converged strip solver state for adjoint sensitivity")
            })?;
            let grad_force = solver.reaction_gradient(&upper_weights)?;
            budget.adjoint_solves += 1;
            rows.push(scaled_row(grad_force.dq, point.scale_n)?);

            if let Some(width) = &point.mid_width {
                let mid = (ny / 2) * (nx + 1);
                let chord = solver.x[mid + nx] - solver.x[mid];
                let dist = chord.norm();
                if !dist.is_finite() || dist <= 0.0 {
                    return Err(invalid("strip mid-width degenerate in adjoint sensitivity"));
                }
                let unit = chord / dist;
                let mut dj_dx = vec![Vec3::ZERO; n_verts];
                dj_dx[mid + nx] = unit;
                dj_dx[mid] = -unit;
                let grad_width = solver.constitutive_gradient(&dj_dx, [0.0; 4], 0.0)?;
                budget.adjoint_solves += 1;
                rows.push(scaled_row(grad_width.dq, width.scale_m)?);
            }

            for channel in &point.extensometers {
                let dj_dx =
                    measurement::gradient_dx(&channel.operator, &experiment.fixture, state)?;
                let grad_ext = solver.constitutive_gradient(&dj_dx, [0.0; 4], 0.0)?;
                budget.adjoint_solves += 1;
                rows.push(scaled_row(grad_ext.dq, channel.scale_m)?);
            }
        }
    }
    if solved.next().is_some() || rows.len() != eval.residuals.len() {
        return Err(invalid(
            "strip adjoint Jacobian row count does not match FIT residual count",
        ));
    }
    Ok(rows)
}

fn jacobian<const N: usize>(
    input: &Input,
    p: [f64; N],
    budget: &mut Budget<'_>,
    center: Option<&Evaluation>,
) -> Result<Vec<[f64; N]>, Refusal> {
    let mut rows = Vec::new();
    let mut computed_center = None;
    for j in 0..N {
        let mut delta = 1e-4;
        let mut admissible = [None, None];
        let mut refused = None;
        let mut column = None;
        for attempt in 0..=8 {
            let coordinates = [(p[j] - delta).max(0.0), (p[j] + delta).min(1.0)];
            let mut pair = [None, None];
            for (side, coordinate) in coordinates.into_iter().enumerate() {
                let mut probe = p;
                probe[j] = coordinate;
                match evaluate(input, probe, budget) {
                    Ok(value) => pair[side] = Some((coordinate, value)),
                    Err(ForwardFailure::Trial(refusal)) => refused = Some(refusal),
                    Err(ForwardFailure::Fatal(refusal)) => return Err(refusal),
                }
            }
            if let [Some((lo, left)), Some((hi, right))] = &pair {
                // Preserve the original central-difference arithmetic and solve count whenever
                // both initial probes are admitted by the ordinary forward solver.
                column = Some(
                    left.residuals
                        .iter()
                        .zip(&right.residuals)
                        .map(|(a, b)| (b - a) / (hi - lo))
                        .collect::<Vec<_>>(),
                );
                break;
            }
            for (retained, candidate) in admissible.iter_mut().zip(pair) {
                if candidate.as_ref().is_some_and(|(q, _)| *q != p[j]) {
                    *retained = candidate;
                }
            }
            if attempt < 8 {
                delta *= 0.5;
                budget.recovery.sensitivity_halvings += 1;
            }
        }
        let column = if let Some(column) = column {
            column
        } else {
            let Some((coordinate, neighbor)) = admissible
                .iter()
                .flatten()
                .min_by(|a, b| (a.0 - p[j]).abs().total_cmp(&(b.0 - p[j]).abs()))
            else {
                return Err(refused.unwrap_or_else(|| {
                    invalid("strip sensitivity has no distinct admissible neighboring state")
                }));
            };
            if center.is_none() && computed_center.is_none() {
                computed_center =
                    Some(evaluate(input, p, budget).map_err(ForwardFailure::refusal)?);
            }
            let middle = center
                .or(computed_center.as_ref())
                .ok_or_else(|| invalid("strip sensitivity requires an admitted center state"))?;
            budget.recovery.one_sided_sensitivity_columns += 1;
            middle
                .residuals
                .iter()
                .zip(&neighbor.residuals)
                .map(|(a, b)| (b - a) / (coordinate - p[j]))
                .collect()
        };
        if rows.is_empty() {
            rows = vec![[0.0; N]; column.len()];
        }
        for (row, value) in rows.iter_mut().zip(column) {
            row[j] = value;
        }
    }
    if rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("strip force sensitivity overflow"));
    }
    Ok(rows)
}

fn norms<const N: usize>(j: &[[f64; N]]) -> [f64; N] {
    std::array::from_fn(|k| j.iter().map(|row| row[k] * row[k]).sum::<f64>().sqrt())
}

fn gradient<const N: usize>(j: &[[f64; N]], e: &Evaluation) -> f64 {
    let scales = norms(j);
    (0..N)
        .map(|k| {
            let value = j
                .iter()
                .zip(&e.residuals)
                .map(|(row, r)| row[k] * r)
                .sum::<f64>();
            if scales[k] == 0.0 {
                0.0
            } else {
                value.abs() / (scales[k] * (1.0 + e.sse.sqrt()))
            }
        })
        .fold(0.0, f64::max)
}

fn excitation<const N: usize>(j: &[[f64; N]]) -> f64 {
    let scales = norms(j);
    if scales.iter().any(|s| !s.is_finite() || *s <= 1e-10) {
        return 0.0;
    }
    let mut lower = [[0.0; N]; N];
    let mut pivot = 1.0_f64;
    for i in 0..N {
        for k in 0..=i {
            let dot = j
                .iter()
                .map(|row| (row[i] / scales[i]) * (row[k] / scales[k]))
                .sum::<f64>();
            let value = dot - (0..k).map(|n| lower[i][n] * lower[k][n]).sum::<f64>();
            if i == k {
                if !value.is_finite() || value <= 0.0 {
                    return 0.0;
                }
                lower[i][k] = value.sqrt();
                pivot = pivot.min(value);
            } else {
                lower[i][k] = value / lower[k][k];
            }
        }
    }
    pivot
}

fn search<const N: usize>(
    input: &Input,
    initial: f64,
    budget: &mut Budget<'_>,
) -> Result<(Start, [f64; N], Evaluation), Refusal> {
    let mut p = [initial; N];
    let mut e = evaluate(input, p, budget).map_err(ForwardFailure::refusal)?;
    let mut lambda = 1e-3_f64;
    let mut converged = false;
    let mut iterations = 0;
    for _ in 0..input.policy.max_iterations {
        iterations += 1;
        let j = match input.policy.sensitivity {
            SensitivityMode::FiniteDifference => jacobian(input, p, budget, Some(&e))?,
            SensitivityMode::Adjoint => adjoint_jacobian(input, p, budget, &mut e)?,
        };
        if gradient(&j, &e) < 1e-6 {
            converged = true;
            break;
        }
        let scales = norms(&j);
        if scales.iter().any(|s| *s <= 1e-10) {
            break;
        }
        let mut accepted = false;
        for _ in 0..10 {
            let mut rows: Vec<[f64; N]> = j
                .iter()
                .map(|row| std::array::from_fn(|k| row[k] / scales[k]))
                .collect();
            let mut target: Vec<f64> = e.residuals.iter().map(|r| -r).collect();
            for k in 0..N {
                let mut row = [0.0; N];
                row[k] = lambda.sqrt();
                rows.push(row);
                target.push(0.0);
            }
            let scaled = lstsq(&rows, &target);
            let mut step: [f64; N] = std::array::from_fn(|k| scaled[k] / scales[k]);
            let max = step.iter().map(|v| v.abs()).fold(0.0, f64::max);
            if !max.is_finite() {
                return Err(invalid("strip inverse step overflow"));
            }
            if max > 0.25 {
                step.iter_mut().for_each(|v| *v *= 0.25 / max);
            }
            // Projection is an explicit bounded-search operation, never an input/property repair.
            let candidate = std::array::from_fn(|k| (p[k] + step[k]).clamp(0.0, 1.0));
            let trial = match evaluate(input, candidate, budget) {
                Ok(trial) => trial,
                Err(ForwardFailure::Trial(_)) => {
                    lambda *= 10.0;
                    continue;
                }
                Err(ForwardFailure::Fatal(refusal)) => return Err(refusal),
            };
            if trial.sse < e.sse {
                p = candidate;
                e = trial;
                lambda = (lambda * 0.2).max(1e-10);
                accepted = true;
                break;
            }
            lambda *= 10.0;
        }
        if !accepted {
            break;
        }
    }
    Ok((
        Start {
            initial_fraction: initial,
            parameters: physical(input, p).to_vec(),
            fit_sse: e.sse,
            converged,
            iterations,
        },
        p,
        e,
    ))
}

fn quality(values: &[f64]) -> Quality {
    Quality {
        components: values.len(),
        rms_scaled: (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt(),
        peak_scaled: values.iter().map(|v| v.abs()).fold(0.0, f64::max),
    }
}

/// Identify scalar orthotropy, declared nonlinear shape amplitudes, or independent signed tangents.
/// Every parameterization uses the production membrane authority and actual grips. Bounded starts, local
/// excitation, held-out experiments and doubled-mesh checks are publication gates. An inadequate
/// candidate returns diagnostics with no material or selection.
///
/// # Errors
/// Invalid acquisition/source/model input, exhausted aggregate work, or any failed forward solve.
pub fn calibrate(input: &Input) -> Result<Report, Refusal> {
    validate(input)?;
    match bounds(input).len() {
        4 => calibrate_n::<4>(input),
        6 => calibrate_n::<6>(input),
        7 => calibrate_n::<7>(input),
        8 => calibrate_n::<8>(input),
        9 => calibrate_n::<9>(input),
        10 => calibrate_n::<10>(input),
        11 => calibrate_n::<11>(input),
        12 => calibrate_n::<12>(input),
        13 => calibrate_n::<13>(input),
        14 => calibrate_n::<14>(input),
        _ => Err(invalid("unsupported strip identification parameter count")),
    }
}

fn calibrate_n<const N: usize>(input: &Input) -> Result<Report, Refusal> {
    let prior_response = input
        .material
        .membrane_response
        .as_ref()
        .map(|response| response.compile(&input.material.sources, true))
        .transpose()?;
    let family = input
        .response_family
        .as_ref()
        .map(|family| {
            family::Prepared::new(
                family,
                &input.material.sources,
                input
                    .parameter_bounds
                    .as_slice()
                    .try_into()
                    .map_err(|_| invalid("shape family requires four parameter bounds"))?,
            )
        })
        .transpose()?;
    let mut inherited_domain = fitted_domain(&input.material)?;
    for domain in [
        prior_response
            .as_ref()
            .map(|response| StrainDomain::new(response.strain_range()))
            .transpose()?,
        family.as_ref().map(family::Prepared::domain),
        input
            .nonlinear
            .as_ref()
            .map(Nonlinear::domain)
            .transpose()?,
        input
            .area_response
            .as_ref()
            .and_then(area::Area::scalar_range)
            .map(StrainDomain::new)
            .transpose()?,
    ]
    .into_iter()
    .flatten()
    {
        inherited_domain = Some(match inherited_domain {
            Some(prior) => intersect(prior, domain)?,
            None => domain,
        });
    }
    if inherited_domain.is_some_and(|domain| !domain.contains([0.0; 3])) {
        return Err(invalid(
            "the inherited material domain must admit the strip's undeformed reference; replacing coefficients cannot widen it",
        ));
    }
    let mut budget = Budget {
        used: 0,
        adjoint_solves: 0,
        limit: input.policy.max_forward_solves,
        inherited_domain,
        family,
        input,
        recovery: SearchRecovery::default(),
    };
    let mut starts = Vec::new();
    let mut coordinates = Vec::new();
    let mut evaluations = Vec::new();
    for initial in [0.25, 0.5, 0.75] {
        let (start, p, eval) = search::<N>(input, initial, &mut budget)?;
        starts.push(start);
        coordinates.push(p);
        evaluations.push(eval);
    }
    let best = (0..starts.len())
        .min_by(|&a, &b| starts[a].fit_sse.total_cmp(&starts[b].fit_sse))
        .unwrap_or(0);
    let p = coordinates[best];
    let parameters = starts[best].parameters.clone();
    let j = match input.policy.sensitivity {
        SensitivityMode::FiniteDifference => jacobian(input, p, &mut budget, None)?,
        SensitivityMode::Adjoint => {
            adjoint_jacobian(input, p, &mut budget, &mut evaluations[best])?
        }
    };
    let pivot = excitation(&j);
    let mut reasons = Vec::new();
    if starts.iter().any(|s| !s.converged) {
        reasons.push("not all three bounded starts reached objective stationarity".into());
    }
    if p.iter().any(|v| *v <= 2e-4 || *v >= 1.0 - 2e-4) {
        reasons
            .push("best fit lies at a parameter bound; expand justified bounds and repeat".into());
    }
    if pivot < input.policy.min_excitation_pivot {
        reasons.push(if input.area_response.is_some() {
            format!("FIT forces/widths do not independently identify all {} fitted directional/area coefficients", parameters.len())
        } else if input.nonlinear.is_some() {
            "FIT forces/widths do not independently identify every signed tangent and coupling coefficient".into()
        } else {
            "FIT forces/widths do not independently identify all four coefficients".into()
        });
    }
    for (i, start) in starts.iter().enumerate() {
        if start.fit_sse <= starts[best].fit_sse + 1e-8 * (1.0 + starts[best].fit_sse)
            && coordinates[i]
                .iter()
                .zip(p)
                .any(|(a, b)| (a - b).abs() > 0.02)
        {
            reasons.push("equivalent FIT minima have materially different coefficients".into());
            break;
        }
    }
    let mut predictions = Vec::new();
    let mut fit_errors = Vec::new();
    let mut check_errors = Vec::new();
    let mut mesh_peak = 0.0_f64;
    // Preserve exact zero as well as the actually solved rest state (which may contain roundoff).
    // Every objective, probe, coarse/refined state and CHECK already obeys inherited support.
    let mut ranges = [[0.0_f64; 2]; 3];
    let mut area_range = input
        .area_response
        .as_ref()
        .filter(|mode| mode.identifies())
        .map(|_| [1.0_f64; 2]);
    for experiment in &input.experiments {
        let mut refined = experiment.fixture.clone();
        refined.cells.iter_mut().for_each(|n| *n *= 2);
        if experiment.partition == Partition::Fit {
            if area_range.is_some() {
                let zero = budget.solve(&experiment.fixture, &parameters, 0.0, &input.controls)?;
                observe_area(&mut area_range, &zero);
                for (range, actual) in ranges.iter_mut().zip(zero.green_strain_range) {
                    range[0] = range[0].min(actual[0]);
                    range[1] = range[1].max(actual[1]);
                }
            }
            let zero = budget.solve(&refined, &parameters, 0.0, &input.controls)?;
            observe_area(&mut area_range, &zero);
            for (range, actual) in ranges.iter_mut().zip(zero.green_strain_range) {
                range[0] = range[0].min(actual[0]);
                range[1] = range[1].max(actual[1]);
            }
        }
        for observation in &experiment.observations {
            let coarse = budget.solve(
                &experiment.fixture,
                &parameters,
                observation.extension_fraction,
                &input.controls,
            )?;
            let state = budget.solve(
                &refined,
                &parameters,
                observation.extension_fraction,
                &input.controls,
            )?;
            mesh_peak = mesh_peak.max((state.force_n - coarse.force_n).abs() / observation.scale_n);
            let error = state.force_n - observation.force_n;
            let width_error = observation
                .mid_width
                .as_ref()
                .map(|w| state.mid_width_m - w.value_m);
            let errors = if experiment.partition == Partition::Fit {
                &mut fit_errors
            } else {
                &mut check_errors
            };
            errors.push(error / observation.scale_n);
            if let Some(width) = &observation.mid_width {
                errors.push((state.mid_width_m - width.value_m) / width.scale_m);
                mesh_peak =
                    mesh_peak.max((state.mid_width_m - coarse.mid_width_m).abs() / width.scale_m);
            }
            let extensometers = extensometer_predictions(
                observation,
                (&experiment.fixture, &coarse),
                (&refined, &state),
                errors,
                &mut mesh_peak,
            )?;
            if experiment.partition == Partition::Fit {
                observe_area(&mut area_range, &coarse);
                observe_area(&mut area_range, &state);
                if area_range.is_some() {
                    for (range, actual) in ranges.iter_mut().zip(coarse.green_strain_range) {
                        range[0] = range[0].min(actual[0]);
                        range[1] = range[1].max(actual[1]);
                    }
                }
                for (range, actual) in ranges.iter_mut().zip(state.green_strain_range) {
                    range[0] = range[0].min(actual[0]);
                    range[1] = range[1].max(actual[1]);
                }
            }
            predictions.push(Prediction {
                experiment: experiment.id.clone(),
                observation: observation.id.clone(),
                partition: experiment.partition,
                state,
                force_residual_n: error,
                width_residual_m: width_error,
                extensometers,
            });
        }
    }
    ranges = budget
        .within_inherited_domain(StrainDomain::new(ranges)?)?
        .ranges();
    if let Some(range) = &mut area_range {
        let support = input
            .area_response
            .as_ref()
            .ok_or_else(|| {
                invalid("physical-area support requires an explicit identification mode")
            })?
            .range(area_template(input))?;
        range[0] = range[0].max(support[0]);
        range[1] = range[1].min(support[1]);
        if range[0] >= range[1] {
            reasons.push("FIT states do not characterize a nonempty physical-area interval".into());
        }
    }
    let mut fit = quality(&fit_errors);
    let mut check = (!check_errors.is_empty()).then(|| quality(&check_errors));
    if predictions
        .iter()
        .filter(|p| p.partition == Partition::Check)
        .any(|prediction| {
            prediction
                .state
                .green_strain_range
                .iter()
                .zip(ranges)
                .any(|(actual, fitted)| actual[0] < fitted[0] || actual[1] > fitted[1])
        })
    {
        reasons.push(
            "held-out CHECK states escape the FIT-only local strain domain; CHECK data cannot enlarge the fitted law's domain".into(),
        );
    }
    if check.is_none() {
        reasons.push(
            "an independent held-out CHECK acquisition is required before publishing a material"
                .into(),
        );
    }
    if let Some(fitted) = area_range
        && predictions
            .iter()
            .filter(|p| p.partition == Partition::Check)
            .any(|p| {
                p.state
                    .area_ratio_range
                    .is_some_and(|actual| actual[0] < fitted[0] || actual[1] > fitted[1])
            })
    {
        reasons.push("held-out CHECK states escape the FIT-only physical-area domain; CHECK data cannot enlarge J support".into());
    }
    for (name, q) in [("FIT", Some(&fit)), ("CHECK", check.as_ref())] {
        if let Some(q) = q
            && (!q.rms_scaled.is_finite()
                || !q.peak_scaled.is_finite()
                || q.rms_scaled > input.policy.max_rms_scaled
                || q.peak_scaled > input.policy.max_peak_scaled)
        {
            reasons.push(format!(
                "{name} force/width residuals exceed the declared elastic-model budget"
            ));
        }
    }
    if !mesh_peak.is_finite() || mesh_peak > input.policy.max_mesh_scaled {
        reasons.push(
            "doubling the strip mesh changes predictions beyond the declared numerical budget"
                .into(),
        );
    }
    let digest = sha256_hex(&serde_json::to_vec(input).map_err(invalid)?);
    let mut nonlinear_material = None;
    if reasons.is_empty()
        && (budget.family.is_some() || input.nonlinear.is_some() || input.area_response.is_some())
    {
        match publish(input, &digest, &parameters, ranges, area_range, budget.family.as_ref()) {
            Ok(material) => nonlinear_material = Some(material),
            Err(refusal) => reasons.push(format!(
                "nonlinear FIT-support publication failed: {}; no material or missing branch was invented",
                refusal.message
            )),
        }
    }
    // Replay the actual clipped publication, including its production interpolation arithmetic.
    // Its reference moduli are initialization/reporting values, never a replacement scalar law.
    let replay_response = nonlinear_material
        .as_ref()
        .map(|material| {
            material
                .membrane_response
                .as_ref()
                .ok_or_else(|| invalid("nonlinear strip publication lacks its response"))?
                .compile(&material.sources, true)
        })
        .transpose()?;
    // A converged unconstrained state alone does not prove that ordinary initialization and
    // Newton trials can reproduce it inside the fitted domain. Exercise the real domain gate;
    // never enlarge the authority with CHECK states, trial states or a numerical tolerance.
    let mut domain_replay_verified = false;
    if reasons.is_empty() {
        let domain = StrainDomain::new(ranges)?;
        let mut replayed = Vec::new();
        'replay: for experiment in &input.experiments {
            let mut coarse_states: Vec<State> = Vec::new();
            for factor in [1, 2] {
                let mut fixture = experiment.fixture.clone();
                fixture.cells.iter_mut().for_each(|n| *n *= factor);
                for (i, observation) in experiment.observations.iter().enumerate() {
                    let state = match budget.replay(
                        &fixture,
                        &parameters,
                        domain,
                        replay_response.as_ref(),
                        observation.extension_fraction,
                        &input.controls,
                    ) {
                        Ok(state) => state,
                        Err(refusal) => {
                            if refusal.code == RefusalCode::BudgetExceeded {
                                return Err(refusal);
                            }
                            reasons.push(format!(
                                "fitted-domain replay failed for {} / {} at mesh factor {factor}: {}; no domain was widened",
                                experiment.id, observation.id, refusal.message,
                            ));
                            break 'replay;
                        }
                    };
                    if replay_response.is_some() {
                        if factor == 1 {
                            coarse_states.push(state);
                        } else {
                            replayed.push((coarse_states[i].clone(), state));
                        }
                    }
                }
            }
        }
        if reasons.is_empty() && replay_response.is_some() {
            fit_errors.clear();
            check_errors.clear();
            mesh_peak = 0.0;
            let observations = input
                .experiments
                .iter()
                .flat_map(|e| e.observations.iter().map(move |o| (e, o)));
            for ((prediction, (coarse, state)), (experiment, observation)) in
                predictions.iter_mut().zip(replayed).zip(observations)
            {
                mesh_peak =
                    mesh_peak.max((state.force_n - coarse.force_n).abs() / observation.scale_n);
                let error = state.force_n - observation.force_n;
                let width_error = observation
                    .mid_width
                    .as_ref()
                    .map(|width| state.mid_width_m - width.value_m);
                let errors = if experiment.partition == Partition::Fit {
                    &mut fit_errors
                } else {
                    &mut check_errors
                };
                errors.push(error / observation.scale_n);
                if let Some(width) = &observation.mid_width {
                    errors.push((state.mid_width_m - width.value_m) / width.scale_m);
                    mesh_peak = mesh_peak
                        .max((state.mid_width_m - coarse.mid_width_m).abs() / width.scale_m);
                }
                let mut refined = experiment.fixture.clone();
                refined.cells.iter_mut().for_each(|n| *n *= 2);
                prediction.extensometers = extensometer_predictions(
                    observation,
                    (&experiment.fixture, &coarse),
                    (&refined, &state),
                    errors,
                    &mut mesh_peak,
                )?;
                prediction.state = state;
                prediction.force_residual_n = error;
                prediction.width_residual_m = width_error;
            }
            fit = quality(&fit_errors);
            check = Some(quality(&check_errors));
            for (name, quality) in [("FIT", &fit), ("CHECK", check.as_ref().unwrap())] {
                if !quality.rms_scaled.is_finite()
                    || !quality.peak_scaled.is_finite()
                    || quality.rms_scaled > input.policy.max_rms_scaled
                    || quality.peak_scaled > input.policy.max_peak_scaled
                {
                    reasons.push(format!(
                        "published nonlinear {name} force/width residuals exceed the declared elastic-model budget"
                    ));
                }
            }
            if !mesh_peak.is_finite() || mesh_peak > input.policy.max_mesh_scaled {
                reasons.push(
                    "published nonlinear coarse/refined predictions exceed the declared numerical budget"
                        .into(),
                );
            }
        }
        domain_replay_verified = reasons.is_empty();
    }
    let material = if reasons.is_empty() {
        Some(match nonlinear_material {
            Some(material) => material,
            None => publish(input, &digest, &parameters, ranges, area_range, None)?,
        })
    } else {
        None
    };
    let selection = material
        .as_ref()
        .map(|material| {
            textile_import::catalog()?
                .resolve(&Selection {
                    id: material.id.clone(),
                    record: Some(material.clone()),
                    allow_estimates: input.allow_estimates,
                    ..Selection::default()
                })
                .map(|m| m.frozen_selection())
        })
        .transpose()?;
    let selection_json = selection
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(invalid)?;
    Ok(Report {
        schema: "ftx-strip-calibration-report/1".into(),
        input: input.clone(),
        input_sha256: digest,
        parameters,
        parameter_descriptors: input
            .response_family
            .as_ref()
            .map(|_| family::descriptors().to_vec())
            .or_else(|| input.nonlinear.as_ref().map(Nonlinear::descriptors))
            .or_else(|| {
                input
                    .area_response
                    .as_ref()
                    .map(|_| area::scalar_descriptors())
            })
            .map(|mut descriptors| {
                if let Some(area) = &input.area_response {
                    descriptors.extend(area.descriptors());
                }
                descriptors
            }),
        parameter_bounds: if input.area_response.is_some() {
            bounds(input)
        } else {
            input
                .nonlinear
                .as_ref()
                .map_or_else(Vec::new, Nonlinear::bounds)
        },
        accepted: material.is_some(),
        reasons,
        material,
        selection,
        selection_json,
        starts,
        excitation_pivot: pivot,
        forward_solves: budget.used,
        adjoint_solves: budget.adjoint_solves,
        search_recovery: (budget.recovery.refused_forward_solves > 0)
            .then(|| budget.recovery.clone()),
        fit,
        check,
        mesh_change_peak_scaled: mesh_peak,
        domain_replay_verified,
        fitted_green_strain_range: ranges,
        fitted_area_ratio_range: area_range,
        predictions,
        model_scope: {
            let mut scope = if input.area_response.is_some() {
                if input.nonlinear.is_some() {
                    format!(
                        "{SCOPE} Independent signed piecewise-linear interval tangents and constant normal coupling are identified at declared fixed Green-strain knots. Knot positions, symmetry, missing branches and history are not identified."
                    )
                } else if input.response_family.is_some() {
                    format!(
                        "{SCOPE} Conditional on caller-declared signed piecewise-linear shapes; three dimensionless amplitudes and constant normal coupling are identified. Knots, relative shape and unobserved branches are not identified."
                    )
                } else {
                    format!(
                        "{SCOPE} Four scalar orthotropic base parameters are identified under explicit signed trial support. The base Q and separate physical-area energy are published through the canonical membrane response."
                    )
                }
            } else if input.nonlinear.is_some() {
                format!(
                    "{SCOPE} Independent signed piecewise-linear interval tangents and constant normal coupling are identified at declared fixed Green-strain knots. Knot positions, symmetry, missing branches, history and area-preserving behavior are not identified."
                )
            } else if input.response_family.is_some() {
                format!(
                    "{SCOPE} Conditional on caller-declared signed piecewise-linear shapes; only three dimensionless amplitudes and constant normal coupling are identified. Knots, relative shape, unobserved branches and area-preserving behavior are not identified."
                )
            } else {
                SCOPE.into()
            };
            if let Some(area) = &input.area_response {
                scope.push_str(area.scope());
            }
            scope
        },
    })
}

fn publish(
    input: &Input,
    digest: &str,
    parameters: &[f64],
    ranges: [[f64; 2]; 3],
    area_range: Option<[f64; 2]>,
    family: Option<&family::Prepared>,
) -> Result<Material, Refusal> {
    let mut material = input.material.clone();
    let source = format!("strip-{digest}");
    if material.sources.contains_key(&source) {
        return Err(invalid("strip receipt source identity already exists"));
    }
    let source_family = if area_range.is_some() {
        "local:ftx-strip-area-calibration"
    } else {
        "local:ftx-strip-calibration"
    };
    material.sources.insert(source.clone(), Source { reference: format!("{source_family}/{digest}"), accessed_on: input.fitted_on.clone(), note: format!("Inverse production-membrane clamped-strip model; complete acquisitions, assumptions, sensitivity and mesh checks retained in ftx-strip-calibration-report/1 input {digest}. {}", input.method) });
    let mut conditions = input.conditions.clone();
    conditions.insert(
        "fitted_green_strain_range".into(),
        serde_json::to_string(&ranges).map_err(invalid)?,
    );
    conditions.insert("held_out_checks".into(), "true".into());
    conditions.insert("fixture_model".into(), SCOPE.into());
    if let Some(range) = area_range {
        conditions.insert(
            "fitted_area_ratio_range".into(),
            serde_json::to_string(&range).map_err(invalid)?,
        );
    }
    if let Some(family) = family {
        material.membrane_response = Some(family.publish(
            four(base_parameters(input, parameters))?,
            StrainDomain::new(ranges)?,
            &source,
            &conditions,
        )?);
        for key in KEYS {
            material.properties.remove(key);
        }
    } else if let Some(model) = &input.nonlinear {
        material.membrane_response = Some(model.response(
            base_parameters(input, parameters),
            Some(ranges),
            &source,
            &conditions,
        )?);
        for key in KEYS {
            material.properties.remove(key);
        }
    } else if input.area_response.is_some() {
        material.membrane_response = Some(area::scalar_response(
            four(base_parameters(input, parameters))?,
            ranges,
            &source,
            &conditions,
        )?);
        for key in KEYS {
            material.properties.remove(key);
        }
    } else {
        for (key, &value) in KEYS.into_iter().zip(parameters) {
            material.properties.insert(key.into(), Quantity { value, unit: if key == "poisson_warp_weft" { "1" } else { "N/m" }.into(), evidence: Evidence::Estimate, source: source.clone(), method: "Bounded inverse orthotropic Green-strain membrane using clamped strip equilibrium, total grip force and optional transverse width; local sensitivity and held-out doubled-mesh checks. No independent bending or contact identification.".into(), conditions: conditions.clone(), bounds: None });
        }
    }
    if let (Some(mode), Some(response)) = (&input.area_response, &mut material.membrane_response) {
        mode.attach(
            response,
            area_template(input),
            &parameters[base_parameters(input, parameters).len()..],
            &source,
            &conditions,
            area_range,
        )?;
        response.coupling.method = "Signed constant normal coupling identified with directional response under the explicit physical-area policy; all coefficients remain model estimates.".into();
    }
    material.validate()?;
    Ok(material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strip::experiment::solve;
    use crate::strip::measurement::{Extensometer, Operator};

    #[test]
    fn adjoint_jacobian_matches_central_finite_difference_across_all_channels() {
        let bias = std::f64::consts::FRAC_1_SQRT_2;
        let truth = [310.0, 190.0, 0.22, 68.0];
        let controls = Controls {
            tolerance_m: 1e-12,
            force_tolerance_n: 1e-8,
            max_newton: 100,
        };
        let mut sources = BTreeMap::new();
        let mut experiments = Vec::new();
        let ops = [
            (
                "edge-chord",
                Operator::EdgeChord {
                    station_fraction: 0.35,
                },
            ),
            (
                "edge-proj",
                Operator::EdgeProjection {
                    station_fraction: 0.65,
                    axis: [0.8, 0.6],
                },
            ),
            (
                "mat-chord",
                Operator::MaterialChord {
                    start_fraction: [0.15, 0.2],
                    end_fraction: [0.85, 0.78],
                },
            ),
            (
                "mat-proj",
                Operator::MaterialProjection {
                    start_fraction: [0.25, 0.3],
                    end_fraction: [0.72, 0.82],
                    axis: [-0.6, 0.8],
                },
            ),
        ];
        for (id, axis) in [
            ("fit-warp", [0.0, 1.0]),
            ("fit-weft", [1.0, 0.0]),
            ("fit-bias", [bias, bias]),
        ] {
            let fixture = Fixture {
                gauge_length_m: 0.1,
                width_m: 0.06,
                warp_axis: axis,
                cells: [4, 4],
            };
            let state = solve(&fixture, truth, 0.07, &controls).unwrap();
            let extensometers = ops
                .iter()
                .map(|(name, op)| Extensometer {
                    id: format!("{id}-{name}"),
                    operator: op.clone(),
                    value_m: measurement::evaluate(op, &fixture, &state).unwrap(),
                    scale_m: 0.001,
                })
                .collect();
            sources.insert(
                id.into(),
                Source {
                    reference: format!("test:adjoint-jacobian/{id}"),
                    accessed_on: "2026-11-20".into(),
                    note: "Synthetic unit fixture comparing exact IFT adjoint against central FD"
                        .into(),
                },
            );
            experiments.push(Experiment {
                id: id.into(),
                fixture,
                source: id.into(),
                evidence: Evidence::Estimate,
                partition: Partition::Fit,
                observations: vec![Observation {
                    id: format!("{id}-0"),
                    extension_fraction: 0.07,
                    force_n: state.force_n,
                    scale_n: 0.1,
                    mid_width: Some(Width {
                        value_m: state.mid_width_m,
                        scale_m: 0.001,
                    }),
                    extensometers,
                }],
            });
        }
        let mut input = Input {
            schema: "ftx-strip-calibration/1".into(),
            material: Material {
                id: "synthetic-adjoint-check".into(),
                name: "Synthetic adjoint Jacobian fixture".into(),
                form: Form::Woven,
                composition: vec![],
                composition_complete: false,
                structure: BTreeMap::new(),
                properties: BTreeMap::new(),
                membrane_response: None,
                membrane_relaxation: None,
                thermal_response: None,
                optical_faces: None,
                sources,
            },
            method: "Synthetic planar grip fixture".into(),
            conditions: BTreeMap::new(),
            fitted_on: "2026-11-20".into(),
            experiments,
            parameter_bounds: vec![[200.0, 420.0], [100.0, 280.0], [0.05, 0.35], [40.0, 110.0]],
            response_family: None,
            nonlinear: None,
            area_response: None,
            admit_planar_elastic_model: true,
            allow_estimated_observations: true,
            replace_existing: false,
            allow_estimates: true,
            controls,
            policy: Policy {
                max_rms_scaled: 1.0,
                max_peak_scaled: 1.0,
                max_mesh_scaled: 1.0,
                min_excitation_pivot: 1e-4,
                max_iterations: 40,
                max_forward_solves: 1000,
                sensitivity: SensitivityMode::Adjoint,
            },
        };
        let p = [0.37, 0.61, 0.43, 0.52];
        let mut adj_budget = Budget {
            used: 0,
            adjoint_solves: 0,
            limit: 1000,
            inherited_domain: None,
            family: None,
            input: &input,
            recovery: SearchRecovery::default(),
        };
        let mut eval = evaluate(&input, p, &mut adj_budget)
            .map_err(ForwardFailure::refusal)
            .unwrap();
        let j_adj = adjoint_jacobian(&input, p, &mut adj_budget, &mut eval).unwrap();
        assert_eq!(adj_budget.used, 3);
        assert_eq!(adj_budget.adjoint_solves, 18); // 3 strips * (1 force + 1 width + 4 extensometers)

        input.policy.sensitivity = SensitivityMode::FiniteDifference;
        let mut fd_budget = Budget {
            used: 0,
            adjoint_solves: 0,
            limit: 1000,
            inherited_domain: None,
            family: None,
            input: &input,
            recovery: SearchRecovery::default(),
        };
        let j_fd = jacobian(&input, p, &mut fd_budget, None).unwrap();
        assert_eq!(fd_budget.used, 24); // 2 * 4 params * 3 strips
        assert_eq!(j_adj.len(), j_fd.len());
        for (row_idx, (a_row, f_row)) in j_adj.iter().zip(&j_fd).enumerate() {
            for col_idx in 0..4 {
                let a = a_row[col_idx];
                let f = f_row[col_idx];
                assert!(
                    (a - f).abs() <= 1e-6 * (1.0 + f.abs()),
                    "row={row_idx} col={col_idx}: adjoint={a} fd={f}"
                );
            }
        }
    }
}

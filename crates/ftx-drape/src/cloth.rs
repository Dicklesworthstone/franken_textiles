//! The cloth model and its contact-constrained Newton solver (plan §11.2–11.4).
//!
//! Each time step minimises the incremental potential
//! `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane(x) + Ψ_bend(x)` with `x̃ = xₙ + h vₙ + h² g` and lumped mass,
//! by projected Newton: element Hessians projected to positive semidefinite, a sparse LDLᵀ solve whose
//! symbolic analysis (ordered on the vertex graph) is reused while contact couplings stay unchanged,
//! and an Armijo backtracking line search. Near floating-point energy resolution, a full step may
//! instead pass approximate Wolfe slopes and strict gradient progress within a bounded numerical
//! energy discrepancy. The requested Newton-correction tolerance is unchanged. A static solve
//! minimises `Ψ − Σ mᵢ g·xᵢ` the same way. Units: metres, kilograms, seconds.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::material_response::{CompiledMembrane, StrainDomain};
use serde::{Deserialize, Serialize};

use crate::contact::{Body, Contact, ContactParams, Intersection};
use crate::damping::{DampingOptions, DampingStep, VelocityMap};
use crate::eigen::{project_psd, project_psd_translation_free};
use crate::elements::{
    BendingModel, CompressionModel, ElasticSegment, Hinge, Local, Membrane, MembraneElement,
};
use crate::integrator::{GeneralizedAlphaParams, TimeIntegrator};
use crate::sparse::advanced::{
    Precond, SteihaugConfig, eisenstat_walker, ilu0_spd_shifted, steihaug_cg,
};
use crate::sparse::precond::{PcgFailure, pcg};
use crate::sparse::{Coo, Csr, CsrFill, LdltFactor, LdltOptions, SymbolicLdlt, amd_order};

/// Edge (lo, hi) → its uses as (edge start, edge end, opposite vertex, triangle).
type EdgeUses = std::collections::BTreeMap<(usize, usize), Vec<(usize, usize, usize, usize)>>;

mod materials;
pub use materials::SheetMaterial;
mod bending;
mod membrane_plasticity;
mod plasticity;
mod relaxation;

/// The cloth: flat rest geometry, elements, lumped masses and material.
#[derive(Clone, Debug)]
pub struct ClothModel {
    /// Rest coordinates in the material frame (m; +u along the grain).
    pub rest: Vec<Vec2>,
    /// Membrane triangles.
    pub membranes: Vec<MembraneElement>,
    /// Bending hinges (interior edges).
    pub hinges: Vec<Hinge>,
    /// Lumped vertex masses (kg).
    pub mass: Vec<f64>,
    // Optional source-bound point masses, applied once after sheet assembly and seam welding.
    pub(crate) attached_mass: Option<crate::attached_mass::Binding>,
    /// Membrane material.
    pub membrane: Membrane,
    /// Compression factor of each membrane element, per direction (`membrane.compression` unless set by
    /// [`ClothModel::set_compression`]); [`ClothModel::material`] combines the two.
    pub compression: Vec<[f64; 2]>,
    /// Bending stiffness B (N·m, flexural rigidity per unit width).
    pub bending: f64,
    /// Hinge bending formulation (default: legacy [`BendingModel::Quadratic`]).
    pub bending_model: BendingModel,
    /// Membrane compression-softening formulation (default: legacy [`CompressionModel::AxisAligned`]).
    pub compression_model: CompressionModel,
    /// Tension-only elastic segments (a casing's elastic), between vertices that share an element so the
    /// Hessian pattern does not change.
    pub elastics: Vec<ElasticSegment>,
    // None preserves the homogeneous model's arithmetic and mutable legacy coefficients.
    element_materials: Option<Vec<Membrane>>,
    // Nonlinear laws are immutable source-bound constitutive responses; None entries use the
    // corresponding linear/reference-tangent material above.
    element_responses: Option<Vec<Option<CompiledMembrane>>>,
    // Scalar calibration bounds survive source resolution and grain-frame scene assembly.
    element_domains: Option<Vec<Option<StrainDomain>>>,
    hinge_bending: Option<Vec<f64>>,
    // Each hinge's adjacent triangles and their shares of the existing bending integration area.
    hinge_support: Vec<[(usize, f64); 2]>,
    // Optional panel curvature replaces interior scalar hinges, never the explicit seam hinges.
    panel_bending: Option<bending::PanelField>,
}

impl ClothModel {
    /// Builds the model from a triangulated flat piece.
    ///
    /// # Errors
    /// [`RefusalCode::DegenerateGeometry`] for a degenerate triangle, an out-of-range index or an edge
    /// shared by more than two triangles; [`RefusalCode::OutOfRange`] for a non-positive density.
    pub fn new(
        rest: Vec<Vec2>,
        tris: &[[usize; 3]],
        membrane: Membrane,
        bending: f64,
        areal_density: f64,
    ) -> Result<Self, Refusal> {
        if let Some((t, _)) = tris
            .iter()
            .enumerate()
            .find(|(_, tri)| tri.iter().any(|&i| i >= rest.len()))
        {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("triangle {t} indexes a missing vertex"),
            ));
        }
        let tri_rest: Vec<[Vec2; 3]> = tris.iter().map(|t| t.map(|i| rest[i])).collect();
        Self::from_triangles(
            rest.len(),
            tris,
            &tri_rest,
            membrane,
            bending,
            areal_density,
        )
    }

    /// Builds the model from triangles that each carry their own rest coordinates, so a vertex may sit
    /// at different rest positions in different triangles: a seam welded across the pattern, or a tube
    /// closed on itself. A welded hinge uses the cut geometry from each side of the seam.
    /// `rest` (per vertex) keeps the first rest position seen for each vertex.
    ///
    /// # Errors
    /// As [`ClothModel::new`].
    pub fn from_triangles(
        n: usize,
        tris: &[[usize; 3]],
        tri_rest: &[[Vec2; 3]],
        membrane: Membrane,
        bending: f64,
        areal_density: f64,
    ) -> Result<Self, Refusal> {
        if !areal_density.is_finite() || areal_density <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("areal density {areal_density} kg/m² must be positive"),
            ));
        }
        if tri_rest.len() != tris.len() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "one rest triangle per triangle is required",
            ));
        }
        let mut rest: Vec<Option<Vec2>> = vec![None; n];
        let mut membranes = Vec::with_capacity(tris.len());
        let mut mass = vec![0.0; n];
        // edge (lo, hi) → [(edge start, edge end, opposite, triangle)] in triangle order
        let mut edges: EdgeUses = std::collections::BTreeMap::new();
        for (t, tri) in tris.iter().enumerate() {
            if tri.iter().any(|&i| i >= n) {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("triangle {t} indexes a missing vertex"),
                ));
            }
            for (k, &i) in tri.iter().enumerate() {
                rest[i].get_or_insert(tri_rest[t][k]);
            }
            let e = MembraneElement::new(*tri, tri_rest[t]).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("triangle {t} is degenerate"),
                )
            })?;
            for &i in tri {
                mass[i] += areal_density * e.area / 3.0;
            }
            membranes.push(e);
            for k in 0..3 {
                let (a, b, o) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
                edges
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((a, b, o, t));
            }
        }
        let mut hinges = Vec::new();
        let mut hinge_tris = Vec::new();
        let mut interior = vec![0usize; tris.len()];
        for (key, uses) in &edges {
            match uses.as_slice() {
                [_] => {}
                [(a, b, o1, t1), (_, _, o2, t2)] => {
                    let v = [*a, *b, *o1, *o2];
                    let at = |t: usize, i: usize| {
                        tri_rest[t][tris[t].iter().position(|&j| j == i).unwrap_or(0)]
                    };
                    let first = [*a, *b, *o1].map(|i| at(*t1, i));
                    let second = [*a, *b, *o2].map(|i| at(*t2, i));
                    let h = Hinge::from_rest_triangles(v, first, second).ok_or_else(|| {
                        Refusal::new(
                            RefusalCode::DegenerateGeometry,
                            format!("hinge on edge {key:?} is degenerate"),
                        )
                    })?;
                    hinges.push(h);
                    hinge_tris.push([*t1, *t2]);
                    interior[*t1] += 1;
                    interior[*t2] += 1;
                }
                _ => {
                    return Err(Refusal::new(
                        RefusalCode::DegenerateGeometry,
                        format!("edge {key:?} is shared by more than two triangles"),
                    ));
                }
            }
        }
        // Each triangle's area goes to its interior hinges, so free edges are not too soft.
        let mut hinge_support = Vec::with_capacity(hinges.len());
        for (h, [t1, t2]) in hinges.iter_mut().zip(hinge_tris) {
            let (a, b) = (
                membranes[t1].area / interior[t1] as f64,
                membranes[t2].area / interior[t2] as f64,
            );
            h.set_area(a + b);
            hinge_support.push([(t1, a), (t2, b)]);
        }
        Ok(Self {
            rest: rest.into_iter().map(Option::unwrap_or_default).collect(),
            compression: vec![membrane.compression; membranes.len()],
            membranes,
            hinges,
            mass,
            attached_mass: None,
            membrane,
            bending,
            bending_model: BendingModel::Quadratic,
            compression_model: CompressionModel::AxisAligned,
            elastics: Vec::new(),
            element_materials: None,
            element_responses: None,
            element_domains: None,
            hinge_bending: None,
            hinge_support,
            panel_bending: None,
        })
    }

    /// Sets each membrane element's compression factor (both directions), e.g. softer where gathers
    /// draw the cloth up.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] unless there is one factor per element, each in (0, 1].
    pub fn set_compression(&mut self, per_element: &[f64]) -> Result<(), Refusal> {
        if per_element.len() != self.membranes.len() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} compression factors for {} membrane elements",
                    per_element.len(),
                    self.membranes.len()
                ),
            ));
        }
        if let Some(c) = per_element.iter().find(|c| !(**c > 0.0 && **c <= 1.0)) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("compression factor {c} must be in (0, 1]"),
            ));
        }
        self.compression = per_element.iter().map(|&c| [c, c]).collect();
        Ok(())
    }

    /// The membrane material of element `e` (its own compression factor).
    #[must_use]
    pub fn material(&self, e: usize) -> Membrane {
        Membrane {
            compression: self.compression[e],
            ..self
                .element_materials
                .as_ref()
                .map_or(self.membrane, |m| m[e])
        }
    }

    /// Effective bending coefficient of hinge `h` (N·m), including unlike adjacent sheets.
    #[must_use]
    pub fn hinge_rigidity(&self, h: usize) -> f64 {
        if self
            .panel_bending
            .as_ref()
            .is_some_and(|p| p.replaced_hinges[h])
        {
            return 0.0;
        }
        self.hinge_bending.as_ref().map_or(self.bending, |b| b[h])
    }

    /// Sets each hinge's rest dihedral angle `θ₀` (rad, strictly inside `(−π, π)`) for pressed
    /// pleats, ironed creases, or folded lapels under [`BendingModel::DiscreteShells`].
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on length mismatch or an angle outside `(−π, π)`.
    pub fn set_hinge_rest_angles(&mut self, angles: &[f64]) -> Result<(), Refusal> {
        if angles.len() != self.hinges.len() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "{} hinge rest angles for {} hinges",
                    angles.len(),
                    self.hinges.len()
                ),
            ));
        }
        for (i, &theta) in angles.iter().enumerate() {
            if !(theta.is_finite() && theta.abs() < std::f64::consts::PI) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("hinge {i} rest angle {theta} rad must be finite and in (-pi, pi)"),
                ));
            }
        }
        for (hinge, &theta) in self.hinges.iter_mut().zip(angles) {
            hinge.rest_angle = theta;
        }
        Ok(())
    }

    /// Membrane energy of element `e` under [`Self::compression_model`].
    #[must_use]
    pub fn membrane_energy(&self, e: usize, x: &[Vec3]) -> f64 {
        self.membranes[e].energy_with_model(x, &self.material(e), self.compression_model)
    }

    /// Membrane energy, gradient and Hessian of element `e` under [`Self::compression_model`].
    #[must_use]
    pub fn membrane_local(&self, e: usize, x: &[Vec3]) -> Local<9, 81> {
        self.membranes[e].local_with_model(x, &self.material(e), self.compression_model)
    }

    /// Hinge energy of hinge `h` under [`Self::bending_model`].
    #[must_use]
    pub fn hinge_energy(&self, h: usize, x: &[Vec3]) -> f64 {
        self.hinges[h].energy_with_model(x, self.hinge_rigidity(h), self.bending_model)
    }

    /// Hinge energy, gradient and Hessian of hinge `h` under [`Self::bending_model`].
    #[must_use]
    pub fn hinge_local(&self, h: usize, x: &[Vec3]) -> Local<12, 144> {
        self.hinges[h].local_with_model(x, self.hinge_rigidity(h), self.bending_model)
    }

    /// Whether element coefficients differ, rather than merely their source identities.
    #[must_use]
    pub fn is_heterogeneous(&self) -> bool {
        self.element_materials.is_some()
            || self.panel_bending.as_ref().is_some_and(|p| p.heterogeneous)
    }

    /// Bind one optional characterized nonlinear response to each membrane triangle.
    /// An all-`None` field preserves the legacy/reference-tangent solver path exactly.
    ///
    /// # Errors
    /// Unless there is exactly one response slot per membrane element.
    pub fn set_membrane_responses(
        &mut self,
        responses: &[Option<CompiledMembrane>],
    ) -> Result<(), Refusal> {
        if responses.len() != self.membranes.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "one nonlinear response slot per membrane triangle is required",
            ));
        }
        self.element_responses = responses
            .iter()
            .any(Option::is_some)
            .then(|| responses.to_vec());
        Ok(())
    }

    /// Nonlinear law of membrane element `e`, when characterized.
    #[must_use]
    pub fn membrane_response(&self, e: usize) -> Option<&CompiledMembrane> {
        self.element_responses
            .as_ref()
            .and_then(|r| r.get(e))
            .and_then(Option::as_ref)
    }

    /// Bind the optional physical domain of each fitted scalar membrane.
    ///
    /// # Errors
    /// Unless there is exactly one domain slot per membrane triangle.
    pub fn set_membrane_domains(
        &mut self,
        domains: &[Option<StrainDomain>],
    ) -> Result<(), Refusal> {
        if domains.len() != self.membranes.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "one fitted domain slot per membrane triangle is required",
            ));
        }
        self.element_domains = domains
            .iter()
            .any(Option::is_some)
            .then(|| domains.to_vec());
        Ok(())
    }

    /// Optional source-fitted scalar domain of one membrane element.
    #[must_use]
    pub fn membrane_domain(&self, e: usize) -> Option<StrainDomain> {
        self.element_domains
            .as_ref()
            .and_then(|domains| domains.get(e))
            .copied()
            .flatten()
    }

    /// Refuse a physical state outside any fitted scalar or nonlinear strain domain.
    /// Numerical compression/gather softening never widens this domain.
    ///
    /// # Errors
    /// First element whose raw material-frame Green strain leaves its characterized box.
    pub fn check_membrane_domains(&self, x: &[Vec3]) -> Result<(), Refusal> {
        for (i, e) in self.membranes.iter().enumerate() {
            if let Some(domain) = self.membrane_domain(i) {
                let strain = e.strain(x);
                if !domain.contains(strain) {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "membrane element {i}: physical Green strain {strain:?} is outside fitted ranges {:?}",
                            domain.ranges()
                        ),
                    ));
                }
            }
            if let Some(law) = self.membrane_response(i) {
                let strain = e.strain(x);
                if law.area_ratio_range().is_some() {
                    law.check_domain(strain)?;
                }
                if !law.contains(strain) {
                    let ranges = law.strain_range();
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!(
                            "membrane element {i}: physical Green strain {strain:?} is outside characterized ranges {ranges:?}"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Elastic energy Ψ (membrane + bending + elastics) at positions `x`.
    #[must_use]
    pub fn elastic_energy(&self, x: &[Vec3]) -> f64 {
        if self.check_membrane_domains(x).is_err() {
            return f64::NAN;
        }
        self.elastic_energy_with_fields(x, None, None)
    }

    // History fields supply WHOLE contributions, avoiding subtraction of large elastic energies
    // or double-counting yielded material. Memoryless summation stays bit-identical.
    fn elastic_energy_with_fields(
        &self,
        x: &[Vec3],
        panels: Option<f64>,
        membranes: Option<f64>,
    ) -> f64 {
        let m: f64 = membranes.unwrap_or_else(|| {
            self.membranes
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    self.membrane_response(i).map_or_else(
                        || self.membrane_energy(i, x),
                        |law| e.response_energy(x, law, &self.material(i)),
                    )
                })
                .sum()
        });
        let mut b: f64 = (0..self.hinges.len())
            .map(|i| self.hinge_energy(i, x))
            .sum();
        if let Some(field) = &self.panel_bending {
            b += panels.unwrap_or_else(|| field.patches.iter().map(|p| p.energy(x)).sum::<f64>());
        }
        let t: f64 = self.elastics.iter().map(|e| e.energy(x)).sum();
        m + b + t
    }

    // Liquid changes inertia only. Material histories continue under exactly the admitted laws.
    pub(crate) fn same_liquid_mechanics(&self, original: &Self) -> bool {
        self.rest == original.rest
            && self.membranes == original.membranes
            && self.hinges == original.hinges
            && self.membrane == original.membrane
            && self.compression == original.compression
            && self.bending == original.bending
            && self.bending_model == original.bending_model
            && self.compression_model == original.compression_model
            && self.elastics == original.elastics
            && self.element_materials == original.element_materials
            && self.element_responses == original.element_responses
            && self.element_domains == original.element_domains
            && self.hinge_bending == original.hinge_bending
            && self.hinge_support == original.hinge_support
            && self.panel_bending == original.panel_bending
    }
}

/// What one minimisation did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct SolveReport {
    /// Newton iterations taken.
    pub iterations: usize,
    /// Potential before and after.
    pub energy_start: f64,
    /// Potential after.
    pub energy_end: f64,
    /// Largest Newton step component in the last iteration (m).
    pub last_step: f64,
    /// Largest free gradient component at the end (N).
    pub residual: f64,
    /// Whether the convergence criterion was met.
    pub converged: bool,
    /// Line searches that hit their floor.
    pub line_search_failures: usize,
    /// Iterations whose exact Hessian was indefinite, so element blocks were projected to PSD.
    pub projected_iterations: usize,
    /// Sparse LDLᵀ factorizations (the dominant cost: one per iteration, two where projection begins).
    pub factorizations: usize,
    /// Iterations whose step the CCD bound cut below a full Newton step.
    pub ccd_limited: usize,
    /// Line-search halvings, summed.
    pub halvings: usize,
    /// Full steps admitted by approximate Wolfe slopes when energy differences are unresolved.
    pub roundoff_steps: usize,
    /// Preconditioned conjugate-gradient iterations, failed attempts included.
    pub pcg_iterations: usize,
}

// Hager–Zhang, SIAM J. Optim. 16 (2005), §4, (4.1) and T2. This is an approximate
// line-search test, not convergence. Limit its use to a full step with an unresolved predicted
// decrease, finite strictly improving free gradient, and a roundoff-scale energy discrepancy.
// https://people.clas.ufl.edu/hager/files/cg_descent.pdf
fn roundoff_step(
    energy: f64,
    trial_energy: f64,
    alpha: f64,
    slope: f64,
    trial_slope: f64,
    residual: f64,
    trial_residual: f64,
) -> bool {
    let uncertainty = 16.0 * f64::EPSILON * energy.abs().max(trial_energy.abs());
    [
        energy,
        trial_energy,
        slope,
        trial_slope,
        residual,
        trial_residual,
    ]
    .iter()
    .all(|v| v.is_finite())
        && alpha == 1.0
        && slope < 0.0
        && -slope <= uncertainty
        && (trial_energy - energy).abs() <= uncertainty
        && trial_slope >= 0.9 * slope
        && trial_slope <= (2.0 * 1e-4 - 1.0) * slope
        && trial_residual < residual
}

/// What [`Solver::settle`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettleReport {
    /// Time steps taken.
    pub steps: usize,
    /// Simulated time (s).
    pub time: f64,
    /// Final step size (s).
    pub h: f64,
    /// Largest vertex speed after the last step (m/s).
    pub max_speed: f64,
    /// Whether the speed stayed below the threshold for the required number of steps.
    pub settled: bool,
    /// Newton iterations over all steps.
    pub newton_iterations: usize,
}

/// Nonlinear solver strategy for [`Solver`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NewtonStrategy {
    /// Projected Newton with exact LDLᵀ (or cached-factor PCG when `pcg_iters > 0`) and Armijo
    /// backtracking. Default: preserves bit-identical legacy behavior.
    #[default]
    ProjectedNewton,
    /// Inexact Newton with Eisenstat–Walker adaptive forcing and zero-fill `ILU(0)` (with
    /// geometric diagonal-shift fallback) preconditioned CG before falling back to exact LDLᵀ.
    AdaptiveIlu {
        /// Maximum ILU-PCG iterations attempted per Newton step before factorizing.
        pcg_iters: usize,
        /// Maximum geometric diagonal shifts allowed on `ILU(0)` pivot breakdown.
        max_shifts: usize,
    },
    /// Trust-region Newton using Steihaug–Toint truncated CG directly on the unprojected exact
    /// Hessian, escaping negative curvature (buckling / wrinkling saddles) along the trust-region
    /// boundary while honoring contact CCD and material strain domains.
    TrustRegionSteihaug {
        /// Initial Euclidean trust-region radius (m).
        initial_radius: f64,
        /// Maximum trust-region radius (m).
        max_radius: f64,
        /// Minimum trust-region radius before refusing (m).
        min_radius: f64,
        /// Maximum CG matvecs per outer trust-region step (`0` chooses `2 * nfree.max(32)`).
        cg_iters: usize,
    },
}

impl NewtonStrategy {
    /// Default parameters for [`NewtonStrategy::AdaptiveIlu`].
    #[must_use]
    pub const fn adaptive_ilu_default() -> Self {
        Self::AdaptiveIlu {
            pcg_iters: 64,
            max_shifts: 4,
        }
    }

    /// Default parameters for [`NewtonStrategy::TrustRegionSteihaug`].
    #[must_use]
    pub const fn trust_region_default() -> Self {
        Self::TrustRegionSteihaug {
            initial_radius: 0.05,
            max_radius: 0.5,
            min_radius: 1e-12,
            cg_iters: 0,
        }
    }

    /// Whether this is the legacy default [`NewtonStrategy::ProjectedNewton`].
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::ProjectedNewton)
    }

    /// Validate strategy parameters before meshing or solving.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on invalid trust-region radii or zero `AdaptiveIlu` CG cap.
    pub fn validate(&self) -> Result<(), Refusal> {
        match *self {
            Self::ProjectedNewton => Ok(()),
            Self::AdaptiveIlu {
                pcg_iters,
                max_shifts,
            } => {
                if pcg_iters == 0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        "AdaptiveIlu pcg_iters must be positive",
                    ));
                }
                if max_shifts > 16 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        "AdaptiveIlu max_shifts must be at most 16",
                    ));
                }
                Ok(())
            }
            Self::TrustRegionSteihaug {
                initial_radius,
                max_radius,
                min_radius,
                ..
            } => {
                if !(initial_radius.is_finite()
                    && max_radius.is_finite()
                    && min_radius.is_finite()
                    && min_radius > 0.0
                    && initial_radius >= min_radius
                    && max_radius >= initial_radius)
                {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        "trust-region radii must be finite with 0 < min_radius <= initial_radius <= max_radius",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// The cloth state and the solver.
#[derive(Clone, Debug)]
pub struct Solver {
    /// The model.
    pub model: ClothModel,
    /// Positions (m).
    pub x: Vec<Vec3>,
    /// Velocities (m/s).
    pub v: Vec<Vec3>,
    /// Gravity (m/s²).
    pub gravity: Vec3,
    /// Newton tolerance on the step: ‖p‖∞/h for a time step, ‖p‖∞ for a static solve (m).
    pub tol: f64,
    /// Newton iteration cap per solve (300 by default for linearly converging contact modes).
    pub max_newton: usize,
    /// Contact with a static body and/or other cloth surfaces, if enabled.
    pub contact: Option<Contact>,
    /// Friction lag rounds per step after the first solve (IPC; 1 is usually enough).
    pub friction_rounds: usize,
    /// Inexact Newton: the iteration cap of conjugate gradients preconditioned by the last
    /// positive-definite factorization, tried before factorizing anew (0: factorize every iteration).
    /// A singular projected factorization can additionally use the bounded, unpreconditioned fallback.
    pub pcg_iters: usize,
    /// Nonlinear globalization and linear-solve strategy (default: [`NewtonStrategy::ProjectedNewton`]).
    pub newton_strategy: NewtonStrategy,
    /// Structural time-integration scheme (default: [`TimeIntegrator::ImplicitEuler`]).
    pub integrator: TimeIntegrator,
    /// Opt-in aerodynamic drag and SE(3)-invariant Rayleigh internal damping (default: disabled).
    pub damping: DampingOptions,
    /// Nodal accelerations (m/s²) retained by [`TimeIntegrator::GeneralizedAlpha`].
    a: Vec<Vec3>,
    /// Whether `a` has been initialized consistently with `M a₀ = M g − ∇Ψ(x₀)`.
    accel_initialized: bool,
    free: Vec<Option<usize>>,
    nfree: usize,
    symbolic: Option<SymbolicLdlt>,
    /// The Hessian pattern used by the current symbolic analysis.
    pattern: Option<Csr>,
    /// With self-contact: the free-DOF pattern of every element, elastic and mass block (and the
    /// membrane, hinge and elastic counts it was built for), which each assembly extends by its
    /// contact couplings.
    element_pattern: Option<(Csr, [usize; 4])>,
    cached_panel_count: usize,
    /// The last positive-definite factorization, the preconditioner for [`Solver::pcg_iters`].
    precond: Option<LdltFactor>,
    // Per-triangle, accepted physical memory; trial evaluations never mutate it.
    relaxation: Option<relaxation::Field>,
    // Per-source-panel permanent curvature. Accepted together with all other material memory.
    plasticity: Option<plasticity::Field>,
    // Permanent raw Green strain; replaces selected membrane energies and derivatives.
    membrane_plasticity: Option<membrane_plasticity::Field>,
    pub(crate) liquid_mass: Option<crate::liquid_mass::Binding>,
}

/// The relative residual an inexact dynamic Newton direction is solved to.
const PCG_TOL: f64 = 1e-4;

/// What potential and time-integration stencil `minimize` is solving.
#[derive(Clone, Copy)]
enum StepContext<'a> {
    Static,
    ImplicitEuler {
        xt: &'a [Vec3],
        h: f64,
        damping: Option<&'a DampingStep<'a>>,
    },
    GeneralizedAlpha {
        x0: &'a [Vec3],
        xt: &'a [Vec3],
        h: f64,
        params: GeneralizedAlphaParams,
        damping: Option<&'a DampingStep<'a>>,
    },
}

impl<'a> StepContext<'a> {
    fn from_inertia(inertia: Option<(&'a [Vec3], f64)>) -> Self {
        match inertia {
            None => Self::Static,
            Some((xt, h)) => Self::ImplicitEuler {
                xt,
                h,
                damping: None,
            },
        }
    }

    fn is_dynamic(self) -> bool {
        !matches!(self, Self::Static)
    }

    fn time_step(self) -> Option<f64> {
        match self {
            Self::Static => None,
            Self::ImplicitEuler { h, .. } | Self::GeneralizedAlpha { h, .. } => Some(h),
        }
    }

    fn damping(self) -> Option<&'a DampingStep<'a>> {
        match self {
            Self::Static => None,
            Self::ImplicitEuler { damping, .. } | Self::GeneralizedAlpha { damping, .. } => damping,
        }
    }

    fn scale(self) -> f64 {
        self.time_step().unwrap_or(1.0)
    }
}

/// Where an assembly's entries go: straight into the fixed pattern once it is known, else a triplet
/// buffer (the first assembly). Both give the same matrix bit for bit ([`CsrFill`]).
enum Sink {
    Fill(CsrFill),
    Coo(Coo),
}

impl Sink {
    fn push(&mut self, row: usize, col: usize, val: f64) {
        match self {
            Sink::Fill(f) => f.add(row, col, val),
            Sink::Coo(c) => c.push(row, col, val),
        }
    }

    fn finish(self) -> Csr {
        match self {
            Sink::Fill(f) => f.finish(),
            Sink::Coo(c) => c.assemble(),
        }
    }
}

impl Solver {
    pub(crate) fn liquid_history_kinds(&self) -> [bool; 3] {
        [
            self.relaxation.is_some(),
            self.plasticity.is_some(),
            self.membrane_plasticity.is_some(),
        ]
    }

    pub(crate) fn invalidate_mass_preconditioner(&mut self) {
        self.precond = None;
    }

    /// A solver at positions `x` (at rest velocity) with per-DOF constraints `fixed[i][k]` (a fixed
    /// component keeps its current value).
    ///
    /// # Panics
    /// If `x` or `fixed` does not have one entry per vertex.
    #[must_use]
    pub fn new(model: ClothModel, x: Vec<Vec3>, fixed: &[[bool; 3]], gravity: Vec3) -> Self {
        let n = model.rest.len();
        assert!(
            x.len() == n && fixed.len() == n,
            "one position and one constraint per vertex"
        );
        let mut free = vec![None; 3 * n];
        let mut nfree = 0;
        for i in 0..n {
            for k in 0..3 {
                if !fixed[i][k] {
                    free[3 * i + k] = Some(nfree);
                    nfree += 1;
                }
            }
        }
        let cached_panel_count = model.panel_bending_patches().len();
        Self {
            v: vec![Vec3::ZERO; n],
            a: vec![Vec3::ZERO; n],
            accel_initialized: false,
            model,
            x,
            gravity,
            tol: 1e-6,
            max_newton: 300,
            contact: None,
            friction_rounds: 1,
            pcg_iters: 0,
            newton_strategy: NewtonStrategy::ProjectedNewton,
            integrator: TimeIntegrator::ImplicitEuler,
            damping: DampingOptions::default(),
            free,
            nfree,
            symbolic: None,
            pattern: None,
            element_pattern: None,
            cached_panel_count,
            precond: None,
            relaxation: None,
            plasticity: None,
            membrane_plasticity: None,
            liquid_mass: None,
        }
    }

    /// Adds contact with a static body. The cloth must start clear of it (IPC needs an
    /// intersection-free start). Replacing a collider retains source-bound sheet assignments and
    /// self-contact enablement, but resets friction history. Changed cloth topology requires rebinding.
    ///
    /// # Errors
    /// [`RefusalCode::IntersectingStart`] when some cloth–body gap is not positive, an edge of either
    /// mesh passes through the other, or a piece lies inside a closed body;
    /// [`RefusalCode::NonFinite`] for a non-finite position; a refusal for stale material topology.
    pub fn with_contact(mut self, body: Body, params: ContactParams) -> Result<Self, Refusal> {
        params.validate()?;
        let self_params = self.contact.as_ref().and_then(Contact::self_contact_params);
        if self.x.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth has a non-finite position",
            ));
        }
        let tris: Vec<[usize; 3]> = self.model.membranes.iter().map(|e| e.v).collect();
        let lengths: std::collections::BTreeMap<(usize, usize), f64> = self
            .model
            .membranes
            .iter()
            .flat_map(|e| {
                let l = e.rest_edge_lengths();
                (0..3).map(move |k| {
                    let (a, b) = (e.v[k], e.v[(k + 1) % 3]);
                    ((a.min(b), a.max(b)), l[k])
                })
            })
            .collect();
        let contact = Contact::new(body, params, tris, |a, b| {
            lengths.get(&(a.min(b), a.max(b))).copied().unwrap_or(1e-3)
        })
        .retain_surface_materials(self.contact.as_ref(), &self.x)?;
        if let Some((g, pair, at)) = contact.closest_pair(&self.x, params.dhat)
            && (g.is_nan() || g <= 0.0)
        {
            return Err(Refusal::new(
                RefusalCode::IntersectingStart,
                format!(
                    "the cloth starts {:.3} mm inside the body's offset surface ({pair:?} near \
                     ({:.1}, {:.1}, {:.1}) mm)",
                    -g * 1e3,
                    at.x * 1e3,
                    at.y * 1e3,
                    at.z * 1e3
                ),
            )
            .repair("place the pieces further out (a larger push-out or clearance)")
            .repair("reduce the cloth thickness offset"));
        }
        if let Some(i) = contact.first_intersection(&self.x) {
            let at = match i {
                Intersection::ClothEdge { ce, .. } => {
                    let [a, b] = contact.cloth.edges[ce];
                    (self.x[a] + self.x[b]) * 0.5
                }
                Intersection::BodyEdge { ct, .. } => {
                    let t = contact.cloth.triangles[ct];
                    (self.x[t[0]] + self.x[t[1]] + self.x[t[2]]) / 3.0
                }
                Intersection::Inside { cv } => self.x[cv],
            };
            return Err(Refusal::new(
                RefusalCode::IntersectingStart,
                format!(
                    "the cloth crosses the body at the start: {i:?} near ({:.1}, {:.1}, {:.1}) mm",
                    at.x * 1e3,
                    at.y * 1e3,
                    at.z * 1e3
                ),
            )
            .repair("place the pieces outside the body (a larger push-out or clearance)"));
        }
        self.contact = Some(match self_params {
            Some(p) => contact.with_self_contact(&self.x, p)?,
            None => contact,
        });
        self.symbolic = None;
        self.pattern = None;
        self.precond = None;
        self.element_pattern = self
            .dynamic_contact()
            .then(|| self.element_blocks_pattern());
        Ok(self)
    }

    /// Enables cloth–cloth contact, with or without a body collider. `params.thickness` is the sum
    /// of the two cloth half-thicknesses; body and self-contact friction remain independent. All
    /// non-incident point–triangle and edge–edge pairs participate, including across separate pieces.
    ///
    /// # Errors
    /// Invalid parameters, non-finite geometry, or [`RefusalCode::IntersectingStart`] when the
    /// initial cloth layers intersect or touch their thickness offsets.
    pub fn with_self_contact(mut self, params: ContactParams) -> Result<Self, Refusal> {
        params.validate()?;
        if self.contact.is_none() {
            self = self.with_contact(Body::empty(), params)?;
        }
        self.enable_self_contact_observed(params, &mut None)?;
        Ok(self)
    }

    // When scene admission fails, keep the rejected positions for diagnostic rendering.
    pub(crate) fn enable_self_contact_observed(
        &mut self,
        params: ContactParams,
        crossing: &mut Option<[usize; 2]>,
    ) -> Result<(), Refusal> {
        let contact = self.contact.take().ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "missing cloth contact topology",
            )
        })?;
        self.contact = Some(contact.with_self_contact_observed(&self.x, params, crossing)?);
        self.symbolic = None;
        self.pattern = None;
        self.precond = None;
        self.element_pattern = Some(self.element_blocks_pattern());
        Ok(())
    }

    fn dynamic_contact(&self) -> bool {
        self.contact
            .as_ref()
            .is_some_and(|c| c.self_contact_params().is_some())
    }

    fn element_counts(&self) -> [usize; 4] {
        let m = &self.model;
        [
            m.membranes.len(),
            m.hinges.len(),
            m.elastics.len(),
            m.panel_bending_patches().len(),
        ]
    }

    /// The positions [`Solver::assemble`] pushes before any contact block: every pair of free DOFs of
    /// each membrane, hinge and elastic, and each free DOF's mass.
    fn element_blocks_pattern(&self) -> (Csr, [usize; 4]) {
        let mut coo = Coo::new(self.nfree, self.nfree);
        let mut block = |vs: &[usize]| {
            let dofs: Vec<usize> = vs
                .iter()
                .flat_map(|&v| (0..3).filter_map(move |k| self.free[3 * v + k]))
                .collect();
            for &a in &dofs {
                for &b in &dofs {
                    coo.push(a, b, 0.0);
                }
            }
        };
        self.model.membranes.iter().for_each(|e| block(&e.v));
        self.model.hinges.iter().for_each(|h| block(&h.v));
        self.model
            .panel_bending_patches()
            .iter()
            .for_each(|p| block(&p.v));
        self.model.elastics.iter().for_each(|e| block(&e.v));
        for f in self.free.iter().flatten() {
            coo.push(*f, *f, 0.0);
        }
        (coo.assemble(), self.element_counts())
    }

    fn audit_self_contact(&self) -> Result<(), Refusal> {
        match &self.contact {
            Some(c) => c.audit_self_contact(&self.x),
            None => Ok(()),
        }
    }

    /// Free degrees of freedom.
    #[must_use]
    pub fn free_dofs(&self) -> usize {
        self.nfree
    }

    /// Current nodal accelerations (m/s²) tracked by [`TimeIntegrator::GeneralizedAlpha`].
    #[must_use]
    pub fn acceleration(&self) -> &[Vec3] {
        &self.a
    }

    /// Explicitly set nodal accelerations (m/s²) and mark them initialized (bypassing automatic
    /// consistent-acceleration initialization on the next [`TimeIntegrator::GeneralizedAlpha`] step).
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] on length mismatch; [`RefusalCode::NonFinite`] if any
    /// entry is non-finite.
    pub fn set_acceleration(&mut self, a: Vec<Vec3>) -> Result<(), Refusal> {
        if a.len() != self.x.len() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "acceleration length {} must match vertex count {}",
                    a.len(),
                    self.x.len()
                ),
            ));
        }
        if a.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "nodal accelerations must be finite",
            ));
        }
        self.a = a;
        self.accel_initialized = true;
        Ok(())
    }

    /// Clear tracked accelerations so the next [`TimeIntegrator::GeneralizedAlpha`] step
    /// recomputes a consistent initial acceleration `M a₀ = M g − ∇Ψ(x₀)`. Call after changing
    /// gravity, external loads, or positions discontinuously between phases.
    pub fn reset_acceleration(&mut self) {
        self.a.fill(Vec3::ZERO);
        self.accel_initialized = false;
    }

    /// Exact static nodal residual `R_i(x) = ∂Ψ/∂x_i − m_i g` (N) on every vertex (free and
    /// fixed). At a converged static equilibrium `R_i ≈ 0` on free DOFs, and `R_i` on fixed DOFs
    /// is the exact external support reaction force required to hold those Dirichlet constraints.
    #[must_use]
    pub fn nodal_reactions(&self) -> Vec<Vec3> {
        let mut r = vec![Vec3::ZERO; self.x.len()];
        let add_local = |r: &mut [Vec3], dofs: &[usize], grad: &[f64]| {
            for (k, &v) in dofs.iter().enumerate() {
                r[v].x += grad[3 * k];
                r[v].y += grad[3 * k + 1];
                r[v].z += grad[3 * k + 2];
            }
        };
        for (i, e) in self.model.membranes.iter().enumerate() {
            let mut l = self
                .membrane_plasticity
                .as_ref()
                .and_then(|field| field.local(i, &self.x, false))
                .unwrap_or_else(|| {
                    self.model.membrane_response(i).map_or_else(
                        || self.model.membrane_local(i, &self.x),
                        |law| e.response_local(&self.x, law, &self.model.material(i)),
                    )
                });
            if let Some(extra) = self
                .relaxation
                .as_ref()
                .and_then(|field| field.local(i, &self.x, None))
            {
                for (a, b) in l.grad.iter_mut().zip(extra.grad) {
                    *a += b;
                }
            }
            add_local(&mut r, &e.v, &l.grad);
        }
        for (i, h) in self.model.hinges.iter().enumerate() {
            let l = self.model.hinge_local(i, &self.x);
            add_local(&mut r, &h.v, &l.grad);
        }
        for (i, p) in self.model.panel_bending_patches().iter().enumerate() {
            let l = self
                .plasticity
                .as_ref()
                .and_then(|field| field.local(i, &self.x, false))
                .unwrap_or_else(|| p.local(&self.x));
            add_local(&mut r, &p.v, &l.grad);
        }
        for e in &self.model.elastics {
            let l = e.local(&self.x);
            add_local(&mut r, &e.v, &l.grad);
        }
        for (i, &m) in self.model.mass.iter().enumerate() {
            r[i] -= self.gravity * m;
        }
        if let Some(c) = &self.contact {
            for block in c.terms(&self.x, true, false).blocks {
                for (ia, &(va, ka)) in block.dofs.iter().enumerate() {
                    match ka {
                        0 => r[va].x += block.grad[ia],
                        1 => r[va].y += block.grad[ia],
                        _ => r[va].z += block.grad[ia],
                    }
                }
            }
        }
        r
    }

    /// Matrix-vector product `H_{FD} c_D = (∂R_D / ∂x_F)ᵀ c_D` mapping Dirichlet (fixed) DOF
    /// weights `c_fixed` into free-DOF adjoint RHS entries at `self.x`.
    pub(crate) fn fixed_to_free_hessian_mul(&self, c_fixed: &[Vec3]) -> Vec<f64> {
        let mut out = vec![0.0; self.nfree];
        let mut accum = |dofs: &[usize], hess: &[f64]| {
            let n = 3 * dofs.len();
            for a in 0..n {
                let Some(fa) = self.free[3 * dofs[a / 3] + a % 3] else {
                    continue;
                };
                for b in 0..n {
                    let vb = dofs[b / 3];
                    let kb = b % 3;
                    if self.free[3 * vb + kb].is_none() {
                        let w = c_fixed[vb][kb];
                        if w != 0.0 {
                            out[fa] += hess[a * n + b] * w;
                        }
                    }
                }
            }
        };
        for (i, e) in self.model.membranes.iter().enumerate() {
            let l = self.model.membrane_local(i, &self.x);
            accum(&e.v, &l.hess);
        }
        for (i, h) in self.model.hinges.iter().enumerate() {
            let l = self.model.hinge_local(i, &self.x);
            accum(&h.v, &l.hess);
        }
        for e in &self.model.elastics {
            let l = e.local(&self.x);
            accum(&e.v, &l.hess);
        }
        if let Some(c) = &self.contact {
            for block in c.terms(&self.x, true, false).blocks {
                let m = block.dofs.len();
                for (ia, &(va, ka)) in block.dofs.iter().enumerate() {
                    let Some(fa) = self.free[3 * va + ka] else {
                        continue;
                    };
                    for (ib, &(vb, kb)) in block.dofs.iter().enumerate() {
                        if self.free[3 * vb + kb].is_none() {
                            let w = c_fixed[vb][kb];
                            if w != 0.0 {
                                out[fa] += block.hess[ia * m + ib] * w;
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub(crate) fn is_fixed_dof(&self, vertex: usize, axis: usize) -> bool {
        self.free[3 * vertex + axis].is_none()
    }

    pub(crate) fn check_integrator_history(&self) -> Result<(), Refusal> {
        self.damping.validate()?;
        if self.integrator != TimeIntegrator::ImplicitEuler
            && (self.relaxation.is_some()
                || self.plasticity.is_some()
                || self.membrane_plasticity.is_some())
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Generalized-alpha integration is not admitted for backward-Euler material-history fields; use TimeIntegrator::ImplicitEuler",
            ));
        }
        if self.model.compression_model == CompressionModel::Principal
            && (self.model.element_responses.is_some()
                || self.relaxation.is_some()
                || self.membrane_plasticity.is_some())
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "CompressionModel::Principal is not admitted alongside calibrated nonlinear or history membrane laws, which operate on axis-aligned material-frame strains",
            ));
        }
        Ok(())
    }

    pub(crate) fn pack_free(&self, v: &[Vec3]) -> Vec<f64> {
        let mut out = vec![0.0; self.nfree];
        for (i, p) in v.iter().enumerate() {
            for k in 0..3 {
                if let Some(f) = self.free[3 * i + k] {
                    out[f] = p[k];
                }
            }
        }
        out
    }

    pub(crate) fn unpack_free(&self, free_vec: &[f64]) -> Vec<Vec3> {
        self.displacement(free_vec)
    }

    pub(crate) fn assemble_static_exact(&self) -> (Vec<f64>, Csr) {
        self.assemble_ctx(&self.x, StepContext::Static, false, false)
    }

    pub(crate) fn cached_factor_solve(&self, rhs: &[f64]) -> Option<Vec<f64>> {
        self.precond
            .as_ref()
            .filter(|f| f.stats().n == rhs.len())
            .map(|f| f.solve(rhs))
    }

    pub(crate) fn factor_and_solve_exact(
        &mut self,
        h: &Csr,
        rhs: &[f64],
    ) -> Result<Vec<f64>, Refusal> {
        let sym = self.symbolic(h)?;
        let factor = sym.factor(h, &LdltOptions::default()).map_err(|e| {
            Refusal::new(
                RefusalCode::NotConverged,
                format!("exact tangent factorization failed: {e}"),
            )
        })?;
        let sol = factor.solve(rhs);
        if factor.inertia().negative == 0 {
            self.precond = Some(factor);
        } else {
            self.precond = None;
        }
        Ok(sol)
    }

    pub(crate) fn is_homogeneous_elastic_for_adjoint(&self) -> bool {
        !self.model.is_heterogeneous()
            && self.model.bending_model.is_default()
            && self.model.compression_model.is_default()
            && self.damping.is_default()
            && self.model.element_materials.is_none()
            && self.model.element_responses.is_none()
            && self.model.hinge_bending.is_none()
            && self.model.panel_bending.is_none()
            && self.relaxation.is_none()
            && self.plasticity.is_none()
            && self.membrane_plasticity.is_none()
            && !self.contact.as_ref().is_some_and(|c| c.has_friction())
    }

    /// Test-only helper matching the legacy static / implicit-Euler potential signature.
    #[cfg(test)]
    fn potential(&self, x: &[Vec3], inertia: Option<(&[Vec3], f64)>) -> Result<f64, Refusal> {
        self.potential_ctx(x, StepContext::from_inertia(inertia))
    }

    /// The potential for the active step context (static, implicit Euler, or Generalized-α).
    fn potential_ctx(&self, x: &[Vec3], ctx: StepContext<'_>) -> Result<f64, Refusal> {
        match ctx {
            StepContext::Static => self.potential_legacy(x, None, None),
            StepContext::ImplicitEuler { xt, h, damping } => {
                self.potential_legacy(x, Some((xt, h)), damping)
            }
            StepContext::GeneralizedAlpha {
                x0,
                xt,
                h,
                params,
                damping,
            } => {
                self.model.check_membrane_domains(x)?;
                self.model.check_panel_bending_domains(x)?;
                let x_eval: Vec<Vec3> = x
                    .iter()
                    .zip(x0)
                    .map(|(&xn, &xo)| params.eval_pos(xn, xo))
                    .collect();
                self.model.check_membrane_domains(&x_eval)?;
                self.model.check_panel_bending_domains(&x_eval)?;
                let panels = self
                    .plasticity
                    .as_ref()
                    .map(|field| field.energy(&self.model, &x_eval, true))
                    .transpose()?;
                let membranes = self
                    .membrane_plasticity
                    .as_ref()
                    .map(|field| field.energy(&self.model, &x_eval, true))
                    .transpose()?;
                let mut e = self
                    .model
                    .elastic_energy_with_fields(&x_eval, panels, membranes);
                if let Some(field) = &self.relaxation {
                    e += field.energy(&x_eval, Some(h))?;
                }
                e /= params.elastic_weight();
                // Contact barrier and lagged friction enforce end-of-step non-penetration at x_{n+1}
                // so the barrier blows up on the exact CCD boundary d(x_{n+1}) -> 0 while elastic
                // internal forces retain 2nd-order Chung-Hulbert midpoint evaluation at x_{n+1-α_f}.
                if let Some(c) = &self.contact {
                    e += c.terms(x, false, true).energy;
                }
                if let Some(d) = damping {
                    e += d.energy(&self.model, x);
                }
                let cm = params.mass_coeff(h);
                for (i, m) in self.model.mass.iter().enumerate() {
                    e += 0.5 * cm * m * (x[i] - xt[i]).norm2();
                }
                if e.is_finite() {
                    Ok(e)
                } else {
                    Err(Refusal::new(
                        RefusalCode::NonFinite,
                        "cloth potential is not finite",
                    ))
                }
            }
        }
    }

    fn potential_legacy(
        &self,
        x: &[Vec3],
        inertia: Option<(&[Vec3], f64)>,
        damping: Option<&DampingStep<'_>>,
    ) -> Result<f64, Refusal> {
        self.model.check_membrane_domains(x)?;
        self.model.check_panel_bending_domains(x)?;
        let panels = self
            .plasticity
            .as_ref()
            .map(|field| field.energy(&self.model, x, inertia.is_some()))
            .transpose()?;
        let membranes = self
            .membrane_plasticity
            .as_ref()
            .map(|field| field.energy(&self.model, x, inertia.is_some()))
            .transpose()?;
        let mut e = self.model.elastic_energy_with_fields(x, panels, membranes);
        if let Some(field) = &self.relaxation {
            e += field.energy(x, inertia.map(|(_, h)| h))?;
        }
        match inertia {
            Some((xt, h)) => {
                for (i, m) in self.model.mass.iter().enumerate() {
                    e += 0.5 * m * (x[i] - xt[i]).norm2() / (h * h);
                }
            }
            None => {
                for (i, m) in self.model.mass.iter().enumerate() {
                    e -= m * self.gravity.dot(x[i]);
                }
            }
        }
        if let Some(c) = &self.contact {
            e += c.terms(x, false, inertia.is_some()).energy;
        }
        if let Some(d) = damping {
            e += d.energy(&self.model, x);
        }
        if e.is_finite() {
            Ok(e)
        } else {
            Err(Refusal::new(
                RefusalCode::NonFinite,
                "cloth potential is not finite",
            ))
        }
    }

    /// Test-only helper matching the legacy static / implicit-Euler assembly signature.
    #[cfg(test)]
    fn assemble(
        &self,
        x: &[Vec3],
        inertia: Option<(&[Vec3], f64)>,
        project: bool,
    ) -> (Vec<f64>, Csr) {
        self.assemble_ctx(x, StepContext::from_inertia(inertia), project, true)
    }

    fn assemble_ctx(
        &self,
        x: &[Vec3],
        ctx: StepContext<'_>,
        project_elements: bool,
        project_contact: bool,
    ) -> (Vec<f64>, Csr) {
        let x_eval_buf = match ctx {
            StepContext::GeneralizedAlpha { x0, params, .. } => Some(
                x.iter()
                    .zip(x0)
                    .map(|(&xn, &xo)| params.eval_pos(xn, xo))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let x_el = x_eval_buf.as_deref().unwrap_or(x);
        let dynamic = ctx.is_dynamic();
        let hess_scale = match ctx {
            StepContext::GeneralizedAlpha { params, .. } => params.elastic_weight(),
            _ => 1.0,
        };
        let mut g = vec![0.0; self.nfree];
        // The contact blocks (evaluated at endpoint `x`) come first so their sparsity positions
        // are known before element blocks are pushed; they are still scattered last.
        let blocks = self
            .contact
            .as_ref()
            .map(|c| c.terms(x, true, dynamic).blocks);
        // Cloth–cloth contact can couple previously unrelated elements. Never scatter those blocks
        // into a stale fixed pattern (CsrFill rejects missing entries): the element pattern plus this
        // iterate's contact couplings is the structure the triplets would assemble to, filled in place
        // in the same order (bit for bit Coo's matrix). Body-only solves fill the analysed pattern.
        let mut sink = if let (Some(p), false) = (&self.pattern, self.dynamic_contact()) {
            Sink::Fill(CsrFill::new(p))
        } else if let Some((elements, counts)) = &self.element_pattern
            && *counts == self.element_counts()
            && self.dynamic_contact()
        {
            let mut coupled = Vec::new();
            for block in blocks.iter().flatten() {
                let free: Vec<usize> = block
                    .dofs
                    .iter()
                    .filter_map(|&(v, k)| self.free[3 * v + k])
                    .collect();
                for &a in &free {
                    coupled.extend(free.iter().map(|&b| (a, b)));
                }
            }
            Sink::Fill(CsrFill::new(&elements.with_entries(&mut coupled)))
        } else {
            Sink::Coo(Coo::new(self.nfree, self.nfree))
        };
        // Elements whose Hessian is PSD by construction (every hinge; membranes without compression
        // or a negative stress) are not projected: it would change nothing but rounding. The others
        // depend on their vertices only through differences, so they project on the complement of
        // the translations.
        let scatter = |dofs: &[usize],
                       grad: &[f64],
                       hess: &mut [f64],
                       psd: bool,
                       scale: f64,
                       sink: &mut Sink,
                       g: &mut [f64]| {
            let n = 3 * dofs.len();
            if project_elements && !psd {
                project_psd_translation_free(hess, dofs.len());
            }
            for a in 0..n {
                let Some(fa) = self.free[3 * dofs[a / 3] + a % 3] else {
                    continue;
                };
                g[fa] += grad[a];
                for b in 0..n {
                    if let Some(fb) = self.free[3 * dofs[b / 3] + b % 3] {
                        let val = hess[a * n + b];
                        sink.push(fa, fb, if scale != 1.0 { val * scale } else { val });
                    }
                }
            }
        };
        for (i, e) in self.model.membranes.iter().enumerate() {
            let mut l = self
                .membrane_plasticity
                .as_ref()
                .and_then(|field| field.local(i, x_el, dynamic))
                .unwrap_or_else(|| {
                    self.model.membrane_response(i).map_or_else(
                        || self.model.membrane_local(i, x_el),
                        |law| e.response_local(x_el, law, &self.model.material(i)),
                    )
                });
            if let Some(extra) = self
                .relaxation
                .as_ref()
                .and_then(|field| field.local(i, x_el, ctx.time_step()))
            {
                l.energy += extra.energy;
                for (a, b) in l.grad.iter_mut().zip(extra.grad) {
                    *a += b;
                }
                for (a, b) in l.hess.iter_mut().zip(extra.hess) {
                    *a += b;
                }
                l.psd &= extra.psd;
            }
            scatter(
                &e.v,
                &l.grad,
                &mut l.hess,
                l.psd,
                hess_scale,
                &mut sink,
                &mut g,
            );
        }
        for (i, h) in self.model.hinges.iter().enumerate() {
            let mut l = self.model.hinge_local(i, x_el);
            scatter(
                &h.v,
                &l.grad,
                &mut l.hess,
                l.psd,
                hess_scale,
                &mut sink,
                &mut g,
            );
        }
        for (i, p) in self.model.panel_bending_patches().iter().enumerate() {
            let mut l = self
                .plasticity
                .as_ref()
                .and_then(|field| field.local(i, x_el, dynamic))
                .unwrap_or_else(|| p.local(x_el));
            scatter(
                &p.v,
                &l.grad,
                &mut l.hess,
                l.psd,
                hess_scale,
                &mut sink,
                &mut g,
            );
        }
        for e in &self.model.elastics {
            let mut l = e.local(x_el);
            scatter(
                &e.v,
                &l.grad,
                &mut l.hess,
                l.psd,
                hess_scale,
                &mut sink,
                &mut g,
            );
        }
        if let Some(damping) = ctx.damping() {
            for (i, e) in self.model.membranes.iter().enumerate() {
                if let Some(mut l) = damping.air_drag_local(i, e.v, x) {
                    scatter(&e.v, &l.grad, &mut l.hess, l.psd, 1.0, &mut sink, &mut g);
                }
                if let Some(mut l) = damping.rayleigh_membrane_local(&self.model, i, x) {
                    scatter(&e.v, &l.grad, &mut l.hess, l.psd, 1.0, &mut sink, &mut g);
                }
            }
            for (i, h) in self.model.hinges.iter().enumerate() {
                if let Some(mut l) = damping.rayleigh_bending_local(&self.model, i, x) {
                    scatter(&h.v, &l.grad, &mut l.hess, l.psd, 1.0, &mut sink, &mut g);
                }
            }
        }
        for (i, &m) in self.model.mass.iter().enumerate() {
            for k in 0..3 {
                let Some(f) = self.free[3 * i + k] else {
                    continue;
                };
                match ctx {
                    StepContext::ImplicitEuler { xt, h, .. } => {
                        g[f] += m * (x[i][k] - xt[i][k]) / (h * h);
                        sink.push(f, f, m / (h * h));
                    }
                    StepContext::GeneralizedAlpha { xt, h, params, .. } => {
                        let cm = params.mass_coeff(h);
                        g[f] += cm * m * (x[i][k] - xt[i][k]);
                        sink.push(f, f, cm * m);
                    }
                    StepContext::Static => {
                        g[f] -= m * self.gravity[k];
                        sink.push(f, f, 0.0);
                    }
                }
            }
        }
        if let Some(blocks) = blocks {
            // Mollified edge pairs carry their full, indefinite Hessian and are projected here; the
            // Gauss–Newton barrier and friction blocks are PSD already. Self-contact blocks may
            // couple all four moving vertices across previously unrelated elements.
            for mut block in blocks {
                let m = block.dofs.len();
                if project_contact && !block.psd {
                    project_psd(&mut block.hess, m);
                }
                for (ia, &(va, ka)) in block.dofs.iter().enumerate() {
                    let Some(fa) = self.free[3 * va + ka] else {
                        continue;
                    };
                    g[fa] += block.grad[ia];
                    for (ib, &(vb, kb)) in block.dofs.iter().enumerate() {
                        if let Some(fb) = self.free[3 * vb + kb] {
                            sink.push(fa, fb, block.hess[ia * m + ib]);
                        }
                    }
                }
            }
        }
        (g, sink.finish())
    }

    fn symbolic(&mut self, a: &Csr) -> Result<&SymbolicLdlt, Refusal> {
        if self.dynamic_contact()
            && self.pattern.as_ref().is_some_and(|p| {
                p.nrows() != a.nrows() || (0..a.nrows()).any(|r| p.row(r).0 != a.row(r).0)
            })
        {
            self.symbolic = None;
            self.pattern = None;
        }
        if self.symbolic.is_none() {
            // Order the vertex graph, then expand each free vertex's DOFs in place.
            let n = self.model.rest.len();
            let mut vcoo = Coo::new(n, n);
            let mut link = |vs: &[usize]| {
                for &a in vs {
                    for &b in vs {
                        vcoo.push(a, b, 1.0);
                    }
                }
            };
            self.model.membranes.iter().for_each(|e| link(&e.v));
            self.model.hinges.iter().for_each(|h| link(&h.v));
            self.model
                .panel_bending_patches()
                .iter()
                .for_each(|p| link(&p.v));
            if self.dynamic_contact() {
                // Order the actual contact graph too, not only the rest-mesh adjacency.
                let mut vertex = vec![0; self.nfree];
                for (dof, free) in self.free.iter().enumerate() {
                    if let Some(f) = free {
                        vertex[*f] = dof / 3;
                    }
                }
                for r in 0..a.nrows() {
                    for &c in a.row(r).0 {
                        vcoo.push(vertex[r], vertex[c], 1.0);
                    }
                }
            }
            for i in 0..n {
                vcoo.push(i, i, 1.0);
            }
            let order = amd_order(&vcoo.assemble());
            let free = &self.free;
            let perm: Vec<usize> = order
                .iter()
                .flat_map(|&v| (0..3).filter_map(move |k| free[3 * v + k]))
                .collect();
            let s = SymbolicLdlt::analyze_with_permutation(a, perm).map_err(|e| {
                Refusal::new(
                    RefusalCode::NotConverged,
                    format!("symbolic analysis failed: {e}"),
                )
            })?;
            self.symbolic = Some(s);
            // Body-only assemblies fill this in place; self-contact compares the next pattern.
            self.pattern = Some(a.clone());
        }
        self.symbolic
            .as_ref()
            .ok_or_else(|| Refusal::new(RefusalCode::NotConverged, "no symbolic analysis"))
    }

    /// Per-vertex displacement of a free-DOF vector (zero on fixed components).
    fn displacement(&self, p: &[f64]) -> Vec<Vec3> {
        (0..self.x.len())
            .map(|i| {
                Vec3::new(
                    self.free[3 * i].map_or(0.0, |f| p[f]),
                    self.free[3 * i + 1].map_or(0.0, |f| p[f]),
                    self.free[3 * i + 2].map_or(0.0, |f| p[f]),
                )
            })
            .collect()
    }

    fn apply(&self, x: &[Vec3], p: &[f64], alpha: f64) -> Vec<Vec3> {
        x.iter()
            .zip(self.displacement(p))
            .map(|(xi, d)| *xi + d * alpha)
            .collect()
    }

    fn ccd_step_bound(&self, p: &[f64]) -> f64 {
        self.contact
            .as_ref()
            .map_or(1.0, |c| c.step_bound(&self.x, &self.displacement(p)))
    }

    /// Projected Newton (or configured [`NewtonStrategy`]) on the potential from the current positions.
    fn minimize(&mut self, inertia: Option<(&[Vec3], f64)>) -> Result<SolveReport, Refusal> {
        self.minimize_ctx(StepContext::from_inertia(inertia))
    }

    fn minimize_ctx(&mut self, ctx: StepContext<'_>) -> Result<SolveReport, Refusal> {
        let count = self.model.panel_bending_patches().len();
        if count != self.cached_panel_count {
            self.symbolic = None;
            self.pattern = None;
            self.precond = None;
            self.element_pattern = None;
            self.cached_panel_count = count;
        }
        if let NewtonStrategy::TrustRegionSteihaug {
            initial_radius,
            max_radius,
            min_radius,
            cg_iters,
        } = self.newton_strategy
        {
            return self.minimize_trust_region(
                ctx,
                initial_radius,
                max_radius,
                min_radius,
                cg_iters,
            );
        }
        let mut rep = SolveReport {
            energy_start: self.potential_ctx(&self.x, ctx)?,
            ..SolveReport::default()
        };
        let mut energy = rep.energy_start;
        if self.nfree == 0 {
            rep.converged = true;
            rep.energy_end = energy;
            return Ok(rep);
        }
        let scale = ctx.scale();
        // A static Hessian can retain free rigid modes. A loose linear residual could hide an
        // unsupported load in those modes, even when a cached dynamic factor is positive definite.
        let base_linear_tol = if ctx.is_dynamic() {
            PCG_TOL
        } else {
            LdltOptions::default().zero_pivot_rel
        };
        let adaptive_ilu = match self.newton_strategy {
            NewtonStrategy::AdaptiveIlu {
                pcg_iters,
                max_shifts,
            } => Some((pcg_iters, max_shifts)),
            _ => None,
        };
        let eta_max = if self.contact.is_some() {
            PCG_TOL
        } else {
            1e-2
        };
        let mut prev_grad_l2: Option<f64> = None;
        let mut ew_eta = eta_max;
        // Once a solve has met negative inertia it keeps projecting: retrying the exact Hessian first
        // cost a second assembly and factorization on almost every iteration of a garment.
        let mut projecting = false;
        for it in 0..self.max_newton {
            let mut iter_lin_tol: Option<f64> = None;
            let mut next_lin_tol = |g_vec: &[f64]| -> f64 {
                if let Some(t) = iter_lin_tol {
                    return t;
                }
                let t = if adaptive_ilu.is_some() && ctx.is_dynamic() {
                    let gl2 = g_vec.iter().map(|v| v * v).sum::<f64>().sqrt();
                    if let Some(prev) = prev_grad_l2 {
                        ew_eta = eisenstat_walker(prev, gl2, ew_eta, 1e-8, eta_max, 0.9, 2.0);
                    }
                    prev_grad_l2 = Some(gl2);
                    ew_eta
                } else {
                    base_linear_tol
                };
                iter_lin_tol = Some(t);
                t
            };
            // Exact Hessian first (quadratic convergence near a minimum); element blocks projected to
            // PSD only when the factorization reports negative inertia (plan §11.2).
            let (mut g, mut a_mat, mut p, mut used_lin_tol) = if projecting {
                (Vec::new(), None, None, base_linear_tol)
            } else {
                let (g, a) = self.assemble_ctx(&self.x, ctx, false, true);
                let lin_tol = next_lin_tol(&g);
                let p = self.solve_newton_linear(
                    &a,
                    &g,
                    false,
                    lin_tol,
                    base_linear_tol,
                    adaptive_ilu,
                    &mut rep,
                )?;
                (g, Some(a), p, lin_tol)
            };
            let mut newton_correction = true;
            if p.is_none() {
                projecting = true;
                rep.projected_iterations += 1;
                let (g2, a2) = self.assemble_ctx(&self.x, ctx, true, true);
                g = g2;
                let lin_tol = next_lin_tol(&g);
                used_lin_tol = lin_tol;
                p = self.solve_newton_linear(
                    &a2,
                    &g,
                    true,
                    lin_tol,
                    base_linear_tol,
                    adaptive_ilu,
                    &mut rep,
                )?;
                if p.is_none() {
                    newton_correction = false;
                    if !ctx.is_dynamic() {
                        p = self.static_search_direction(&a2, &g, &mut rep)?;
                    }
                }
                a_mat = Some(a2);
            }
            // If neither bounded linear path succeeds, retain steepest descent.
            let mut p = p.unwrap_or_else(|| g.iter().map(|v| -v).collect());
            if g.iter().chain(&p).any(|v| !v.is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "Newton gradient or direction is non-finite",
                ));
            }
            let mut slope: f64 = g.iter().zip(&p).map(|(a, b)| a * b).sum();
            if slope.is_nan() || slope >= 0.0 {
                newton_correction = false;
                p = g.iter().map(|v| -v).collect();
                slope = -g.iter().map(|v| v * v).sum::<f64>();
            }
            let mut step = max_abs_or_nan(&p);
            rep.iterations = it + 1;
            rep.last_step = step;
            rep.residual = max_abs_or_nan(&g);
            // An inexact Eisenstat–Walker solve (used_lin_tol > base_linear_tol) is valid for
            // descent far from equilibrium, but cannot certify displacement convergence: stiff
            // membrane components can dominate ‖g‖₂ while leaving soft bending modes unsolved.
            // Re-solve at `base_linear_tol` before accepting convergence.
            if newton_correction
                && rep.residual > 0.0
                && step / scale < self.tol
                && used_lin_tol > base_linear_tol
                && let Some(a_cur) = a_mat.take()
            {
                ew_eta = base_linear_tol;
                let mut p_tight = self.solve_newton_linear(
                    &a_cur,
                    &g,
                    projecting,
                    base_linear_tol,
                    base_linear_tol,
                    adaptive_ilu,
                    &mut rep,
                )?;
                if p_tight.is_none() && !projecting {
                    projecting = true;
                    rep.projected_iterations += 1;
                    let (g2, a2) = self.assemble_ctx(&self.x, ctx, true, true);
                    g = g2;
                    if g.iter().any(|v| !v.is_finite()) {
                        return Err(Refusal::new(
                            RefusalCode::NonFinite,
                            "Newton gradient is non-finite",
                        ));
                    }
                    rep.residual = max_abs_or_nan(&g);
                    p_tight = self.solve_newton_linear(
                        &a2,
                        &g,
                        true,
                        base_linear_tol,
                        base_linear_tol,
                        adaptive_ilu,
                        &mut rep,
                    )?;
                }
                if let Some(p_new) = p_tight {
                    if g.iter().chain(&p_new).any(|v| !v.is_finite()) {
                        return Err(Refusal::new(
                            RefusalCode::NonFinite,
                            "Newton gradient or direction is non-finite",
                        ));
                    }
                    let slope_new: f64 = g.iter().zip(&p_new).map(|(a, b)| a * b).sum();
                    if slope_new.is_finite() && slope_new < 0.0 {
                        p = p_new;
                        slope = slope_new;
                    } else {
                        newton_correction = false;
                        p = g.iter().map(|v| -v).collect();
                        slope = -g.iter().map(|v| v * v).sum::<f64>();
                    }
                } else {
                    newton_correction = false;
                    p = g.iter().map(|v| -v).collect();
                    slope = -g.iter().map(|v| v * v).sum::<f64>();
                }
                step = max_abs_or_nan(&p);
                rep.last_step = step;
            }
            drop(a_mat);
            // A gradient fallback can descend, but its norm is not a displacement error bound.
            // Exact zero gradient still certifies stationarity when the Hessian is singular.
            if (newton_correction || rep.residual == 0.0) && step / scale < self.tol {
                rep.converged = true;
                break;
            }
            // Never step through the body or another cloth layer: cap α by additive CCD.
            let mut alpha = self.ccd_step_bound(&p);
            rep.ccd_limited += usize::from(alpha < 1.0);
            let mut accepted = false;
            for _ in 0..50 {
                let trial = self.apply(&self.x, &p, alpha);
                let e = match self.potential_ctx(&trial, ctx) {
                    Ok(e) => e,
                    // A Newton trial may leave the characterized domain; treat it like an invalid
                    // line-search trial and shorten the step. The accepted state never extrapolates.
                    Err(e) if e.code == RefusalCode::OutOfRange => {
                        alpha *= 0.5;
                        rep.halvings += 1;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let armijo = e <= energy + 1e-4 * alpha * slope;
                let precision_step = !armijo
                    && alpha == 1.0
                    && -slope <= 16.0 * f64::EPSILON * energy.abs().max(e.abs())
                    && (e - energy).abs() <= 16.0 * f64::EPSILON * energy.abs().max(e.abs())
                    && {
                        let (trial_gradient, _) = self.assemble_ctx(&trial, ctx, false, false);
                        let trial_slope = trial_gradient.iter().zip(&p).map(|(a, b)| a * b).sum();
                        let trial_residual = max_abs_or_nan(&trial_gradient);
                        trial_gradient.iter().all(|v| v.is_finite())
                            && roundoff_step(
                                energy,
                                e,
                                alpha,
                                slope,
                                trial_slope,
                                rep.residual,
                                trial_residual,
                            )
                    };
                if armijo || precision_step {
                    self.x = trial;
                    energy = e;
                    rep.roundoff_steps += usize::from(precision_step);
                    if alpha < 0.25 {
                        ew_eta = (ew_eta * 0.1).max(1e-8);
                    }
                    accepted = true;
                    break;
                }
                alpha *= 0.5;
                rep.halvings += 1;
            }
            if !accepted {
                rep.line_search_failures += 1;
                break;
            }
        }
        rep.energy_end = energy;
        if !rep.converged {
            let (g, _) = self.assemble_ctx(&self.x, ctx, false, false);
            rep.residual = max_abs_or_nan(&g);
            let mut refusal = Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "Newton did not converge after {} iterations (residual {} N, last correction {} m, {} failed line searches); diagnostics {rep:?}",
                    rep.iterations, rep.residual, rep.last_step, rep.line_search_failures
                ),
            )
            .repair(if ctx.is_dynamic() {
                "increase the Newton iteration budget or reduce the time step"
            } else {
                "check that the external loads have static support or increase the Newton iteration budget"
            })
            .repair("check the material domain and the garment start state");
            if let StepContext::GeneralizedAlpha { params, .. } = ctx
                && params.rho_inf > 0.0
                && self.contact.is_some()
            {
                refusal = refusal.repair(
                    "for Generalized-alpha with rho_inf > 0 on stiff contact transitions, reduce the time step or lower --rho-inf toward 0.0",
                );
            }
            return Err(refusal);
        }
        Ok(rep)
    }

    #[allow(clippy::too_many_arguments)]
    fn solve_newton_linear(
        &mut self,
        a: &Csr,
        g: &[f64],
        projected: bool,
        linear_tolerance: f64,
        fallback_tolerance: f64,
        adaptive_ilu: Option<(usize, usize)>,
        rep: &mut SolveReport,
    ) -> Result<Option<Vec<f64>>, Refusal> {
        if let Some((ilu_pcg_iters, max_shifts)) = adaptive_ilu
            && ilu_pcg_iters > 0
            && let Ok((ilu, _shift)) = ilu0_spd_shifted(a, max_shifts)
        {
            let b: Vec<f64> = g.iter().map(|v| -v).collect();
            let n = b.len();
            match pcg(
                a,
                &b,
                |r: &[f64]| {
                    let mut z = vec![0.0; n];
                    ilu.apply(r, &mut z);
                    z
                },
                linear_tolerance,
                ilu_pcg_iters,
            ) {
                Ok((p, r)) => {
                    rep.pcg_iterations += r.iters;
                    return Ok(Some(p));
                }
                Err(PcgFailure::NegativeCurvature(k)) if !projected => {
                    rep.pcg_iterations += k;
                    return Ok(None);
                }
                Err(PcgFailure::NegativeCurvature(k) | PcgFailure::NotConverged(k)) => {
                    rep.pcg_iterations += k;
                }
            }
        }
        self.newton_direction(a, g, projected, fallback_tolerance, rep)
    }

    /// Trust-region Newton with Steihaug–Toint truncated CG on the exact unprojected Hessian.
    fn minimize_trust_region(
        &mut self,
        ctx: StepContext<'_>,
        initial_radius: f64,
        max_radius: f64,
        min_radius: f64,
        cg_iters: usize,
    ) -> Result<SolveReport, Refusal> {
        if !(initial_radius.is_finite()
            && max_radius.is_finite()
            && min_radius.is_finite()
            && min_radius > 0.0
            && initial_radius >= min_radius
            && max_radius >= initial_radius)
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "trust-region radii must be finite with 0 < min_radius <= initial_radius <= max_radius",
            ));
        }
        let mut rep = SolveReport {
            energy_start: self.potential_ctx(&self.x, ctx)?,
            ..SolveReport::default()
        };
        let mut energy = rep.energy_start;
        if self.nfree == 0 {
            rep.converged = true;
            rep.energy_end = energy;
            return Ok(rep);
        }
        let scale = ctx.scale();
        let lin_tol = if ctx.is_dynamic() {
            PCG_TOL
        } else {
            LdltOptions::default().zero_pivot_rel
        };
        let mut delta = initial_radius;
        let max_cg = if cg_iters == 0 {
            2 * self.nfree.max(32)
        } else {
            cg_iters
        };
        for it in 0..self.max_newton {
            let (g, h_mat) = self.assemble_ctx(&self.x, ctx, false, false);
            if g.iter().any(|v| !v.is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "trust-region gradient is non-finite",
                ));
            }
            let res_inf = max_abs_or_nan(&g);
            rep.iterations = it + 1;
            rep.residual = res_inf;
            if res_inf == 0.0 {
                rep.last_step = 0.0;
                rep.converged = true;
                break;
            }
            let out = steihaug_cg(
                &h_mat,
                &g,
                SteihaugConfig {
                    radius: delta,
                    rel_tol: lin_tol,
                    max_iters: max_cg,
                },
            );
            rep.pcg_iterations += out.matvecs;
            if out.step.iter().any(|v| !v.is_finite()) || !out.model_decrease.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "trust-region step or model decrease is non-finite",
                ));
            }
            let mut p = out.step;
            let mut model_decrease = out.model_decrease;
            let step_inf = max_abs_or_nan(&p);
            rep.last_step = step_inf;
            let newton_accurate = out.interior_converged || {
                let mut hp = vec![0.0; self.nfree];
                h_mat.spmv(&p, &mut hp);
                let r_norm = hp
                    .iter()
                    .zip(&g)
                    .map(|(a, b)| (a + b) * (a + b))
                    .sum::<f64>()
                    .sqrt();
                let g_norm = g.iter().map(|v| v * v).sum::<f64>().sqrt();
                g_norm == 0.0 || (r_norm.is_finite() && r_norm <= lin_tol * g_norm)
            };
            if newton_accurate
                && !out.hit_boundary
                && !out.negative_curvature
                && step_inf / scale < self.tol
            {
                rep.converged = true;
                break;
            }
            let ccd_alpha = self.ccd_step_bound(&p);
            if ccd_alpha < 1.0 {
                rep.ccd_limited += 1;
                for v in &mut p {
                    *v *= ccd_alpha;
                }
                let mut hp = vec![0.0; self.nfree];
                h_mat.spmv(&p, &mut hp);
                let gp: f64 = g.iter().zip(&p).map(|(a, b)| a * b).sum();
                let php: f64 = p.iter().zip(&hp).map(|(a, b)| a * b).sum();
                model_decrease = -gp - 0.5 * php;
            }
            let p_norm = p.iter().map(|v| v * v).sum::<f64>().sqrt();
            let trial = self.apply(&self.x, &p, 1.0);
            let trial_energy = match self.potential_ctx(&trial, ctx) {
                Ok(e) => e,
                Err(e) if e.code == RefusalCode::OutOfRange => {
                    delta = if p_norm > 0.0 {
                        (0.25 * delta).min(0.25 * p_norm)
                    } else {
                        0.25 * delta
                    };
                    rep.halvings += 1;
                    if delta < min_radius {
                        rep.line_search_failures += 1;
                        break;
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            let actual = energy - trial_energy;
            let rho = if model_decrease > 0.0 {
                actual / model_decrease
            } else {
                -1.0
            };
            let slope: f64 = g.iter().zip(&p).map(|(a, b)| a * b).sum();
            let precision_step = !(rho > 1e-4 && model_decrease > 0.0)
                && ccd_alpha == 1.0
                && !out.hit_boundary
                && -slope <= 16.0 * f64::EPSILON * energy.abs().max(trial_energy.abs())
                && (trial_energy - energy).abs()
                    <= 16.0 * f64::EPSILON * energy.abs().max(trial_energy.abs())
                && {
                    let (tg, _) = self.assemble_ctx(&trial, ctx, false, false);
                    let tslope = tg.iter().zip(&p).map(|(a, b)| a * b).sum();
                    let tres = max_abs_or_nan(&tg);
                    tg.iter().all(|v| v.is_finite())
                        && roundoff_step(energy, trial_energy, 1.0, slope, tslope, res_inf, tres)
                };
            let accepted = (rho > 1e-4 && model_decrease > 0.0) || precision_step;
            if !accepted {
                // Shrink relative to the actual rejected step norm so an interior or CCD-clipped
                // rejection immediately constrains the next Steihaug solve instead of spinning.
                delta = if p_norm > 0.0 {
                    (0.25 * delta).min(0.25 * p_norm)
                } else {
                    0.25 * delta
                };
                rep.halvings += 1;
            } else if rho < 0.25 && !precision_step {
                delta = (0.25 * delta).max(min_radius);
                rep.halvings += 1;
            } else if rho > 0.75 && out.hit_boundary && ccd_alpha == 1.0 {
                delta = (2.0 * delta).min(max_radius);
            }
            if accepted {
                self.x = trial;
                energy = trial_energy;
                rep.roundoff_steps += usize::from(precision_step);
            } else if delta < min_radius {
                rep.line_search_failures += 1;
                break;
            }
        }
        rep.energy_end = energy;
        if !rep.converged {
            let (g, _) = self.assemble_ctx(&self.x, ctx, false, false);
            rep.residual = max_abs_or_nan(&g);
            let mut refusal = Refusal::new(
                RefusalCode::NotConverged,
                format!(
                    "trust-region Newton did not converge after {} iterations (residual {} N, last correction {} m, {} failed steps); diagnostics {rep:?}",
                    rep.iterations, rep.residual, rep.last_step, rep.line_search_failures
                ),
            )
            .repair(if ctx.is_dynamic() {
                "increase the Newton iteration budget or reduce the time step"
            } else {
                "check that the external loads have static support or increase the Newton iteration budget"
            })
            .repair("check the material domain and the garment start state");
            if let StepContext::GeneralizedAlpha { params, .. } = ctx
                && params.rho_inf > 0.0
                && self.contact.is_some()
            {
                refusal = refusal.repair(
                    "for Generalized-alpha with rho_inf > 0 on stiff contact transitions, reduce the time step or lower --rho-inf toward 0.0",
                );
            }
            return Err(refusal);
        }
        Ok(rep)
    }

    // A free static garment can carry a gravity load in a rigid null mode before reaching its
    // support. Raw gradient descent couples that motion to the stiffest elastic mode's line-search
    // scale. A shifted projected system provides a descent direction without changing the energy,
    // pinning a vertex or advancing time. It is never a Newton displacement convergence certificate.
    fn static_search_direction(
        &mut self,
        a: &Csr,
        g: &[f64],
        rep: &mut SolveReport,
    ) -> Result<Option<Vec<f64>>, Refusal> {
        if (0..self.nfree).any(|i| !a.get(i, i).is_finite()) {
            return Ok(None);
        }
        let scale = (0..self.nfree)
            .map(|i| a.get(i, i).abs())
            .fold(0.0_f64, f64::max);
        let shift = f64::EPSILON.sqrt() * scale;
        if !shift.is_finite() || shift <= 0.0 {
            return Ok(None);
        }
        let mut shifted = a.clone();
        let mut offset = 0;
        for i in 0..self.nfree {
            let (cols, _) = a.row(i);
            if let Ok(diagonal) = cols.binary_search(&i) {
                shifted.values_mut()[offset + diagonal] += shift;
            }
            offset += cols.len();
        }
        let sym = self.symbolic(&shifted)?;
        rep.factorizations += 1;
        Ok(match sym.factor(&shifted, &LdltOptions::default()) {
            Ok(f) if f.inertia().negative == 0 => {
                Some(f.solve(&g.iter().map(|v| -v).collect::<Vec<_>>()))
            }
            _ => None,
        })
    }

    /// A Newton direction, or `None` when the bounded linear solves cannot produce one. With
    /// [`Solver::pcg_iters`], conjugate gradients preconditioned by the last positive-definite factor go
    /// first (Newton-CG): a direction of negative curvature in an exact Hessian is `None` without a
    /// factorization, since the caller then projects; a converged one is a descent direction even where
    /// the Hessian is indefinite elsewhere; anything else falls back to factorizing `A`. A singular
    /// projected system gets at most 32 unpreconditioned CG iterations, admitted only at the direct
    /// solver's relative rank tolerance. This can resolve a compatible gradient without pinning free
    /// rigid modes or treating force magnitude as a displacement.
    fn newton_direction(
        &mut self,
        a: &Csr,
        g: &[f64],
        projected: bool,
        linear_tolerance: f64,
        rep: &mut SolveReport,
    ) -> Result<Option<Vec<f64>>, Refusal> {
        let b: Vec<f64> = g.iter().map(|v| -v).collect();
        if self.pcg_iters > 0
            && let Some(f) = &self.precond
            && f.stats().n == b.len()
        {
            match pcg(
                a,
                &b,
                |r: &[f64]| f.solve(r),
                linear_tolerance,
                self.pcg_iters,
            ) {
                Ok((p, r)) => {
                    rep.pcg_iterations += r.iters;
                    return Ok(Some(p));
                }
                Err(PcgFailure::NegativeCurvature(k)) if !projected => {
                    rep.pcg_iterations += k;
                    return Ok(None);
                }
                Err(PcgFailure::NegativeCurvature(k) | PcgFailure::NotConverged(k)) => {
                    rep.pcg_iterations += k;
                }
            }
        }
        let sym = self.symbolic(a)?;
        rep.factorizations += 1;
        Ok(match sym.factor(a, &LdltOptions::default()) {
            Ok(f) if f.inertia().negative == 0 => {
                let p = f.solve(&b);
                if self.pcg_iters > 0 {
                    self.precond = Some(f);
                }
                Some(p)
            }
            Err(_) if projected => {
                match pcg(
                    a,
                    &b,
                    |r: &[f64]| r.to_vec(),
                    LdltOptions::default().zero_pivot_rel,
                    self.nfree.min(32),
                ) {
                    Ok((p, r)) => {
                        rep.pcg_iterations += r.iters;
                        Some(p)
                    }
                    Err(PcgFailure::NegativeCurvature(k) | PcgFailure::NotConverged(k)) => {
                        rep.pcg_iterations += k;
                        None
                    }
                }
            }
            _ => None,
        })
    }

    /// One implicit-Euler step of size `h` (s).
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] for a non-positive step; [`RefusalCode::NonFinite`] when the state
    /// becomes non-finite; [`RefusalCode::NotConverged`] for a stalled solve. Refusals preserve
    /// positions, velocity, contact lag and linear-solver caches.
    pub fn step(&mut self, h: f64) -> Result<SolveReport, Refusal> {
        self.step_at_body(h, None)
    }

    // Prescribed body motion is admitted transactionally by motion::advance_body. Newton sees
    // the endpoint collider, while friction measures displacement from its previous coordinates.
    pub(crate) fn step_at_body(
        &mut self,
        h: f64,
        body_start: Option<&[Vec3]>,
    ) -> Result<SolveReport, Refusal> {
        self.check_liquid_mass()?;
        self.model.check_attached_mass()?;
        if self.relaxation.is_some()
            || self.plasticity.is_some()
            || self.membrane_plasticity.is_some()
        {
            if self.integrator != TimeIntegrator::ImplicitEuler {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "Generalized-alpha integration is not admitted for backward-Euler material-history fields; use TimeIntegrator::ImplicitEuler",
                ));
            }
            return self.relaxing_step(h, body_start);
        }
        let mut trial = self.clone();
        let report = trial.step_inner(h, body_start)?;
        if trial.v.iter().any(|v| !v.is_finite()) || trial.a.iter().any(|a| !a.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth velocity or acceleration became non-finite",
            ));
        }
        *self = trial;
        Ok(report)
    }

    /// Implicit-Euler (or configured [`TimeIntegrator`]) step from the accepted physical state
    /// `self.x` (`x_n`), optionally seeding free DOFs of the Newton minimizer at `guess` without
    /// altering `x_n` in the inertia predictor or post-step velocity. Falls back to `step` when
    /// contact is active.
    pub(crate) fn step_with_guess(
        &mut self,
        h: f64,
        guess: Option<&[Vec3]>,
    ) -> Result<SolveReport, Refusal> {
        if guess.is_none() || self.contact.is_some() {
            return self.step_at_body(h, None);
        }
        self.check_liquid_mass()?;
        self.model.check_attached_mass()?;
        if self.relaxation.is_some()
            || self.plasticity.is_some()
            || self.membrane_plasticity.is_some()
        {
            if self.integrator != TimeIntegrator::ImplicitEuler {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "Generalized-alpha integration is not admitted for backward-Euler material-history fields; use TimeIntegrator::ImplicitEuler",
                ));
            }
            return self.relaxing_step(h, None);
        }
        let mut trial = self.clone();
        let report = trial.step_inner_with_guess(h, None, guess)?;
        if trial.v.iter().any(|v| !v.is_finite()) || trial.a.iter().any(|a| !a.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth velocity or acceleration became non-finite",
            ));
        }
        *self = trial;
        Ok(report)
    }

    fn step_inner(&mut self, h: f64, body_start: Option<&[Vec3]>) -> Result<SolveReport, Refusal> {
        self.step_inner_with_guess(h, body_start, None)
    }

    fn step_inner_with_guess(
        &mut self,
        h: f64,
        body_start: Option<&[Vec3]>,
        guess: Option<&[Vec3]>,
    ) -> Result<SolveReport, Refusal> {
        self.check_integrator_history()?;
        self.model.check_panel_bending_domains(&self.x)?;
        if !h.is_finite() || h <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("time step {h} s must be finite and positive"),
            ));
        }
        self.audit_self_contact()?;
        if let TimeIntegrator::GeneralizedAlpha { rho_inf } = self.integrator {
            return self.step_inner_generalized_alpha(h, rho_inf, body_start, guess);
        }
        let x0 = self.x.clone();
        let damping_step = DampingStep::build(
            &self.model,
            self.damping,
            &x0,
            h,
            VelocityMap::ImplicitEuler {
                x0: &x0,
                inv_h: 1.0 / h,
            },
        )?;
        let xt: Vec<Vec3> = x0
            .iter()
            .zip(&self.v)
            .enumerate()
            .map(|(i, (x, v))| {
                let mut p = *x + *v * h + self.gravity * (h * h);
                for k in 0..3 {
                    if self.free[3 * i + k].is_none() {
                        match k {
                            0 => p.x = x.x,
                            1 => p.y = x.y,
                            _ => p.z = x.z,
                        }
                    }
                }
                p
            })
            .collect();
        if let Some(c) = &mut self.contact {
            if let Some(b) = body_start {
                c.update_lag_motion(&x0, &x0, h, Some(b));
            } else {
                c.update_lag(&x0, &x0, h);
            }
        } else if let Some(g) = guess {
            if g.len() != self.x.len() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "step warm-start length must match vertex count",
                ));
            }
            for (i, (pos, cand)) in self.x.iter_mut().zip(g).enumerate() {
                if self.free[3 * i].is_some() {
                    pos.x = cand.x;
                }
                if self.free[3 * i + 1].is_some() {
                    pos.y = cand.y;
                }
                if self.free[3 * i + 2].is_some() {
                    pos.z = cand.z;
                }
            }
        }
        let ctx = StepContext::ImplicitEuler {
            xt: &xt,
            h,
            damping: damping_step.as_ref(),
        };
        let mut rep = match self.minimize_ctx(ctx) {
            Ok(rep) => rep,
            Err(e) => {
                self.x = x0;
                return Err(e);
            }
        };
        // Lag rounds (IPC): refresh normal forces and contact sets at the new iterate and re-solve,
        // so a contact made during the step has friction within the same step.
        for _ in 0..self.friction_rounds {
            let Some(c) = &mut self.contact else { break };
            if !c.has_friction() {
                break;
            }
            if let Some(b) = body_start {
                c.update_lag_motion(&x0, &self.x, h, Some(b));
            } else {
                c.update_lag(&x0, &self.x, h);
            }
            let again = match self.minimize_ctx(ctx) {
                Ok(rep) => rep,
                Err(e) => {
                    self.x = x0;
                    return Err(e);
                }
            };
            rep.iterations += again.iterations;
            rep.projected_iterations += again.projected_iterations;
            rep.line_search_failures += again.line_search_failures;
            rep.factorizations += again.factorizations;
            rep.ccd_limited += again.ccd_limited;
            rep.halvings += again.halvings;
            rep.roundoff_steps += again.roundoff_steps;
            rep.pcg_iterations += again.pcg_iterations;
            rep.converged = again.converged;
            rep.energy_end = again.energy_end;
            rep.residual = again.residual;
            rep.last_step = again.last_step;
        }
        if self.x.iter().any(|p| !p.is_finite()) {
            self.x = x0;
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth state became non-finite",
            ));
        }
        if let Err(e) = self.audit_self_contact() {
            self.x = x0;
            return Err(e);
        }
        self.v = self.x.iter().zip(&x0).map(|(a, b)| (*a - *b) / h).collect();
        Ok(rep)
    }

    fn step_inner_generalized_alpha(
        &mut self,
        h: f64,
        rho_inf: f64,
        body_start: Option<&[Vec3]>,
        guess: Option<&[Vec3]>,
    ) -> Result<SolveReport, Refusal> {
        let params = GeneralizedAlphaParams::new(rho_inf)?;
        if !params.mass_coeff(h).is_finite() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("time step {h} s underflows Generalized-alpha inertia scaling"),
            ));
        }
        let x0 = self.x.clone();
        let v0 = self.v.clone();
        if let Some(c) = &mut self.contact {
            if let Some(b) = body_start {
                c.update_lag_motion(&x0, &x0, h, Some(b));
            } else {
                c.update_lag(&x0, &x0, h);
            }
        }
        let a0 = if self.accel_initialized {
            self.a.clone()
        } else {
            // Consistent initial acceleration M a₀ = M g − ∇Ψ(x₀) + F_drag(x₀, v₀) on free DOFs so
            // Generalized-α preserves full 2nd-order accuracy from step 1 without mutating state before accept.
            let (mut g0, _) = self.assemble_ctx(&x0, StepContext::Static, false, false);
            if let Some(drag) = self.damping.air_drag {
                for e in &self.model.membranes {
                    let cross = (x0[e.v[1]] - x0[e.v[0]]).cross(x0[e.v[2]] - x0[e.v[0]]);
                    let norm = cross.norm();
                    if !(norm.is_finite() && norm > 0.0) {
                        continue;
                    }
                    let vc = (v0[e.v[0]] + v0[e.v[1]] + v0[e.v[2]]) * (1.0 / 3.0);
                    let f_res =
                        drag.triangle_resistance_force(0.5 * norm, cross * (1.0 / norm), vc)
                            * (1.0 / 3.0);
                    for &vi in &e.v {
                        for k in 0..3 {
                            if let Some(f) = self.free[3 * vi + k] {
                                g0[f] += f_res[k];
                            }
                        }
                    }
                }
            }
            let mut init = vec![Vec3::ZERO; x0.len()];
            for (i, &m) in self.model.mass.iter().enumerate() {
                if m > 0.0 {
                    let mut acc = Vec3::ZERO;
                    for k in 0..3 {
                        if let Some(f) = self.free[3 * i + k] {
                            let val = -g0[f] / m;
                            match k {
                                0 => acc.x = val,
                                1 => acc.y = val,
                                _ => acc.z = val,
                            }
                        }
                    }
                    init[i] = acc;
                }
            }
            if init.iter().any(|a| !a.is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "consistent initial acceleration is non-finite",
                ));
            }
            init
        };
        let xt: Vec<Vec3> = (0..x0.len())
            .map(|i| {
                let mut p = params.predictor(x0[i], v0[i], a0[i], self.gravity, h);
                for k in 0..3 {
                    if self.free[3 * i + k].is_none() {
                        match k {
                            0 => p.x = x0[i].x,
                            1 => p.y = x0[i].y,
                            _ => p.z = x0[i].z,
                        }
                    }
                }
                p
            })
            .collect();
        if self.contact.is_none()
            && let Some(g) = guess
        {
            if g.len() != self.x.len() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "step warm-start length must match vertex count",
                ));
            }
            for (i, (pos, cand)) in self.x.iter_mut().zip(g).enumerate() {
                if self.free[3 * i].is_some() {
                    pos.x = cand.x;
                }
                if self.free[3 * i + 1].is_some() {
                    pos.y = cand.y;
                }
                if self.free[3 * i + 2].is_some() {
                    pos.z = cand.z;
                }
            }
        }
        let damping_step = if self.damping.is_default() {
            None
        } else {
            let c_v = params.elastic_weight() * params.gamma / (params.beta * h);
            let v_coeff =
                params.elastic_weight() * (1.0 - params.gamma / params.beta) + params.alpha_f;
            let a_coeff = params.elastic_weight() * h * (1.0 - params.gamma / (2.0 * params.beta));
            let v_base: Vec<Vec3> = v0
                .iter()
                .zip(&a0)
                .map(|(&vi, &ai)| vi * v_coeff + ai * a_coeff)
                .collect();
            DampingStep::build(
                &self.model,
                self.damping,
                &x0,
                h,
                VelocityMap::GeneralizedAlpha {
                    x0: &x0,
                    v_base,
                    c_v,
                },
            )?
        };
        let ctx = StepContext::GeneralizedAlpha {
            x0: &x0,
            xt: &xt,
            h,
            params,
            damping: damping_step.as_ref(),
        };
        let mut rep = match self.minimize_ctx(ctx) {
            Ok(rep) => rep,
            Err(e) => {
                self.x = x0;
                return Err(e);
            }
        };
        for _ in 0..self.friction_rounds {
            let Some(c) = &mut self.contact else { break };
            if !c.has_friction() {
                break;
            }
            if let Some(b) = body_start {
                c.update_lag_motion(&x0, &self.x, h, Some(b));
            } else {
                c.update_lag(&x0, &self.x, h);
            }
            let again = match self.minimize_ctx(ctx) {
                Ok(rep) => rep,
                Err(e) => {
                    self.x = x0;
                    return Err(e);
                }
            };
            rep.iterations += again.iterations;
            rep.projected_iterations += again.projected_iterations;
            rep.line_search_failures += again.line_search_failures;
            rep.factorizations += again.factorizations;
            rep.ccd_limited += again.ccd_limited;
            rep.halvings += again.halvings;
            rep.roundoff_steps += again.roundoff_steps;
            rep.pcg_iterations += again.pcg_iterations;
            rep.converged = again.converged;
            rep.energy_end = again.energy_end;
            rep.residual = again.residual;
            rep.last_step = again.last_step;
        }
        if self.x.iter().any(|p| !p.is_finite()) {
            self.x = x0;
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth state became non-finite",
            ));
        }
        if let Err(e) = self.audit_self_contact() {
            self.x = x0;
            return Err(e);
        }
        let mut new_v = Vec::with_capacity(x0.len());
        let mut new_a = Vec::with_capacity(x0.len());
        for i in 0..x0.len() {
            let (mut vn, mut an) = params.corrector(x0[i], v0[i], a0[i], self.x[i], h);
            for k in 0..3 {
                if self.free[3 * i + k].is_none() {
                    match k {
                        0 => {
                            vn.x = 0.0;
                            an.x = 0.0;
                        }
                        1 => {
                            vn.y = 0.0;
                            an.y = 0.0;
                        }
                        _ => {
                            vn.z = 0.0;
                            an.z = 0.0;
                        }
                    }
                }
            }
            if !vn.is_finite() || !an.is_finite() {
                self.x = x0;
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "generalized-alpha velocity or acceleration became non-finite",
                ));
            }
            new_v.push(vn);
            new_a.push(an);
        }
        self.v = new_v;
        self.a = new_a;
        self.accel_initialized = true;
        Ok(rep)
    }

    /// Steps until the largest speed stays below `v_tol` (m/s) for `quiet` consecutive steps, growing
    /// the step from `h0` to `h_max` while Newton converges.
    ///
    /// # Errors
    /// As [`Solver::step`].
    pub fn settle(
        &mut self,
        h0: f64,
        h_max: f64,
        v_tol: f64,
        quiet: usize,
        max_steps: usize,
    ) -> Result<SettleReport, Refusal> {
        if !h0.is_finite()
            || h0 <= 0.0
            || !h_max.is_finite()
            || h_max < h0
            || !v_tol.is_finite()
            || v_tol <= 0.0
            || quiet == 0
            || max_steps < quiet
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "settling needs finite positive time/speed limits, h_max >= h0, and a nonzero quiet window within the step budget",
            ));
        }
        self.check_settling_history()?;
        let mut rep = SettleReport {
            h: h0,
            ..SettleReport::default()
        };
        let mut calm = 0;
        for _ in 0..max_steps {
            let r = self.step(rep.h)?;
            rep.steps += 1;
            rep.time += rep.h;
            rep.newton_iterations += r.iterations;
            if self.v.iter().any(|v| !v.norm().is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "cloth velocity became non-finite while settling",
                ));
            }
            rep.max_speed = self.v.iter().fold(0.0f64, |m, v| m.max(v.norm()));
            calm = if rep.max_speed < v_tol { calm + 1 } else { 0 };
            if calm >= quiet {
                rep.settled = true;
                break;
            }
            if r.converged && r.iterations <= 8 {
                rep.h = (rep.h * 2.0).min(h_max);
            }
        }
        Ok(rep)
    }

    /// An automatic stopping time cannot select a constitutive history duration or loading path.
    pub(crate) fn check_settling_history(&self) -> Result<(), Refusal> {
        if self.relaxation.is_some()
            || self.plasticity.is_some()
            || self.membrane_plasticity.is_some()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "speed-only settling cannot certify material history; use explicit time steps and inspect stress and irreversible strain/curvature",
            ));
        }
        Ok(())
    }

    /// Minimises `Ψ − Σ mᵢ g·xᵢ` from the current positions (equilibrium under gravity) and zeroes the
    /// velocities.
    ///
    /// # Errors
    /// As [`Solver::step`].
    pub fn static_solve(&mut self) -> Result<SolveReport, Refusal> {
        self.check_liquid_mass()?;
        self.model.check_attached_mass()?;
        self.check_integrator_history()?;
        if self.relaxation.is_some()
            || self.plasticity.is_some()
            || self.membrane_plasticity.is_some()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "a static solve cannot choose a material-history duration or irreversible loading path; use explicit time steps",
            ));
        }
        let mut trial = self.clone();
        trial.audit_self_contact()?;
        let rep = trial.minimize(None)?;
        if trial.x.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth state became non-finite",
            ));
        }
        trial.audit_self_contact()?;
        trial.v.fill(Vec3::ZERO);
        trial.a.fill(Vec3::ZERO);
        trial.accel_initialized = false;
        *self = trial;
        Ok(rep)
    }

    /// Internal force from accepted material storage (−∂Ψ/∂x), including history-dependent
    /// reactions. Invalid or stale history returns non-finite forces; use the checked accessor
    /// when the refusal reason is needed.
    #[must_use]
    pub fn elastic_forces(&self) -> Vec<Vec3> {
        self.checked_elastic_forces()
            .unwrap_or_else(|_| vec![Vec3::new(f64::NAN, f64::NAN, f64::NAN); self.x.len()])
    }

    /// Internal reactions from the frozen accepted material state, without advancing history.
    ///
    /// # Errors
    /// Stale material/reference ownership, invalid geometry or a non-finite stored response.
    pub fn checked_elastic_forces(&self) -> Result<Vec<Vec3>, Refusal> {
        self.checked_forces_with(&self.x, None)
    }

    /// Internal elastic forces at `self.x` evaluated with the incremental constitutive state
    /// active during the step from `before_step` over duration `h` (s).
    ///
    /// For undamaged laws, this agrees with [`Self::checked_elastic_forces`] after step
    /// acceptance. For explicit accepted-step stiffness degradation
    /// (`isotropic_accumulated_damage/1`), Newton balances `m * a` against the step-active
    /// stiffness factor `(1 - d_n)` before `d_n -> d_{n+1}` updates on acceptance; this method
    /// reconstructs that exact step-time internal force vector without mutating either solver.
    ///
    /// # Errors
    /// Non-finite or non-positive step `h`, mismatched topology/material/domain, or non-finite forces.
    pub fn checked_step_forces_from(
        &self,
        before_step: &Self,
        h: f64,
    ) -> Result<Vec<Vec3>, Refusal> {
        if !h.is_finite() || h <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "step-time material reactions require a finite positive time step",
            ));
        }
        if self.x.len() != self.model.rest.len()
            || self.model.rest.len() != before_step.model.rest.len()
            || self.model.membranes != before_step.model.membranes
            || self.model.hinges != before_step.model.hinges
            || self.model.elastics.len() != before_step.model.elastics.len()
            || self.model.panel_bending_patches().len()
                != before_step.model.panel_bending_patches().len()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "step-time material reactions require matching cloth topology",
            ));
        }
        before_step.checked_forces_with(&self.x, Some(h))
    }

    fn checked_forces_with(&self, x: &[Vec3], step_dt: Option<f64>) -> Result<Vec<Vec3>, Refusal> {
        let incremental = step_dt.is_some();
        if x.len() != self.model.rest.len()
            || self.model.compression.len() != self.model.membranes.len()
            || x.iter().any(|p| !p.is_finite())
            || self
                .model
                .membranes
                .iter()
                .any(|e| e.v.iter().any(|i| *i >= x.len()))
            || self
                .model
                .hinges
                .iter()
                .any(|e| e.v.iter().any(|i| *i >= x.len()))
            || self
                .model
                .panel_bending_patches()
                .iter()
                .any(|e| e.v.iter().any(|i| *i >= x.len()))
            || self
                .model
                .elastics
                .iter()
                .any(|e| e.v.iter().any(|i| *i >= x.len()))
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "material reactions require matching finite cloth geometry",
            ));
        }
        self.model.check_membrane_domains(x)?;
        self.model.check_panel_bending_domains(x)?;
        if let Some(field) = &self.membrane_plasticity {
            field.check(&self.model, x)?;
        }
        if let Some(field) = &self.relaxation {
            field.check(&self.model, x)?;
        }
        if let Some(field) = &self.plasticity {
            field.check(&self.model, x)?;
        }
        let mut f = vec![Vec3::ZERO; x.len()];
        for (i, e) in self.model.membranes.iter().enumerate() {
            let mut l = self
                .membrane_plasticity
                .as_ref()
                .and_then(|field| field.local(i, x, incremental))
                .unwrap_or_else(|| {
                    self.model.membrane_response(i).map_or_else(
                        || self.model.membrane_local(i, x),
                        |law| e.response_local(x, law, &self.model.material(i)),
                    )
                });
            if let Some(extra) = self
                .relaxation
                .as_ref()
                .and_then(|field| field.local(i, x, step_dt))
            {
                for (a, b) in l.grad.iter_mut().zip(extra.grad) {
                    *a += b;
                }
            }
            for (a, &vi) in e.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        for (i, h) in self.model.hinges.iter().enumerate() {
            let l = self.model.hinge_local(i, x);
            for (a, &vi) in h.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        for (i, p) in self.model.panel_bending_patches().iter().enumerate() {
            let l = self
                .plasticity
                .as_ref()
                .and_then(|field| field.local(i, x, incremental))
                .unwrap_or_else(|| p.local(x));
            for (a, &vi) in p.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        for e in &self.model.elastics {
            let l = e.local(x);
            for (a, &vi) in e.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        if f.iter().any(|force| !force.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "stored material reactions are non-finite",
            ));
        }
        Ok(f)
    }
}

/// A rectangle `w × h` (m) meshed with near-equilateral triangles: rows along x at height ≈ `spacing·√3/2`,
/// alternate rows offset by half a spacing, with half triangles closing the left and right ends so every
/// side is straight. This is the interior the piece mesher uses (plan §11.4): hinge bending on equilateral
/// triangles is isotropic, while right-triangle grids have soft zig-zag modes along clamped edges.
#[must_use]
pub fn hex_grid(w: f64, h: f64, spacing: f64, origin: Vec2) -> (Vec<Vec2>, Vec<[usize; 3]>) {
    let n = ((w / spacing).round() as usize).max(1);
    let s = w / n as f64;
    let rows = ((h / (spacing * 3f64.sqrt() / 2.0)).round() as usize).max(1);
    let dy = h / rows as f64;
    let mut rest = Vec::new();
    let mut starts = Vec::with_capacity(rows + 1);
    for r in 0..=rows {
        starts.push(rest.len());
        let y = origin.y + r as f64 * dy;
        if r % 2 == 0 {
            rest.extend((0..=n).map(|i| Vec2::new(origin.x + i as f64 * s, y)));
        } else {
            rest.push(Vec2::new(origin.x, y));
            rest.extend((0..n).map(|i| Vec2::new(origin.x + (i as f64 + 0.5) * s, y)));
            rest.push(Vec2::new(origin.x + w, y));
        }
    }
    let mut tris = Vec::new();
    for r in 0..rows {
        let (lo, hi) = (starts[r], starts[r + 1]);
        if r % 2 == 0 {
            // Lower row even (n + 1 points e), upper row odd (n + 2 points o).
            let (e, o) = (|i: usize| lo + i, |i: usize| hi + i);
            tris.push([e(0), o(1), o(0)]);
            for i in 0..n {
                tris.push([e(i), e(i + 1), o(i + 1)]);
                if i + 1 < n {
                    tris.push([e(i + 1), o(i + 2), o(i + 1)]);
                }
            }
            tris.push([e(n), o(n + 1), o(n)]);
        } else {
            // Lower row odd, upper row even.
            let (o, e) = (|i: usize| lo + i, |i: usize| hi + i);
            tris.push([o(0), o(1), e(0)]);
            for i in 0..n {
                tris.push([o(i + 1), e(i + 1), e(i)]);
                tris.push([o(i + 1), o(i + 2), e(i + 1)]);
            }
        }
    }
    (rest, tris)
}

/// A welded tube: vertex count, triangles, per-triangle rest coordinates and per-vertex rest coordinates.
pub type Tube = (usize, Vec<[usize; 3]>, Vec<[Vec2; 3]>, Vec<Vec2>);

/// A tube of rest circumference `c` and height `h` (m): a [`hex_grid`] whose right edge is welded onto
/// its left, so each triangle keeps its flat rest coordinates (u ∈ [0, c]) while the seam vertices are
/// shared. Returns the vertex count, triangles, per-triangle rest coordinates and each vertex's rest
/// coordinate (u ∈ [0, c)). Build the model with [`ClothModel::from_triangles`]. `c` should be at least
/// three spacings, or welding collapses triangles.
#[must_use]
pub fn hex_tube(c: f64, h: f64, spacing: f64) -> Tube {
    let (rest, tris) = hex_grid(c, h, spacing, Vec2::ZERO);
    let tol = 1e-9 * c;
    // Each vertex on the right edge maps to the left-edge vertex of its row.
    let left: Vec<usize> = (0..rest.len())
        .filter(|&j| rest[j].x.abs() <= tol)
        .collect();
    let mut target: Vec<usize> = (0..rest.len()).collect();
    for (i, p) in rest.iter().enumerate() {
        if (p.x - c).abs() <= tol
            && let Some(&j) = left.iter().find(|&&j| (rest[j].y - p.y).abs() <= tol)
        {
            target[i] = j;
        }
    }
    let mut index = vec![usize::MAX; rest.len()];
    let mut kept = Vec::new();
    for i in 0..rest.len() {
        if target[i] == i {
            index[i] = kept.len();
            kept.push(rest[i]);
        }
    }
    let tri_rest: Vec<[Vec2; 3]> = tris.iter().map(|t| t.map(|i| rest[i])).collect();
    let welded: Vec<[usize; 3]> = tris.iter().map(|t| t.map(|i| index[target[i]])).collect();
    (kept.len(), welded, tri_rest, kept)
}

/// A rectangular grid `w × h` (m) with `nx × ny` cells in the material frame, each cell split along
/// alternating diagonals. Returns rest coordinates and triangles (counter-clockwise). For tests and
/// simple shapes; prefer [`hex_grid`] for bending (right-triangle grids have soft modes at clamps).
#[must_use]
pub fn grid(w: f64, h: f64, nx: usize, ny: usize, origin: Vec2) -> (Vec<Vec2>, Vec<[usize; 3]>) {
    let mut rest = Vec::with_capacity((nx + 1) * (ny + 1));
    for j in 0..=ny {
        for i in 0..=nx {
            rest.push(Vec2::new(
                origin.x + w * i as f64 / nx as f64,
                origin.y + h * j as f64 / ny as f64,
            ));
        }
    }
    let id = |i: usize, j: usize| j * (nx + 1) + i;
    let mut tris = Vec::with_capacity(2 * nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let (a, b, c, d) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
            if (i + j) % 2 == 0 {
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            } else {
                tris.push([a, b, d]);
                tris.push([b, c, d]);
            }
        }
    }
    (rest, tris)
}

#[cfg(test)]
mod line_search_tests {
    use super::*;

    fn light_sheet(fixed: &[[bool; 3]], gravity: Vec3) -> Solver {
        let rest = vec![Vec2::ZERO, Vec2::new(1e-3, 0.0), Vec2::new(0.0, 1e-3)];
        let x = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(100.0, 100.0, 0.0, 10.0, 1.0),
            0.0,
            0.2,
        )
        .unwrap();
        Solver::new(model, x, fixed, gravity)
    }

    #[test]
    fn light_free_sheets_have_no_static_gravity_equilibrium() {
        let mut unloaded = light_sheet(&[[false; 3]; 3], Vec3::ZERO);
        let initial = unloaded.x.clone();
        let report = unloaded.static_solve().unwrap();
        assert!(report.converged);
        assert!(report.last_step < unloaded.tol);
        assert_eq!(unloaded.x, initial);

        // A rigid translation leaves elastic storage unchanged. Under nonzero gravity the
        // potential is unbounded below, even when the nodal force is numerically below tol.
        for pcg_iters in [0, 20] {
            for gravity in [9.81, 9.81e-6] {
                for strain in [0.0, 0.1] {
                    let mut falling = light_sheet(&[[false; 3]; 3], Vec3::new(0.0, 0.0, -gravity));
                    falling.pcg_iters = pcg_iters;
                    if pcg_iters > 0 {
                        falling.step(1e-3).unwrap();
                        assert!(falling.precond.is_some());
                    }
                    assert!(falling.model.mass[0] * falling.gravity.norm() < falling.tol);
                    falling.x[1].x *= 1.0 + strain;
                    falling.v[0].z = 0.1;
                    let before = format!("{falling:?}");
                    let refusal = falling
                        .static_solve()
                        .expect_err("an unsupported sheet cannot equilibrate");
                    assert_eq!(refusal.code, RefusalCode::NotConverged);
                    assert_eq!(format!("{falling:?}"), before);
                }
            }
        }
    }

    #[test]
    fn light_supported_sheet_reaches_static_force_balance() {
        for pcg_iters in [0, 20] {
            let mut sheet = light_sheet(
                &[[true; 3], [false, true, true], [true; 3]],
                Vec3::new(9.81, 0.0, 0.0),
            );
            sheet.pcg_iters = pcg_iters;
            sheet.tol = 1e-12;
            if pcg_iters > 0 {
                sheet.step(1e-3).unwrap();
                assert!(sheet.precond.is_some());
            }
            let initial = sheet.x.clone();
            let report = sheet.static_solve().unwrap();
            assert!(report.converged);
            assert!(report.last_step < sheet.tol);
            assert!(sheet.x[1].x > initial[1].x);
            assert_eq!(sheet.x[0], initial[0]);
            assert_eq!(sheet.x[2], initial[2]);
            let forces = sheet.checked_elastic_forces().unwrap();
            assert!((forces[1].x + sheet.model.mass[1] * sheet.gravity.x).abs() < 1e-11);
            assert!(sheet.v.iter().all(|v| *v == Vec3::ZERO));
        }
    }

    #[test]
    fn light_free_sheet_reaches_contact_support_without_pinning_rigid_modes() {
        for pcg_iters in [0, 20] {
            let body = Body::new(
                vec![
                    Vec3::new(-0.01, -0.01, -0.01),
                    Vec3::new(0.02, -0.01, -0.01),
                    Vec3::new(-0.01, 0.02, -0.01),
                ],
                vec![[0, 1, 2]],
                0.01,
            )
            .unwrap();
            let params = ContactParams {
                dhat: 1e-3,
                thickness: 1e-4,
                kappa: 10.0,
                mu: 0.0,
                eps_v: 1e-3,
            };
            let mut sheet = light_sheet(&[[false; 3]; 3], Vec3::new(0.0, 0.0, -9.81))
                .with_contact(body, params)
                .unwrap();
            sheet.pcg_iters = pcg_iters;
            sheet.tol = 1e-10;
            if pcg_iters > 0 {
                sheet.step(1e-3).unwrap();
                assert!(sheet.precond.is_some());
            }
            let initial = sheet.x.clone();
            let report = sheet.static_solve().unwrap();
            assert!(report.converged && report.last_step < sheet.tol);
            assert!(report.energy_end < report.energy_start);
            let (gradient, _) = sheet.assemble(&sheet.x, None, false);
            assert!(gradient.iter().all(|v| v.abs() < 1e-10), "{gradient:?}");
            for (before, after) in initial.iter().zip(&sheet.x) {
                assert!(after.z < before.z - 0.005);
                let gap = after.z + 0.01 - params.thickness;
                assert!(gap > 0.0 && gap < params.dhat);
                assert!((after.x - before.x).abs() < 1e-12);
                assert!((after.y - before.y).abs() < 1e-12);
            }
            assert!(sheet.v.iter().all(|v| *v == Vec3::ZERO));
        }
    }

    #[test]
    fn unresolved_energy_requires_full_step_curvature_and_actual_gradient_progress() {
        // A quadratic near its minimum: f=1+(x-1)^2, x=1+1e-8, p=-1e-8.
        // Its decrement is below one energy ULP while its derivative still resolves the root.
        // One ULP of upward evaluation noise must not prevent that full Newton correction.
        let energy = 1.0;
        let perturbed = energy + f64::EPSILON;
        let (slope, residual) = (-2e-16, 2e-8);
        assert!(roundoff_step(
            energy, perturbed, 1.0, slope, 0.0, residual, 0.0
        ));
        // Equal energies alone cannot authorize no progress, an overshoot, a CCD-limited step,
        // a resolvable energy rise/decrement, or non-finite derivatives.
        for (e, alpha, s0, s1, r1) in [
            (energy, 1.0, slope, slope, residual),
            (energy, 1.0, slope, -2.0 * slope, 0.0),
            (energy, 0.5, slope, 0.0, 0.0),
            (energy + 1e-10, 1.0, slope, 0.0, 0.0),
            (energy, 1.0, -1e-6, 0.0, 0.0),
            (energy, 1.0, slope, 0.0, residual),
            (energy, 1.0, slope, f64::NAN, 0.0),
        ] {
            assert!(!roundoff_step(energy, e, alpha, s0, s1, residual, r1));
        }
    }
}

#[cfg(test)]
mod self_contact_tests {
    use super::*;

    fn layers(pinned_bottom: bool) -> Solver {
        let rest: Vec<_> = (0..2)
            .flat_map(|_| {
                [
                    Vec2::new(-0.01, -0.01),
                    Vec2::new(0.01, -0.01),
                    Vec2::new(0.0, 0.01),
                ]
            })
            .collect();
        let x: Vec<_> = rest
            .iter()
            .enumerate()
            .map(|(i, p)| Vec3::new(p.x, p.y, if i < 3 { 0.0 } else { 0.006 }))
            .collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2], [3, 4, 5]],
            Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
            1e-5,
            0.2,
        )
        .unwrap();
        let fixed: Vec<_> = (0..6)
            .map(|i| [true, true, pinned_bottom && i < 3])
            .collect();
        let mut solver = Solver::new(model, x, &fixed, Vec3::ZERO)
            .with_self_contact(ContactParams {
                dhat: 0.001,
                thickness: 0.0005,
                kappa: 100.0,
                mu: 0.0,
                eps_v: 0.001,
            })
            .unwrap();
        solver.tol = 1e-7;
        solver.max_newton = 200;
        solver
    }

    #[test]
    fn moving_layers_add_new_hessian_couplings_without_crossing() {
        let mut s = layers(false);
        for (i, v) in s.v.iter_mut().enumerate() {
            v.z = if i < 3 { 0.3 } else { -0.3 };
        }
        let rep = s.step(0.002).unwrap();
        assert!(rep.converged, "{rep:?}");
        assert!(!s.pattern.as_ref().unwrap().row(0).0.iter().any(|&v| v >= 3));
        let mut coupled = false;
        for _ in 0..12 {
            let rep = s.step(0.002).unwrap();
            assert!(rep.converged, "{rep:?}");
            s.audit_self_contact().unwrap();
            assert!(s.x[..3].iter().all(|p| p.z < s.x[3].z - 0.0005));
            let centroid = s.x.iter().map(|p| p.z).sum::<f64>() / 6.0;
            assert!(
                (centroid - 0.003).abs() < 1e-9,
                "unbalanced self-contact force"
            );
            coupled |= s.pattern.as_ref().unwrap().row(0).0.iter().any(|&v| v >= 3);
        }
        assert!(
            coupled,
            "contact must add off-diagonal couplings between separate pieces"
        );
    }

    #[test]
    fn upper_layer_settles_on_cloth_without_a_body_collider() {
        let mut s = layers(true);
        s.gravity.z = -9.81;
        for _ in 0..30 {
            let rep = s.step(0.01).unwrap();
            assert!(rep.converged, "{rep:?}");
            s.audit_self_contact().unwrap();
        }
        let rep = s.static_solve().unwrap();
        assert!(rep.converged, "{rep:?}");
        for p in &s.x[3..] {
            assert!(p.z > 0.0005 && p.z < 0.0015, "layer height {}", p.z);
        }
        assert!(s.x[..3].iter().all(|p| p.z == 0.0));
        assert!(s.v.iter().all(|v| *v == Vec3::ZERO));
    }

    #[test]
    fn failed_line_search_preserves_the_characterized_state() {
        let rest = vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
        let x: Vec<_> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let mut model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
            0.0,
            0.2,
        )
        .unwrap();
        // Zero shear is the upper characterized boundary. Gravity pulls the sole free x DOF
        // into positive shear: every shortened line-search trial still leaves that domain.
        model
            .set_membrane_domains(&[Some(
                StrainDomain::new([[-0.1, 0.1], [-0.1, 0.1], [-0.1, 0.0]]).unwrap(),
            )])
            .unwrap();
        let mut s = Solver::new(
            model,
            x,
            &[[true; 3], [true; 3], [false, true, true]],
            Vec3::new(1.0, 0.0, 0.0),
        );
        let before = format!("{s:?}");
        let error = s.static_solve().unwrap_err();
        assert_eq!(error.code, RefusalCode::NotConverged);
        assert!(
            error.message.contains("1 failed line searches"),
            "{error:?}"
        );
        assert_eq!(format!("{s:?}"), before);
    }

    #[test]
    fn unconverged_step_preserves_contact_and_caches_then_replays() {
        let mut s = layers(false);
        for (i, v) in s.v.iter_mut().enumerate() {
            v.z = if i < 3 { 0.3 } else { -0.3 };
        }
        s.max_newton = 1;
        let mut control = s.clone();
        let before = format!("{s:?}");
        let refusal = s.step(0.002).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::NotConverged);
        assert!(refusal.message.contains("residual"));
        assert_eq!(format!("{s:?}"), before);

        s.max_newton = 200;
        control.max_newton = 200;
        let report = s.step(0.002).unwrap();
        assert!(report.converged);
        assert_eq!(report, control.step(0.002).unwrap());
        assert_eq!(s.x, control.x);
        assert_eq!(s.v, control.v);
    }

    #[test]
    fn unconverged_static_solve_preserves_velocity_and_caches() {
        let mut s = layers(true);
        s.gravity.z = -9.81;
        s.v[3].z = 0.1;
        s.max_newton = 1;
        let before = format!("{s:?}");
        assert_eq!(
            s.static_solve().unwrap_err().code,
            RefusalCode::NotConverged
        );
        assert_eq!(format!("{s:?}"), before);
    }

    #[test]
    fn settling_requires_a_real_finite_quiet_window() {
        let mut s = layers(false);
        let before = format!("{s:?}");
        for (h0, h_max, speed, quiet, budget) in [
            (0.001, 0.01, 1e-3, 0, 3),
            (0.001, 0.01, 1e-3, 3, 2),
            (0.001, 0.0001, 1e-3, 3, 3),
            (0.001, f64::INFINITY, 1e-3, 3, 3),
            (0.001, 0.01, f64::NAN, 3, 3),
            (0.001, 0.01, 0.0, 3, 3),
        ] {
            assert_eq!(
                s.settle(h0, h_max, speed, quiet, budget).unwrap_err().code,
                RefusalCode::OutOfRange
            );
            assert_eq!(format!("{s:?}"), before);
        }
        let report = s.settle(0.001, 0.01, 1e-3, 3, 3).unwrap();
        assert!(report.settled);
        assert_eq!(report.steps, 3);
        assert_eq!(report.max_speed, 0.0);
    }

    #[test]
    fn self_contact_rejects_invalid_mutated_state_before_advancing() {
        let mut s = layers(false);
        for p in &mut s.x[3..] {
            p.z = 0.0;
        }
        let x = s.x.clone();
        let v = s.v.clone();
        assert_eq!(
            s.step(0.01).unwrap_err().code,
            RefusalCode::IntersectingStart
        );
        assert_eq!(s.x, x);
        assert_eq!(s.v, v);
        assert_eq!(
            s.static_solve().unwrap_err().code,
            RefusalCode::IntersectingStart
        );
        assert_eq!(
            s.step(f64::INFINITY).unwrap_err().code,
            RefusalCode::OutOfRange
        );
    }
}

#[cfg(test)]
mod strategy_and_integrator_tests {
    use super::*;

    fn hanging_sheet() -> Solver {
        let (rest, tris) = grid(0.10, 0.08, 4, 3, Vec2::ZERO);
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| Vec3::new(p.x, p.y, 0.004 * (p.x / 0.10)))
            .collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                if p.x == 0.0 {
                    [true, true, true]
                } else {
                    [false, false, false]
                }
            })
            .collect();
        let model = ClothModel::new(
            rest,
            &tris,
            Membrane::orthotropic(120.0, 90.0, 0.2, 30.0, 0.15),
            2e-4,
            0.22,
        )
        .unwrap();
        let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81));
        s.tol = 1e-10;
        s
    }

    #[test]
    fn adaptive_ilu_and_trust_region_steihaug_match_projected_newton_on_cloth() {
        let mut ref_solver = hanging_sheet();
        let rep_ref = ref_solver.step(0.01).unwrap();
        assert!(rep_ref.converged);

        let mut ilu_solver = hanging_sheet();
        ilu_solver.newton_strategy = NewtonStrategy::AdaptiveIlu {
            pcg_iters: 64,
            max_shifts: 4,
        };
        let rep_ilu = ilu_solver.step(0.01).unwrap();
        assert!(rep_ilu.converged, "{rep_ilu:?}");
        assert!(
            rep_ilu.factorizations <= rep_ref.factorizations,
            "ILU-PCG should not require more LDLT factorizations than ProjectedNewton"
        );
        for (a, b) in ref_solver.x.iter().zip(&ilu_solver.x) {
            assert!(
                (*a - *b).norm() < 1e-7,
                "AdaptiveIlu mismatch: {a:?} vs {b:?}"
            );
        }

        let mut tr_solver = hanging_sheet();
        tr_solver.newton_strategy = NewtonStrategy::TrustRegionSteihaug {
            initial_radius: 0.05,
            max_radius: 0.5,
            min_radius: 1e-12,
            cg_iters: 0,
        };
        let rep_tr = tr_solver.step(0.01).unwrap();
        assert!(rep_tr.converged, "{rep_tr:?}");
        assert_eq!(
            rep_tr.factorizations, 0,
            "Steihaug TR is factorization-free"
        );
        for (a, b) in ref_solver.x.iter().zip(&tr_solver.x) {
            assert!(
                (*a - *b).norm() < 1e-7,
                "TrustRegionSteihaug mismatch: {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn trust_region_steihaug_escapes_buckling_compression_and_honors_contact_ccd() {
        // Compress a 2-triangle strip along x with small out-of-plane perturbation so the exact
        // Hessian has negative curvature; Steihaug TR must descend cleanly to equilibrium without
        // a single factorization.
        let (rest, tris) = grid(0.06, 0.03, 2, 1, Vec2::ZERO);
        let x: Vec<Vec3> = rest
            .iter()
            .map(|p| {
                let s = if p.x > 0.0 && p.x < 0.06 { 0.002 } else { 0.0 };
                Vec3::new(p.x * 0.96, p.y, s)
            })
            .collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                if p.x == 0.0 || (p.x - 0.06).abs() < 1e-12 {
                    [true, true, true]
                } else {
                    [false, false, false]
                }
            })
            .collect();
        let model = ClothModel::new(
            rest,
            &tris,
            Membrane::orthotropic(80.0, 80.0, 0.0, 20.0, 0.1),
            1e-4,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(model, x, &fixed, Vec3::new(0.0, 0.0, -9.81));
        s.tol = 1e-9;
        s.newton_strategy = NewtonStrategy::TrustRegionSteihaug {
            initial_radius: 0.02,
            max_radius: 0.2,
            min_radius: 1e-12,
            cg_iters: 0,
        };
        let rep = s.step(0.005).unwrap();
        assert!(rep.converged, "{rep:?}");
        assert!(rep.energy_end <= rep.energy_start);
        assert_eq!(rep.factorizations, 0);
    }

    #[test]
    fn generalized_alpha_steps_cleanly_with_contact_without_penetration() {
        let body = Body::new(
            vec![
                Vec3::new(-0.05, -0.05, -0.004),
                Vec3::new(0.08, -0.05, -0.004),
                Vec3::new(-0.05, 0.08, -0.004),
            ],
            vec![[0, 1, 2]],
            0.01,
        )
        .unwrap();
        let params = ContactParams {
            dhat: 1e-3,
            thickness: 2e-4,
            kappa: 50.0,
            mu: 0.2,
            eps_v: 1e-3,
        };
        let rest = vec![Vec2::ZERO, Vec2::new(0.02, 0.0), Vec2::new(0.0, 0.02)];
        let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(200.0, 200.0, 0.1, 40.0, 0.1),
            1e-5,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(model, x, &[[false; 3]; 3], Vec3::new(0.0, 0.0, -9.81))
            .with_contact(body, params)
            .unwrap();
        s.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 0.7 };
        s.tol = 1e-9;
        for _ in 0..15 {
            let rep = s.step(0.004).unwrap();
            assert!(rep.converged, "{rep:?}");
            for p in &s.x {
                assert!(
                    p.z > -0.004 + params.thickness,
                    "cloth penetrated body under GeneralizedAlpha: z={}",
                    p.z
                );
            }
        }
    }

    #[test]
    fn generalized_alpha_refusal_is_transactional_on_acceleration_state() {
        let rest = vec![Vec2::ZERO, Vec2::new(0.02, 0.0), Vec2::new(0.0, 0.02)];
        let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(200.0, 200.0, 0.1, 40.0, 0.1),
            1e-5,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(
            model,
            x.clone(),
            &[[true; 3], [false; 3], [false; 3]],
            Vec3::new(0.0, 0.0, -9.81),
        );
        s.integrator = TimeIntegrator::GeneralizedAlpha { rho_inf: 0.7 };
        assert!(!s.accel_initialized);
        // Refused warm-start length must leave x, v, a, and accel_initialized completely unchanged.
        assert!(s.step_with_guess(0.01, Some(&[Vec3::ZERO])).is_err());
        assert!(!s.accel_initialized);
        assert_eq!(s.x, x);
        assert_eq!(s.v, vec![Vec3::ZERO; 3]);
        assert_eq!(s.a, vec![Vec3::ZERO; 3]);
        // Refused stalled step (max_newton = 0) must likewise leave acceleration uninitialized.
        s.max_newton = 0;
        assert!(s.step(0.01).is_err());
        assert!(!s.accel_initialized);
        assert_eq!(s.x, x);
        assert_eq!(s.a, vec![Vec3::ZERO; 3]);

        // Subnormal time step overflowing Generalized-alpha mass coefficient (1/(beta*h^2))
        // must refuse cleanly and transactionally without initializing acceleration.
        s.max_newton = 30;
        let err_h = s.step(1e-200).unwrap_err();
        assert_eq!(err_h.code, RefusalCode::OutOfRange);
        assert!(!s.accel_initialized);
        assert_eq!(s.x, x);
        assert_eq!(s.a, vec![Vec3::ZERO; 3]);

        // Trust-region static solve matches ProjectedNewton to tight static residual tolerance,
        // and trust-region refusal attaches structured repair hints.
        let mut tr_static = Solver::new(
            s.model.clone(),
            x.clone(),
            &[[true; 3], [true; 3], [false; 3]],
            Vec3::new(0.0, 0.0, -9.81),
        );
        tr_static.newton_strategy = NewtonStrategy::trust_region_default();
        tr_static.tol = 1e-12;
        let rep_tr = tr_static.static_solve().unwrap();
        assert!(rep_tr.converged);
        let mut pn_static = Solver::new(
            s.model.clone(),
            x,
            &[[true; 3], [true; 3], [false; 3]],
            Vec3::new(0.0, 0.0, -9.81),
        );
        pn_static.tol = 1e-12;
        pn_static.static_solve().unwrap();
        let diff = (tr_static.x[2] - pn_static.x[2]).norm();
        assert!(diff < 1e-10, "static TR vs PN diff={diff}");

        let mut tr_refuse = tr_static;
        tr_refuse.max_newton = 0;
        let err = tr_refuse.step(0.01).unwrap_err();
        assert_eq!(err.code, RefusalCode::NotConverged);
        assert!(!err.repairs.is_empty());
    }

    #[test]
    fn adaptive_ilu_verifies_convergence_at_tight_tolerance_and_tr_refuses_truncated_cg() {
        // Stiff-membrane / soft-gravity triangle: initial membrane stretch makes in-plane forces
        // dominate ‖g‖₂ by >1e3 over gravity, so a loose 1e-2 Eisenstat–Walker solve must re-verify
        // at PCG_TOL before certifying convergence.
        let rest = vec![Vec2::ZERO, Vec2::new(0.1, 0.0), Vec2::new(0.0, 0.1)];
        let x0 = vec![
            Vec3::ZERO,
            Vec3::new(0.1, 0.0, 0.0),
            Vec3::new(0.0, 0.100_005, 0.0),
        ];
        let fixed = [[true; 3], [true; 3], [false; 3]];
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(1.0e6, 1.0e6, 0.2, 2.0e5, 0.1),
            1e-5,
            0.2,
        )
        .unwrap();
        let mut pn = Solver::new(
            model.clone(),
            x0.clone(),
            &fixed,
            Vec3::new(0.0, 0.0, -9.81),
        );
        pn.tol = 1e-10;
        pn.step(0.01).unwrap();

        let mut ilu = Solver::new(model, x0, &fixed, Vec3::new(0.0, 0.0, -9.81));
        ilu.tol = 1e-10;
        ilu.newton_strategy = NewtonStrategy::adaptive_ilu_default();
        let rep = ilu.step(0.01).unwrap();
        assert!(rep.converged, "{rep:?}");
        assert!(
            (pn.x[2] - ilu.x[2]).norm() < 1e-9,
            "AdaptiveIlu stopped prematurely on soft mode: pn={:?}, ilu={:?}",
            pn.x[2],
            ilu.x[2]
        );

        // Trust-region Steihaug with cg_iters = 1 on a coupled sheet must not falsely certify
        // convergence when CG stops short of solving the Newton system.
        let mut tr = hanging_sheet();
        tr.tol = 1e-12;
        tr.max_newton = 3;
        tr.newton_strategy = NewtonStrategy::TrustRegionSteihaug {
            initial_radius: 0.05,
            max_radius: 0.5,
            min_radius: 1e-12,
            cg_iters: 1,
        };
        let err = tr.step(0.01).unwrap_err();
        assert_eq!(err.code, RefusalCode::NotConverged);
    }

    #[test]
    fn trust_region_rejected_interior_step_shrinks_radius_relative_to_step_norm() {
        // Free triangle above a stiff contact plane with a huge initial trust radius (1_000 m):
        // the first Newton step is far smaller than `initial_radius` and lands inside the barrier
        // (or CCD-clipped inside it), causing a rejection. Shrinking `delta` only by `0.25 * delta`
        // would spin for >=10 identical rejected iterations; shrinking to `min(0.25*delta, 0.25*||p||)`
        // immediately constrains the next Steihaug step and converges within a tight budget.
        let body = Body::new(
            vec![
                Vec3::new(-0.05, -0.05, -0.002),
                Vec3::new(0.15, -0.05, -0.002),
                Vec3::new(-0.05, 0.15, -0.002),
            ],
            vec![[0, 1, 2]],
            0.01,
        )
        .unwrap();
        let params = ContactParams {
            dhat: 5e-4,
            thickness: 1e-4,
            kappa: 1e4,
            mu: 0.0,
            eps_v: 1e-3,
        };
        let rest = vec![Vec2::ZERO, Vec2::new(0.04, 0.0), Vec2::new(0.0, 0.04)];
        let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(200.0, 200.0, 0.1, 40.0, 0.1),
            1e-5,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(
            model,
            x,
            &[[true; 3], [true; 3], [false; 3]],
            Vec3::new(0.0, 0.0, -50.0),
        )
        .with_contact(body, params)
        .unwrap();
        s.tol = 1e-9;
        s.max_newton = 20;
        s.newton_strategy = NewtonStrategy::TrustRegionSteihaug {
            initial_radius: 1_000.0,
            max_radius: 1_000.0,
            min_radius: 1e-12,
            cg_iters: 0,
        };
        let rep = s.step(0.05).unwrap();
        assert!(rep.converged, "{rep:?}");
        assert!(
            rep.halvings >= 1 && rep.halvings < 8,
            "expected at least one rejected-step contraction without spinning on delta=1000m: {rep:?}"
        );
        assert!(
            s.x[2].z > -0.002 + params.thickness,
            "cloth penetrated contact plane: z={}",
            s.x[2].z
        );
    }

    #[test]
    fn max_abs_or_nan_propagates_nan_and_static_search_rejects_nan_diagonal() {
        assert_eq!(max_abs_or_nan(&[]), 0.0);
        assert_eq!(max_abs_or_nan(&[-3.5, 2.0, 1.0]), 3.5);
        assert!(max_abs_or_nan(&[f64::NAN, 2.0]).is_nan());
        assert!(max_abs_or_nan(&[1.0, f64::NAN, 2.0]).is_nan());
        assert!(max_abs_or_nan(&[1.0, f64::INFINITY]).is_infinite());

        let rest = vec![Vec2::ZERO, Vec2::new(0.04, 0.0), Vec2::new(0.0, 0.04)];
        let x: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let model = ClothModel::new(
            rest,
            &[[0, 1, 2]],
            Membrane::orthotropic(200.0, 200.0, 0.1, 40.0, 0.1),
            1e-5,
            0.2,
        )
        .unwrap();
        let mut s = Solver::new(
            model,
            x,
            &[[true; 3], [true; 3], [false; 3]],
            Vec3::new(0.0, 0.0, -9.81),
        );
        let mut coo = crate::sparse::Coo::new(3, 3);
        coo.push(0, 0, f64::NAN);
        coo.push(1, 1, 10.0);
        coo.push(2, 2, 10.0);
        let a = coo.assemble();
        let mut rep = SolveReport::default();
        assert!(
            s.static_search_direction(&a, &[1.0, 1.0, 1.0], &mut rep)
                .unwrap()
                .is_none()
        );
    }
}

/// Infinity norm of a slice that propagates any `NaN` rather than absorbing it into `0.0`.
fn max_abs_or_nan(xs: &[f64]) -> f64 {
    let mut best = 0.0_f64;
    for &x in xs {
        let a = x.abs();
        if a.is_nan() {
            return f64::NAN;
        }
        if a > best {
            best = a;
        }
    }
    best
}

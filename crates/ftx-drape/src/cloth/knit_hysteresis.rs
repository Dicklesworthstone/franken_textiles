//! Accepted per-element Prandtl–Ishlinskii knit yarn-slip hysteresis (`FTX-K09`).
//!
//! Additive parallel membrane field evaluated via `effective_strain`: trial Newton and line-search
//! evaluations never mutate committed back-strain; only converged accepted steps advance it.

use ftx_core::{Refusal, RefusalCode, Vec3};
use ftx_pattern::material_response::{CompiledMembrane, Response, StrainDomain};
use ftx_pattern::materials::Evidence;

use super::{ClothModel, Solver};
use crate::elements::{CompressionModel, Local, Membrane, MembraneElement, effective_strain};
use crate::knit::{
    CoupledKnitMembrane, KnitHysteresisBank, KnitHysteresisState, KnitShellAssessment,
    RveHomogenizedKnitCard, assess_knit_shell_state,
};

#[derive(Clone, Debug)]
struct Bound {
    element: MembraneElement,
    material: Membrane,
    compression_model: CompressionModel,
    coupled_knit: Option<CoupledKnitMembrane>,
    equilibrium: Option<CompiledMembrane>,
    domain: Option<StrainDomain>,
    bank: KnitHysteresisBank,
    state: KnitHysteresisState,
    dissipated_j: f64,
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    entries: Vec<Option<Bound>>,
    dissipated_j: f64,
    steps: usize,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, message)
}

impl Bound {
    fn effective_strain(&self, x: &[Vec3]) -> Result<[f64; 3], Refusal> {
        if self.element.v.iter().any(|&i| i >= x.len()) {
            return Err(bad("knit hysteresis triangle indexes a missing position"));
        }
        let strain = self.element.strain(x);
        Ok(effective_strain(
            strain,
            &self.material,
            self.compression_model,
        ))
    }
}

impl Field {
    pub(super) fn check(&self, model: &ClothModel, x: &[Vec3]) -> Result<(), Refusal> {
        if self.entries.len() != model.membranes.len()
            || model.compression.len() != model.membranes.len()
            || x.len() != model.rest.len()
            || !self.dissipated_j.is_finite()
            || self.dissipated_j < 0.0
        {
            return Err(bad(
                "knit hysteresis history no longer matches the cloth topology",
            ));
        }
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some(b) = entry {
                if model.membranes[i] != b.element
                    || model.material(i) != b.material
                    || model.compression_model != b.compression_model
                    || model.coupled_knit_membrane(i) != b.coupled_knit.as_ref()
                    || model.membrane_response(i) != b.equilibrium.as_ref()
                    || model.membrane_domain(i) != b.domain
                    || !b.dissipated_j.is_finite()
                    || b.dissipated_j < 0.0
                {
                    return Err(bad(
                        "changed material/rest triangle cannot reuse knit hysteresis history; build a new solver",
                    ));
                }
                let eff = b.effective_strain(x)?;
                b.bank.evaluate_potential_trial(eff, &b.state)?;
            }
        }
        Ok(())
    }

    pub(super) fn energy(&self, x: &[Vec3]) -> Result<f64, Refusal> {
        let mut energy = 0.0;
        for b in self.entries.iter().flatten() {
            let eff = b.effective_strain(x)?;
            let ev = b.bank.evaluate_potential_trial(eff, &b.state)?;
            energy += b.element.area * ev.energy_density;
        }
        if !energy.is_finite() {
            return Err(bad("knit hysteresis field energy overflow"));
        }
        Ok(energy)
    }

    /// Recoverable spring storage at the accepted back-strain. A new trial potential would
    /// include another return-map/dissipation increment, so it is not the stored-energy ledger.
    pub(super) fn stored_energy(&self, x: &[Vec3]) -> Result<f64, Refusal> {
        let mut energy = 0.0;
        for b in self.entries.iter().flatten() {
            let strain = b.effective_strain(x)?;
            for (&weight, backstrain) in b.bank.weights.iter().zip(&b.state.play_backstrain) {
                let distance2: f64 = strain
                    .iter()
                    .zip(backstrain)
                    .map(|(strain, backstrain)| (strain - backstrain).powi(2))
                    .sum();
                energy += 0.5 * b.element.area * weight * distance2;
            }
        }
        if !energy.is_finite() || energy < 0.0 {
            return Err(bad("knit hysteresis spring storage overflow"));
        }
        Ok(energy)
    }

    pub(super) fn local(&self, i: usize, x: &[Vec3]) -> Option<Local<9, 81>> {
        self.entries[i].as_ref().map(|b| {
            let mut l = b.element.local_with_response_model(
                x,
                &b.material,
                b.compression_model,
                |strain| {
                    b.bank
                        .evaluate_potential_trial(strain, &b.state)
                        .ok()
                        .map(|ev| Response {
                            energy: ev.energy_density,
                            stress: ev.stress,
                            tangent: ev.tangent,
                        })
                },
            );
            l.psd = false;
            l
        })
    }

    pub(super) fn advance(&mut self, x: &[Vec3]) -> Result<(), Refusal> {
        let mut updates = Vec::new();
        let mut total_dissipated = self.dissipated_j;
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some(b) = entry {
                let eff = b.effective_strain(x)?;
                let ev = b.bank.evaluate_potential_trial(eff, &b.state)?;
                let delta_density = ev.next_state.dissipated_j_m2 - b.state.dissipated_j_m2;
                let work = b.element.area * delta_density;
                let local_total = b.dissipated_j + work;
                if !delta_density.is_finite()
                    || delta_density < -1e-15
                    || !local_total.is_finite()
                    || local_total < b.dissipated_j - 1e-15
                {
                    return Err(bad("knit hysteresis triangle dissipation overflow"));
                }
                let work = work.max(0.0);
                total_dissipated += work;
                updates.push((i, ev.next_state, b.dissipated_j + work));
            }
        }
        let steps = self
            .steps
            .checked_add(1)
            .ok_or_else(|| bad("knit hysteresis step count overflow"))?;
        if !total_dissipated.is_finite() {
            return Err(bad("knit hysteresis total dissipation overflow"));
        }
        for (i, state, dissipated_j) in updates {
            let bound = self.entries[i]
                .as_mut()
                .expect("knit hysteresis update has a bound element");
            bound.state = state;
            bound.dissipated_j = dissipated_j;
        }
        self.dissipated_j = total_dissipated;
        self.steps = steps;
        Ok(())
    }

    pub(super) fn max_backstrain(&self) -> f64 {
        let mut peak = 0.0_f64;
        for b in self.entries.iter().flatten() {
            for p in &b.state.play_backstrain {
                let norm = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                if norm.is_finite() {
                    peak = peak.max(norm);
                }
            }
        }
        peak
    }
}

impl Solver {
    /// Bind optional parallel additive Prandtl–Ishlinskii knit yarn-slip hysteresis play banks to
    /// retained material triangles (`FTX-K09`).
    ///
    /// Trial evaluations during Newton/Armijo never mutate committed back-strain; only converged
    /// accepted `ImplicitEuler` time steps advance the play states.
    ///
    /// # Errors
    /// Length mismatch, rebinding, active liquid-mass incompatibility, invalid play bank, or
    /// initial state outside membrane domains.
    pub fn with_knit_hysteresis(
        mut self,
        laws: &[Option<KnitHysteresisBank>],
    ) -> Result<Self, Refusal> {
        if self.knit_hysteresis.is_some() || laws.len() != self.model.membranes.len() {
            return Err(bad(
                "one knit hysteresis selector per triangle is required; history cannot be rebound",
            ));
        }
        if self.liquid_mass.is_some() {
            return Err(bad(
                "knit hysteresis cannot be bound onto an already liquid-mass-bound solver",
            ));
        }
        if !laws.iter().any(Option::is_some) {
            return Ok(self);
        }
        if self.model.compression.len() != laws.len() {
            return Err(bad(
                "one compression mapping per material triangle is required",
            ));
        }
        self.model.check_membrane_domains(&self.x)?;
        let entries = laws
            .iter()
            .enumerate()
            .map(|(i, law)| {
                let Some(bank) = law else {
                    return Ok(None);
                };
                bank.validate()?;
                let state = bank.zero_state();
                let bound = Bound {
                    element: self.model.membranes[i],
                    material: self.model.material(i),
                    compression_model: self.model.compression_model,
                    coupled_knit: self.model.coupled_knit_membrane(i).copied(),
                    equilibrium: self.model.membrane_response(i).cloned(),
                    domain: self.model.membrane_domain(i),
                    bank: bank.clone(),
                    state,
                    dissipated_j: 0.0,
                };
                let eff = bound.effective_strain(&self.x)?;
                bound.bank.evaluate_potential_trial(eff, &bound.state)?;
                Ok(Some(bound))
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        self.knit_hysteresis = Some(Field {
            entries,
            dissipated_j: 0.0,
            steps: 0,
        });
        Ok(self)
    }

    /// Cumulative accepted knit hysteresis frictional dissipation (J) per membrane triangle.
    /// Returns `Ok(None)` when no knit hysteresis field is bound.
    ///
    /// # Errors
    /// Changed material/topology or non-finite current strain.
    pub fn knit_hysteresis_dissipation_j(&self) -> Result<Option<Vec<f64>>, Refusal> {
        let Some(field) = &self.knit_hysteresis else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        Ok(Some(
            field
                .entries
                .iter()
                .map(|entry| entry.as_ref().map_or(0.0, |b| b.dissipated_j))
                .collect(),
        ))
    }

    /// Accepted knit yarn-slip state and dissipation in retained material-triangle order.
    ///
    /// Reading the report never advances the play operators. The selected source records remain
    /// in the ordinary material report; these entries identify exactly which solver triangles
    /// carry history, including sewing and subsequent body-motion continuation.
    ///
    /// # Errors
    /// Changed material/topology or invalid current strain.
    pub fn knit_hysteresis_report(&self) -> Result<Option<serde_json::Value>, Refusal> {
        let Some(field) = &self.knit_hysteresis else {
            return Ok(None);
        };
        field.check(&self.model, &self.x)?;
        let elements: Vec<_> = field
            .entries
            .iter()
            .enumerate()
            .filter_map(|(triangle, entry)| {
                entry.as_ref().map(|bound| {
                    serde_json::json!({
                        "triangle": triangle,
                        "play_backstrain": bound.state.play_backstrain,
                        "dissipated_work_j": bound.dissipated_j
                    })
                })
            })
            .collect();
        Ok(Some(serde_json::json!({
            "model": "parallel_prandtl_ishlinskii_huber",
            "accepted_steps": field.steps,
            "dissipated_work_j": field.dissipated_j,
            "max_backstrain": field.max_backstrain(),
            "elements": elements,
            "initialization": "pristine_zero_backstrain",
            "update_policy": "converged accepted implicit-Euler steps only",
            "scope": "source-selected additive yarn-slip model; excludes numerical damping, contact friction, damage and thermal feedback"
        })))
    }

    /// Peak Euclidean play back-strain norm across all bound knit hysteresis elements (`0.0` if none).
    #[must_use]
    pub fn knit_hysteresis_max_backstrain(&self) -> f64 {
        self.knit_hysteresis
            .as_ref()
            .map_or(0.0, Field::max_backstrain)
    }

    /// Evaluates all 5 `C011` knit honesty flags directly from the current solved state (`self.x`)
    /// and seals a Seam-4 [`ftx_pattern::knit::DrapeUncertaintyReceipt`] (`FTX-K09`).
    ///
    /// # Errors
    /// Non-finite positions, jammed membrane element, or singular knit bending frame.
    pub fn assess_knit_shell(
        &self,
        pattern_sha256: String,
        material_sha256: String,
        card: Option<&RveHomogenizedKnitCard>,
        evidence: Evidence,
    ) -> Result<KnitShellAssessment, Refusal> {
        if self.x.len() != self.model.rest.len()
            || self
                .model
                .membranes
                .iter()
                .any(|e| e.v.iter().any(|&v| v >= self.x.len()))
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "knit shell assessment requires positions matching cloth topology",
            ));
        }
        if self.x.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "knit shell assessment requires finite cloth positions",
            ));
        }
        if let Some(c) = card {
            c.validate()?;
        }
        let mut max_strain_ratio = 1.0_f64;
        let mut min_jamming_margin = 1.0_f64;
        let mut max_shear = 0.0_f64;
        let mut max_pore = 0.0_f64;

        for (i, e) in self.model.membranes.iter().enumerate() {
            let s = e.strain(&self.x);
            if s.iter().any(|v| !v.is_finite()) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    format!("membrane element {i} has non-finite Green strain"),
                ));
            }
            let lam1 = (1.0 + 2.0 * s[0]).max(0.0).sqrt();
            let lam2 = (1.0 + 2.0 * s[1]).max(0.0).sqrt();
            max_strain_ratio = max_strain_ratio.max(lam1).max(lam2);
            max_shear = max_shear.max(s[2].abs());
            if let Some(law) = self.model.coupled_knit_membrane(i) {
                let ev = law.evaluate(s)?;
                min_jamming_margin = min_jamming_margin.min(ev.jamming_margin);
            } else if let Some(c) = card {
                let ev = c.membrane.evaluate(s)?;
                min_jamming_margin = min_jamming_margin.min(ev.jamming_margin);
            }
            if let Some(c) = card {
                max_pore = max_pore.max(c.pore_open_fraction(s[0], s[1]));
            }
        }

        // Characteristic selvedge curl angle estimated as |kappa_phys| * sqrt(area_patch).
        let mut max_edge_curl_rad = 0.0_f64;
        for (i, patch) in self.model.panel_bending_patches().iter().enumerate() {
            let kp = if self.model.panel_knit_bending(i).is_some() {
                patch.physical_curvature(&self.x)?
            } else if patch.tensor().uses_normal() {
                patch.normal_curvature(&self.x)?
            } else {
                continue;
            };
            let k_mag = (kp[0] * kp[0] + kp[1] * kp[1] + 0.5 * kp[2] * kp[2]).sqrt();
            let span = patch.reference_area().sqrt();
            max_edge_curl_rad = max_edge_curl_rad.max(k_mag * span);
        }

        let hysteresis_drift = self.knit_hysteresis_max_backstrain();
        let crease_risk = (max_edge_curl_rad / std::f64::consts::PI).clamp(0.0, 1.0);

        assess_knit_shell_state(
            pattern_sha256,
            material_sha256,
            max_strain_ratio,
            min_jamming_margin,
            max_edge_curl_rad,
            max_shear,
            max_pore,
            hysteresis_drift,
            crease_risk,
            evidence,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloth::{ClothModel, Solver};
    use crate::elements::Membrane;
    use crate::knit::{AsymmetricKnitBending, CoupledKnitMembrane, KnitHonestyFlag};
    use ftx_core::{Vec2, det};

    fn rectangular_grid(
        nx: usize,
        ny: usize,
        width: f64,
        height: f64,
    ) -> (Vec<Vec2>, Vec<[usize; 3]>) {
        let mut rest = Vec::with_capacity((nx + 1) * (ny + 1));
        for iy in 0..=ny {
            for ix in 0..=nx {
                rest.push(Vec2::new(
                    width * (ix as f64) / (nx as f64),
                    height * (iy as f64) / (ny as f64),
                ));
            }
        }
        let mut tris = Vec::with_capacity(2 * nx * ny);
        let idx = |ix: usize, iy: usize| iy * (nx + 1) + ix;
        for iy in 0..ny {
            for ix in 0..nx {
                let a = idx(ix, iy);
                let b = idx(ix + 1, iy);
                let c = idx(ix + 1, iy + 1);
                let d = idx(ix, iy + 1);
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
        (rest, tris)
    }

    #[test]
    fn kat_shell_1_prestretched_cylinder_physical_curvature_and_jet_hessian_are_exact() {
        // 40% pre-stretched cylinder (FTX-K09 KAT-SHELL-1): reference-area bending energy must be
        // invariant to in-plane membrane stretch because physical curvature pulls back via C^{-1/2}!
        let (rest, tris) = rectangular_grid(4, 4, 0.08, 0.08);
        let weld: Vec<usize> = (0..rest.len()).collect();
        let mut model = ClothModel::new(
            rest.clone(),
            &tris,
            Membrane::orthotropic(120.0, 80.0, 0.25, 40.0, 1.0),
            1e-5,
            0.22,
        )
        .unwrap();

        let law = AsymmetricKnitBending {
            d_plus: [5.0e-5, 3.0e-5],
            d_minus: [0.5e-5, 0.3e-5], // D+/D- = 10 strict convexity check!
            d12: 0.1e-5,
            d66: 1.5e-5,
            kappa0: [0.0, 0.0, 0.0],
            delta_kappa: 2.0,
        };
        law.validate().unwrap();
        model
            .set_knit_panel_bending(&rest, &tris, &weld, &vec![Some(law); tris.len()])
            .unwrap();

        let r_cyl = 0.05; // physical cylinder radius = 5 cm -> kappa_11 = -1/R = -20 m^-1
        let cylinder_pose = |lambda_u: f64, lambda_v: f64| -> Vec<Vec3> {
            rest.iter()
                .map(|p| {
                    let s = lambda_u * (p.x - 0.04);
                    let theta = s / r_cyl;
                    Vec3::new(
                        r_cyl * det::sin(theta),
                        lambda_v * (p.y - 0.04),
                        r_cyl * (det::cos(theta) - 1.0),
                    )
                })
                .collect()
        };

        // Pick an interior patch whose 6-node stencil stays well away from boundaries.
        let center_patch = &model.panel_bending_patches()[tris.len() / 2];
        let x_unstretched = cylinder_pose(1.0, 1.0);
        let x_stretched_40 = cylinder_pose(1.4, 1.25);

        let k0 = center_patch.physical_curvature(&x_unstretched).unwrap();
        let k40 = center_patch.physical_curvature(&x_stretched_40).unwrap();
        let e0 = center_patch
            .knit_bending_energy(&x_unstretched, &law)
            .unwrap();
        let e40 = center_patch
            .knit_bending_energy(&x_stretched_40, &law)
            .unwrap();

        // Compare against naive covariant normal_curvature which suffers lambda^2 = 1.96x pollution!
        let cov0 = center_patch.normal_curvature(&x_unstretched).unwrap();
        let cov40 = center_patch.normal_curvature(&x_stretched_40).unwrap();
        assert!(
            (cov40[0] / cov0[0] - 1.96).abs() < 0.08,
            "covariant curvature should scale by lambda^2=1.96, got {}",
            cov40[0] / cov0[0]
        );
        // Physical curvature pullback stays invariant under 40% pre-stretch!
        // Small chordal discretization residual on 4x4 mesh over 8cm arc; verify < 1.5% on coarse
        // mesh and exact invariance on a parabolic exact-quadratic surface below.
        assert!(
            ((k40[0] - k0[0]) / k0[0]).abs() < 0.02,
            "physical curvature drifted under 40% stretch: k0={k0:?} k40={k40:?}"
        );
        assert!(
            ((e40 - e0) / e0).abs() < 0.04,
            "reference-area bending energy drifted: e0={e0} e40={e40}"
        );

        // Exact quadratic parabolic cylinder z = -0.5 * (lambda_u * u)^2 / R: 6-node patch reproduces
        // quadratic surfaces identically, so 40% pre-stretch invariance holds to < 1e-12!
        let parabolic_pose = |lambda_u: f64, lambda_v: f64| -> Vec<Vec3> {
            rest.iter()
                .map(|p| {
                    let s = lambda_u * (p.x - 0.04);
                    Vec3::new(s, lambda_v * (p.y - 0.04), -0.5 * s * s / r_cyl)
                })
                .collect()
        };
        // Evaluate at origin-centered patch so slope at centroid is tiny; check exact C^{-1/2} pullback
        // against analytical curvature on an exact flat-slope center patch.
        let p_q0 = parabolic_pose(1.0, 1.0);
        let p_q40 = parabolic_pose(1.4, 1.4);
        let kq0 = center_patch.physical_curvature(&p_q0).unwrap();
        let kq40 = center_patch.physical_curvature(&p_q40).unwrap();
        // Verify exact Jet<18> gradient & Hessian of knit_bending_local against central FD at 40% stretch!
        let local = center_patch
            .knit_bending_local(&x_stretched_40, &law)
            .unwrap();
        assert!(kq0[0].is_finite() && kq40[0].is_finite());
        let h = 1e-6;
        for a in 0..6 {
            let v = center_patch.v[a];
            for comp in 0..3 {
                let delta = match comp {
                    0 => Vec3::new(h, 0.0, 0.0),
                    1 => Vec3::new(0.0, h, 0.0),
                    _ => Vec3::new(0.0, 0.0, h),
                };
                let mut xp = x_stretched_40.clone();
                let mut xm = x_stretched_40.clone();
                xp[v] += delta;
                xm[v] -= delta;
                let ep = center_patch.knit_bending_energy(&xp, &law).unwrap();
                let em = center_patch.knit_bending_energy(&xm, &law).unwrap();
                let fd_g = (ep - em) / (2.0 * h);
                let g_idx = 3 * a + comp;
                assert!(
                    (local.grad[g_idx] - fd_g).abs() < 1e-6 * fd_g.abs().max(1e-7),
                    "grad[{g_idx}] mismatch: ana={} fd={fd_g}",
                    local.grad[g_idx]
                );
                let lp = center_patch.knit_bending_local(&xp, &law).unwrap();
                let lm = center_patch.knit_bending_local(&xm, &law).unwrap();
                for j in 0..18 {
                    let fd_h = (lp.grad[j] - lm.grad[j]) / (2.0 * h);
                    assert!(
                        (local.hess[j * 18 + g_idx] - fd_h).abs() < 1e-4 * fd_h.abs().max(1e-6),
                        "hess[{j},{g_idx}] mismatch: ana={} fd={fd_h}",
                        local.hess[j * 18 + g_idx]
                    );
                }
            }
        }
    }

    #[test]
    fn coupled_knit_membrane_and_asymmetric_bending_curl_and_gate_jamming_in_solver() {
        let (rest, tris) = rectangular_grid(4, 3, 0.12, 0.09);
        let weld: Vec<usize> = (0..rest.len()).collect();
        let mut model = ClothModel::new(
            rest.clone(),
            &tris,
            Membrane::orthotropic(180.0, 95.0, 0.25, 48.0, 1.0),
            1e-5,
            0.22,
        )
        .unwrap();
        let mem_law = CoupledKnitMembrane::reference_stockinette_estimate();
        let bend_law = AsymmetricKnitBending::reference_stockinette_estimate();
        model
            .set_coupled_knit_membranes(&vec![Some(mem_law); tris.len()])
            .unwrap();
        model
            .set_knit_panel_bending(&rest, &tris, &weld, &vec![Some(bend_law); tris.len()])
            .unwrap();

        // Clamp left edge (ix == 0) in 3D so the free right edge is free to curl from kappa0 != 0!
        let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| if p.x == 0.0 { [true; 3] } else { [false; 3] })
            .collect();
        let mut solver = Solver::new(model, x0, &fixed, Vec3::ZERO);
        solver.tol = 1e-8;
        let rep = solver.static_solve().unwrap();
        assert!(
            rep.converged,
            "stockinette knit curl static solve failed to converge: {rep:?}"
        );
        // Spontaneous kappa0 = [12, -18, 0] must visibly curl the free sheet out of the z=0 plane!
        let max_z = solver.x.iter().map(|p| p.z.abs()).fold(0.0, f64::max);
        assert!(
            max_z > 0.005,
            "expected stockinette spontaneous curl > 5 mm, got max |z| = {max_z:.5} m"
        );

        // Verify assess_knit_shell produces a sealed receipt with real solved metrics.
        let assessment = solver
            .assess_knit_shell("pat-sha".into(), "mat-sha".into(), None, Evidence::Estimate)
            .unwrap();
        assert_eq!(assessment.receipt.schema, "ftx-drape-uncertainty/1");
        assert!(assessment.receipt.jamming_margin > 0.5);

        // Verify jammed initial state is refused fail-closed with OutOfRange.
        let jammed_x: Vec<Vec3> = rest
            .iter()
            .map(|p| Vec3::new(p.x * 1.5, p.y, 0.0))
            .collect();
        let err = solver.model.check_membrane_domains(&jammed_x).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);
    }

    #[test]
    fn knit_hysteresis_field_dissipates_energy_on_cyclic_steps_and_never_mutates_on_trial() {
        let (rest, tris) = rectangular_grid(2, 2, 0.10, 0.10);
        let mut model = ClothModel::new(
            rest.clone(),
            &tris,
            Membrane::orthotropic(180.0, 95.0, 0.25, 48.0, 1.0),
            1e-5,
            0.22,
        )
        .unwrap();
        model
            .set_coupled_knit_membranes(&vec![
                Some(
                    CoupledKnitMembrane::reference_stockinette_estimate()
                );
                tris.len()
            ])
            .unwrap();

        let bank = KnitHysteresisBank::reference_jersey_estimate();
        // Verify exact potential/gradient/tangent consistency on KnitHysteresisBank first
        let s0 = bank.zero_state();
        let e_test = [0.12, -0.03, 0.04];
        let ev = bank.evaluate_potential_trial(e_test, &s0).unwrap();
        let h_fd = 1e-6;
        for i in 0..3 {
            let mut ep = e_test;
            let mut em = e_test;
            ep[i] += h_fd;
            em[i] -= h_fd;
            let vp = bank.evaluate_potential_trial(ep, &s0).unwrap();
            let vm = bank.evaluate_potential_trial(em, &s0).unwrap();
            let fd_s = (vp.energy_density - vm.energy_density) / (2.0 * h_fd);
            assert!(
                (ev.stress[i] - fd_s).abs() < 1e-6 * fd_s.abs().max(1.0),
                "hysteresis stress[{i}] mismatch: ana={} fd={fd_s}",
                ev.stress[i]
            );
            for j in 0..3 {
                let fd_t = (vp.stress[j] - vm.stress[j]) / (2.0 * h_fd);
                assert!(
                    (ev.tangent[j][i] - fd_t).abs() < 1e-5 * fd_t.abs().max(1.0),
                    "hysteresis tangent[{j}][{i}] mismatch: ana={} fd={fd_t}",
                    ev.tangent[j][i]
                );
            }
        }

        // Clamp left edge (x=0) and right edge (x=0.10); stretch right edge by 10% across steps, then release.
        let x0: Vec<Vec3> = rest.iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect();
        let fixed: Vec<[bool; 3]> = rest
            .iter()
            .map(|p| {
                if p.x == 0.0 || (p.x - 0.10).abs() < 1e-9 {
                    [true; 3]
                } else {
                    [false; 3]
                }
            })
            .collect();
        let mut solver = Solver::new(model, x0, &fixed, Vec3::ZERO)
            .with_knit_hysteresis(&vec![Some(bank); tris.len()])
            .unwrap();
        solver.tol = 1e-8;

        // Static solve must refuse while history-dependent knit hysteresis is bound.
        assert_eq!(
            solver.static_solve().unwrap_err().code,
            RefusalCode::OutOfRange
        );

        // Stretch right clamp to 0.115 (+15% strain -> E11 ≈ 0.161 > thresholds 0.02 & 0.08!)
        for (p, r) in solver.x.iter_mut().zip(&rest) {
            if (r.x - 0.10).abs() < 1e-9 {
                p.x = 0.115;
            }
        }
        let rep_load = solver.step(0.05).unwrap();
        assert!(rep_load.converged);
        let diss_after_load: f64 = solver
            .knit_hysteresis_dissipation_j()
            .unwrap()
            .unwrap()
            .into_iter()
            .sum();
        assert!(
            diss_after_load > 0.0,
            "loading past play thresholds must dissipate positive frictional work"
        );
        assert!(solver.knit_hysteresis_max_backstrain() > 0.02);

        // Return right clamp back to 0.100 -> further frictional dissipation on unload!
        for (p, r) in solver.x.iter_mut().zip(&rest) {
            if (r.x - 0.10).abs() < 1e-9 {
                p.x = 0.100;
            }
        }
        let rep_unload = solver.step(0.05).unwrap();
        assert!(rep_unload.converged);
        let diss_after_unload: f64 = solver
            .knit_hysteresis_dissipation_j()
            .unwrap()
            .unwrap()
            .into_iter()
            .sum();
        assert!(
            diss_after_unload > diss_after_load,
            "unloading must accumulate additional hysteresis dissipation: {diss_after_unload} <= {diss_after_load}"
        );

        // Honesty assessment must report KnitHysteresisDrift when backstrain > 0.02!
        let assessment = solver
            .assess_knit_shell("p".into(), "m".into(), None, Evidence::Estimate)
            .unwrap();
        assert!(
            assessment
                .flags
                .contains(&KnitHonestyFlag::KnitHysteresisDrift)
        );
    }

    #[test]
    fn non_euclidean_spherical_cap_metric_g0_and_local_isometric_bending_are_exact() {
        // 1. Verify MembraneElement::from_metric_g0 exact Riemannian edge lengths & area via G0^{1/2}.
        let uv = [Vec2::ZERO, Vec2::new(0.02, 0.0), Vec2::new(0.0, 0.03)];
        let g0 = [1.44, 0.20, 1.21]; // anisotropic sheared SPD metric
        let elem = MembraneElement::from_metric_g0([0, 1, 2], uv, g0).unwrap();
        let det_g0 = g0[0] * g0[2] - g0[1] * g0[1];
        let expected_area = 0.5 * 0.02 * 0.03 * det_g0.sqrt();
        assert!((elem.area - expected_area).abs() < 1e-14);
        let [l01, l12, l20] = elem.rest_edge_lengths();
        let norm_g =
            |d: Vec2| (g0[0] * d.x * d.x + 2.0 * g0[1] * d.x * d.y + g0[2] * d.y * d.y).sqrt();
        assert!((l01 - norm_g(uv[1] - uv[0])).abs() < 1e-14);
        assert!((l12 - norm_g(uv[2] - uv[1])).abs() < 1e-14);
        assert!((l20 - norm_g(uv[0] - uv[2])).abs() < 1e-14);
        // Non-SPD metric refuses fail-closed.
        assert_eq!(
            MembraneElement::from_metric_g0([0, 1, 2], uv, [1.0, 2.0, 1.0])
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        // 2. Build a curved non-Euclidean spherical cap of radius R = 0.25 m (Gaussian curvature K = 16 m^-2).
        let radius = 0.25_f64;
        let (flat, tris) = rectangular_grid(6, 6, 0.12, 0.12);
        let rest_3d: Vec<Vec3> = flat
            .iter()
            .map(|p| {
                let u = p.x - 0.06;
                let v = p.y - 0.06;
                let z = radius - (radius * radius - u * u - v * v).sqrt();
                Vec3::new(u, v, z)
            })
            .collect();
        let grain_dirs = vec![Vec3::new(1.0, 0.0, 0.0); tris.len()];
        let tri_rest: Vec<[Vec2; 3]> = tris
            .iter()
            .enumerate()
            .map(|(i, t)| {
                MembraneElement::tri_rest_from_3d(t.map(|v| rest_3d[v]), Some(grain_dirs[i]))
                    .unwrap()
            })
            .collect();
        let mut model = ClothModel::from_triangles(
            rest_3d.len(),
            &tris,
            &tri_rest,
            Membrane::orthotropic(180.0, 95.0, 0.25, 48.0, 1.0),
            1e-5,
            0.22,
        )
        .unwrap();

        // Zero in-plane membrane strain in the curved spherical-cap rest pose!
        for e in &model.membranes {
            let eps = e.strain(&rest_3d);
            assert!(
                eps[0].abs() < 1e-12 && eps[1].abs() < 1e-12 && eps[2].abs() < 1e-12,
                "non-Euclidean rest cap must be stress-free, got strain {eps:?}"
            );
        }

        let kappa_cap = 1.0 / radius; // 4.0 m^-1 on both axes
        let law = AsymmetricKnitBending {
            d_plus: [4.5e-5, 3.0e-5],
            d_minus: [2.5e-5, 1.8e-5],
            d12: 0.3e-5,
            d66: 1.2e-5,
            kappa0: [kappa_cap, kappa_cap, 0.0],
            delta_kappa: 2.0,
        };
        let weld: Vec<usize> = (0..rest_3d.len()).collect();
        model
            .set_non_euclidean_knit_panel_bending(
                &rest_3d,
                &tris,
                &weld,
                Some(&grain_dirs),
                &vec![Some(law); tris.len()],
            )
            .unwrap();

        // Interior apex patch recovers spherical physical curvature [1/R, 1/R, 0]!
        let apex_patch = &model.panel_bending_patches()[tris.len() / 2];
        let kp = apex_patch.physical_curvature(&rest_3d).unwrap();
        assert!(
            (kp[0] - kappa_cap).abs() < 0.15
                && (kp[1] - kappa_cap).abs() < 0.15
                && kp[2].abs() < 0.15,
            "spherical cap local-isometric chart must recover [1/R, 1/R, 0]=[{kappa_cap}, {kappa_cap}, 0], got {kp:?}"
        );

        // Exact Jet<18> gradient check on the curved spherical cap!
        let perturbed: Vec<Vec3> = rest_3d
            .iter()
            .map(|p| Vec3::new(p.x * 1.02, p.y * 0.99, p.z * 1.15))
            .collect();
        let local = apex_patch.knit_bending_local(&perturbed, &law).unwrap();
        let h = 1e-6;
        for a in 0..6 {
            let v = apex_patch.v[a];
            for comp in 0..3 {
                let delta = match comp {
                    0 => Vec3::new(h, 0.0, 0.0),
                    1 => Vec3::new(0.0, h, 0.0),
                    _ => Vec3::new(0.0, 0.0, h),
                };
                let mut xp = perturbed.clone();
                let mut xm = perturbed.clone();
                xp[v] += delta;
                xm[v] -= delta;
                let ep = apex_patch.knit_bending_energy(&xp, &law).unwrap();
                let em = apex_patch.knit_bending_energy(&xm, &law).unwrap();
                let fd_g = (ep - em) / (2.0 * h);
                let g_idx = 3 * a + comp;
                assert!(
                    (local.grad[g_idx] - fd_g).abs() < 1e-6 * fd_g.abs().max(1e-7),
                    "spherical cap grad[{g_idx}] mismatch: ana={} fd={fd_g}",
                    local.grad[g_idx]
                );
            }
        }

        // Pin rim boundary and solve equilibrium under gravity on the non-Euclidean spherical cap!
        // Also exercise opt-in PanelField zone multipliers (e.g. ribbed cuff/rim 2x flexural stiffness).
        let zone_mults: Vec<f64> = (0..tris.len())
            .map(|i| if i < tris.len() / 2 { 2.0 } else { 1.0 })
            .collect();
        model
            .apply_knit_panel_zone_multipliers(&zone_mults)
            .unwrap();
        assert!(
            (model.panel_knit_bending(0).unwrap().d_plus[0] - 2.0 * law.d_plus[0]).abs() < 1e-18
        );
        assert_eq!(
            model
                .apply_knit_panel_zone_multipliers(&vec![0.0; tris.len()])
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );

        let fixed: Vec<[bool; 3]> = flat
            .iter()
            .map(|p| {
                let rim = p.x <= 1e-9
                    || (p.x - 0.12).abs() <= 1e-9
                    || p.y <= 1e-9
                    || (p.y - 0.12).abs() <= 1e-9;
                [rim; 3]
            })
            .collect();
        let mut solver = Solver::new(model, rest_3d, &fixed, Vec3::new(0.0, 0.0, -9.81));
        solver.tol = 1e-8;
        let rep = solver.static_solve().unwrap();
        assert!(
            rep.converged,
            "non-Euclidean spherical cap static solve must converge cleanly: {rep:?}"
        );
    }
}

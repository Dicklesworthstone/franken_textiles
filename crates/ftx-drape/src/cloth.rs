//! The cloth model and its Newton solver (plan §11.2–11.3, without contact yet).
//!
//! Each time step minimises the incremental potential
//! `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane(x) + Ψ_bend(x)` with `x̃ = xₙ + h vₙ + h² g` and lumped mass,
//! by projected Newton: element Hessians projected to positive semidefinite, a sparse LDLᵀ solve whose
//! symbolic analysis (ordered on the vertex graph) is computed once, and an Armijo backtracking line
//! search, so the potential never rises and a strained start relaxes instead of exploding. A static
//! solve minimises `Ψ − Σ mᵢ g·xᵢ` the same way. Units: metres, kilograms, seconds.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};

use crate::eigen::project_psd;
use crate::elements::{Hinge, Membrane, MembraneElement};
use crate::sparse::{Coo, Csr, LdltOptions, SymbolicLdlt, amd_order};

/// Edge (lo, hi) → its uses as (edge start, edge end, opposite vertex, triangle).
type EdgeUses = std::collections::BTreeMap<(usize, usize), Vec<(usize, usize, usize, usize)>>;

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
    /// Membrane material.
    pub membrane: Membrane,
    /// Bending stiffness B (N·m, flexural rigidity per unit width).
    pub bending: f64,
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
        if areal_density.is_nan() || areal_density <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("areal density {areal_density} kg/m² must be positive"),
            ));
        }
        let n = rest.len();
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
            let e = MembraneElement::new(*tri, [rest[tri[0]], rest[tri[1]], rest[tri[2]]])
                .ok_or_else(|| {
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
                    let h = Hinge::new(v, v.map(|i| rest[i])).ok_or_else(|| {
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
        for (h, [t1, t2]) in hinges.iter_mut().zip(hinge_tris) {
            h.set_area(
                membranes[t1].area / interior[t1] as f64 + membranes[t2].area / interior[t2] as f64,
            );
        }
        Ok(Self {
            rest,
            membranes,
            hinges,
            mass,
            membrane,
            bending,
        })
    }

    /// Elastic energy Ψ (membrane + bending) at positions `x`.
    #[must_use]
    pub fn elastic_energy(&self, x: &[Vec3]) -> f64 {
        let m: f64 = self
            .membranes
            .iter()
            .map(|e| e.energy(x, &self.membrane))
            .sum();
        let b: f64 = self.hinges.iter().map(|h| h.energy(x, self.bending)).sum();
        m + b
    }
}

/// What one minimisation did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
    /// Newton iteration cap per solve.
    pub max_newton: usize,
    free: Vec<Option<usize>>,
    nfree: usize,
    symbolic: Option<SymbolicLdlt>,
}

impl Solver {
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
        Self {
            v: vec![Vec3::ZERO; n],
            model,
            x,
            gravity,
            tol: 1e-6,
            max_newton: 100,
            free,
            nfree,
            symbolic: None,
        }
    }

    /// Free degrees of freedom.
    #[must_use]
    pub fn free_dofs(&self) -> usize {
        self.nfree
    }

    /// The potential: incremental (`Some((x̃, h))`) or static (`None`, gravity as potential).
    fn potential(&self, x: &[Vec3], inertia: Option<(&[Vec3], f64)>) -> f64 {
        let mut e = self.model.elastic_energy(x);
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
        e
    }

    /// Gradient over free DOFs and the Hessian (every element block pushed, so the pattern is fixed),
    /// with element blocks projected to positive semidefinite when `project` is set.
    fn assemble(
        &self,
        x: &[Vec3],
        inertia: Option<(&[Vec3], f64)>,
        project: bool,
    ) -> (Vec<f64>, Csr) {
        let mut g = vec![0.0; self.nfree];
        let mut coo = Coo::new(self.nfree, self.nfree);
        let scatter =
            |dofs: &[usize], grad: &[f64], hess: &mut [f64], coo: &mut Coo, g: &mut [f64]| {
                let n = 3 * dofs.len();
                if project {
                    project_psd(hess, n);
                }
                for a in 0..n {
                    let Some(fa) = self.free[3 * dofs[a / 3] + a % 3] else {
                        continue;
                    };
                    g[fa] += grad[a];
                    for b in 0..n {
                        if let Some(fb) = self.free[3 * dofs[b / 3] + b % 3] {
                            coo.push(fa, fb, hess[a * n + b]);
                        }
                    }
                }
            };
        for e in &self.model.membranes {
            let mut l = e.local(x, &self.model.membrane);
            scatter(&e.v, &l.grad, &mut l.hess, &mut coo, &mut g);
        }
        for h in &self.model.hinges {
            let mut l = h.local(x, self.model.bending);
            scatter(&h.v, &l.grad, &mut l.hess, &mut coo, &mut g);
        }
        for (i, &m) in self.model.mass.iter().enumerate() {
            for k in 0..3 {
                let Some(f) = self.free[3 * i + k] else {
                    continue;
                };
                match inertia {
                    Some((xt, h)) => {
                        g[f] += m * (x[i][k] - xt[i][k]) / (h * h);
                        coo.push(f, f, m / (h * h));
                    }
                    None => {
                        g[f] -= m * self.gravity[k];
                        coo.push(f, f, 0.0);
                    }
                }
            }
        }
        (g, coo.assemble())
    }

    fn symbolic(&mut self, a: &Csr) -> Result<&SymbolicLdlt, Refusal> {
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
        }
        self.symbolic
            .as_ref()
            .ok_or_else(|| Refusal::new(RefusalCode::NotConverged, "no symbolic analysis"))
    }

    fn apply(&self, x: &[Vec3], p: &[f64], alpha: f64) -> Vec<Vec3> {
        let mut out = x.to_vec();
        for (i, xi) in out.iter_mut().enumerate() {
            let d = Vec3::new(
                self.free[3 * i].map_or(0.0, |f| p[f]),
                self.free[3 * i + 1].map_or(0.0, |f| p[f]),
                self.free[3 * i + 2].map_or(0.0, |f| p[f]),
            );
            *xi += d * alpha;
        }
        out
    }

    /// Projected Newton on the potential from the current positions.
    fn minimize(&mut self, inertia: Option<(&[Vec3], f64)>) -> Result<SolveReport, Refusal> {
        let mut rep = SolveReport {
            energy_start: self.potential(&self.x, inertia),
            ..SolveReport::default()
        };
        let mut energy = rep.energy_start;
        if self.nfree == 0 {
            rep.converged = true;
            rep.energy_end = energy;
            return Ok(rep);
        }
        let scale = inertia.map_or(1.0, |(_, h)| h);
        for it in 0..self.max_newton {
            // Exact Hessian first (quadratic convergence near a minimum); element blocks projected to
            // PSD only when the factorization reports negative inertia (plan §11.2).
            let (mut g, a) = self.assemble(&self.x, inertia, false);
            let mut p = self.newton_direction(&a, &g)?;
            if p.is_none() {
                rep.projected_iterations += 1;
                let (g2, a2) = self.assemble(&self.x, inertia, true);
                g = g2;
                p = self.newton_direction(&a2, &g)?;
            }
            // Singular even after projection: steepest descent.
            let mut p = p.unwrap_or_else(|| g.iter().map(|v| -v).collect());
            let mut slope: f64 = g.iter().zip(&p).map(|(a, b)| a * b).sum();
            if slope.is_nan() || slope >= 0.0 {
                p = g.iter().map(|v| -v).collect();
                slope = -g.iter().map(|v| v * v).sum::<f64>();
            }
            let step = p.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            rep.iterations = it + 1;
            rep.last_step = step;
            rep.residual = g.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if step / scale < self.tol {
                rep.converged = true;
                break;
            }
            let mut alpha = 1.0;
            let mut accepted = false;
            for _ in 0..50 {
                let trial = self.apply(&self.x, &p, alpha);
                let e = self.potential(&trial, inertia);
                if e <= energy + 1e-4 * alpha * slope {
                    self.x = trial;
                    energy = e;
                    accepted = true;
                    break;
                }
                alpha *= 0.5;
            }
            if !accepted {
                rep.line_search_failures += 1;
                break;
            }
        }
        rep.energy_end = energy;
        if !rep.converged {
            let (g, _) = self.assemble(&self.x, inertia, false);
            rep.residual = g.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        }
        Ok(rep)
    }

    /// The Newton direction `−A⁻¹g`, or `None` when `A` is singular or not positive definite.
    fn newton_direction(&mut self, a: &Csr, g: &[f64]) -> Result<Option<Vec<f64>>, Refusal> {
        let sym = self.symbolic(a)?;
        Ok(match sym.factor(a, &LdltOptions::default()) {
            Ok(f) if f.inertia().negative == 0 => {
                Some(f.solve(&g.iter().map(|v| -v).collect::<Vec<_>>()))
            }
            _ => None,
        })
    }

    /// One implicit-Euler step of size `h` (s).
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] for a non-positive step; [`RefusalCode::NonFinite`] when the state
    /// becomes non-finite; a failed symbolic analysis.
    pub fn step(&mut self, h: f64) -> Result<SolveReport, Refusal> {
        if h.is_nan() || h <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("time step {h} s must be positive"),
            ));
        }
        let x0 = self.x.clone();
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
        let rep = self.minimize(Some((&xt, h)))?;
        if self.x.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth state became non-finite",
            ));
        }
        self.v = self.x.iter().zip(&x0).map(|(a, b)| (*a - *b) / h).collect();
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

    /// Minimises `Ψ − Σ mᵢ g·xᵢ` from the current positions (equilibrium under gravity) and zeroes the
    /// velocities.
    ///
    /// # Errors
    /// As [`Solver::step`].
    pub fn static_solve(&mut self) -> Result<SolveReport, Refusal> {
        let rep = self.minimize(None)?;
        if self.x.iter().any(|p| !p.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the cloth state became non-finite",
            ));
        }
        self.v = vec![Vec3::ZERO; self.x.len()];
        Ok(rep)
    }

    /// Internal elastic force on each vertex (−∂Ψ/∂x), e.g. reactions at constrained DOFs.
    #[must_use]
    pub fn elastic_forces(&self) -> Vec<Vec3> {
        let mut f = vec![Vec3::ZERO; self.x.len()];
        for e in &self.model.membranes {
            let l = e.local(&self.x, &self.model.membrane);
            for (a, &vi) in e.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        for h in &self.model.hinges {
            let l = h.local(&self.x, self.model.bending);
            for (a, &vi) in h.v.iter().enumerate() {
                f[vi] -= Vec3::new(l.grad[3 * a], l.grad[3 * a + 1], l.grad[3 * a + 2]);
            }
        }
        f
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

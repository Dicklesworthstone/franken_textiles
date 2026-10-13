//! The cloth model and its Newton solver (plan §11.2–11.3, without contact yet).
//!
//! Each time step minimises the incremental potential
//! `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane(x) + Ψ_bend(x)` with `x̃ = xₙ + h vₙ + h² g` and lumped mass,
//! by projected Newton: element Hessians projected to positive semidefinite, a sparse LDLᵀ solve whose
//! symbolic analysis (ordered on the vertex graph) is computed once, and an Armijo backtracking line
//! search, so the potential never rises and a strained start relaxes instead of exploding. A static
//! solve minimises `Ψ − Σ mᵢ g·xᵢ` the same way. Units: metres, kilograms, seconds.

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};

use crate::contact::{Body, Contact, ContactParams, Intersection};
use crate::eigen::project_psd;
use crate::elements::{Hinge, Membrane, MembraneElement};
use crate::sparse::{Coo, Csr, CsrFill, LdltOptions, SymbolicLdlt, amd_order};

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
    /// Compression factor of each membrane element, per direction (`membrane.compression` unless set by
    /// [`ClothModel::set_compression`]); [`ClothModel::material`] combines the two.
    pub compression: Vec<[f64; 2]>,
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
    /// closed on itself. A hinge across such a weld maps its second triangle rigidly onto the first.
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
        if areal_density.is_nan() || areal_density <= 0.0 {
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
                    // The stencil in the first triangle's rest frame; the second triangle's opposite
                    // vertex mapped by the rigid motion that takes its copy of the edge onto the first's.
                    let at = |t: usize, i: usize| {
                        tri_rest[t][tris[t].iter().position(|&j| j == i).unwrap_or(0)]
                    };
                    let (pa, pb, po1) = (at(*t1, *a), at(*t1, *b), at(*t1, *o1));
                    let (qa, qb, qo2) = (at(*t2, *a), at(*t2, *b), at(*t2, *o2));
                    let po2 = if (qa, qb) == (pa, pb) {
                        qo2 // the same frame (no weld): use it as is, bit for bit
                    } else {
                        let (u, w) = ((pb - pa).normalized(), (qb - qa).normalized());
                        let (cos, sin) = (u.dot(w), w.cross(u));
                        let r = qo2 - qa;
                        pa + Vec2::new(cos * r.x - sin * r.y, sin * r.x + cos * r.y)
                    };
                    let h = Hinge::new(v, [pa, pb, po1, po2]).ok_or_else(|| {
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
            rest: rest.into_iter().map(Option::unwrap_or_default).collect(),
            compression: vec![membrane.compression; membranes.len()],
            membranes,
            hinges,
            mass,
            membrane,
            bending,
        })
    }

    /// Sets each membrane element's compression factor (both directions), e.g. softer where gathers
    /// draw the cloth up.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] unless there is one factor per element, each in [0, 1].
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
        // 0 is a tension-only membrane, as `Membrane::compression` allows
        if let Some(c) = per_element.iter().find(|c| !(0.0..=1.0).contains(*c)) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("compression factor {c} must be in [0, 1]"),
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
            ..self.membrane
        }
    }

    /// Elastic energy Ψ (membrane + bending) at positions `x`.
    #[must_use]
    pub fn elastic_energy(&self, x: &[Vec3]) -> f64 {
        let m: f64 = self
            .membranes
            .iter()
            .enumerate()
            .map(|(i, e)| e.energy(x, &self.material(i)))
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
    /// Contact with a static body, if any.
    pub contact: Option<Contact>,
    /// Friction lag rounds per step after the first solve (IPC; 1 is usually enough).
    pub friction_rounds: usize,
    free: Vec<Option<usize>>,
    nfree: usize,
    symbolic: Option<SymbolicLdlt>,
    /// The Hessian's pattern, fixed once the symbolic analysis is done.
    pattern: Option<Csr>,
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
            contact: None,
            friction_rounds: 1,
            free,
            nfree,
            symbolic: None,
            pattern: None,
        }
    }

    /// Adds contact with a static body. The cloth must start clear of it (IPC needs an
    /// intersection-free start).
    ///
    /// # Errors
    /// [`RefusalCode::IntersectingStart`] when some cloth–body gap is not positive, an edge of either
    /// mesh passes through the other, or a piece lies inside a closed body;
    /// [`RefusalCode::NonFinite`] for a non-finite position.
    pub fn with_contact(mut self, body: Body, params: ContactParams) -> Result<Self, Refusal> {
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
        });
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
        self.contact = Some(contact);
        Ok(self)
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
        if let Some(c) = &self.contact {
            e += c.terms(x, false, inertia.is_some()).energy;
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
        let mut sink = match &self.pattern {
            Some(p) => Sink::Fill(CsrFill::new(p)),
            None => Sink::Coo(Coo::new(self.nfree, self.nfree)),
        };
        // Elements whose Hessian is PSD by construction (every hinge; membranes without compression
        // or a negative stress) are not projected: it would change nothing but rounding.
        let scatter = |dofs: &[usize],
                       grad: &[f64],
                       hess: &mut [f64],
                       psd: bool,
                       sink: &mut Sink,
                       g: &mut [f64]| {
            let n = 3 * dofs.len();
            if project && !psd {
                project_psd(hess, n);
            }
            for a in 0..n {
                let Some(fa) = self.free[3 * dofs[a / 3] + a % 3] else {
                    continue;
                };
                g[fa] += grad[a];
                for b in 0..n {
                    if let Some(fb) = self.free[3 * dofs[b / 3] + b % 3] {
                        sink.push(fa, fb, hess[a * n + b]);
                    }
                }
            }
        };
        for (i, e) in self.model.membranes.iter().enumerate() {
            let mut l = e.local(x, &self.model.material(i));
            scatter(&e.v, &l.grad, &mut l.hess, l.psd, &mut sink, &mut g);
        }
        for h in &self.model.hinges {
            let mut l = h.local(x, self.model.bending);
            scatter(&h.v, &l.grad, &mut l.hess, l.psd, &mut sink, &mut g);
        }
        for (i, &m) in self.model.mass.iter().enumerate() {
            for k in 0..3 {
                let Some(f) = self.free[3 * i + k] else {
                    continue;
                };
                match inertia {
                    Some((xt, h)) => {
                        g[f] += m * (x[i][k] - xt[i][k]) / (h * h);
                        sink.push(f, f, m / (h * h));
                    }
                    None => {
                        g[f] -= m * self.gravity[k];
                        sink.push(f, f, 0.0);
                    }
                }
            }
        }
        if let Some(c) = &self.contact {
            // Mollified edge pairs carry their full, indefinite Hessian and are projected here; the
            // Gauss–Newton barrier and friction blocks are PSD already. A block's cloth vertices share
            // an element, so the pattern does not change.
            for mut block in c.terms(x, true, inertia.is_some()).blocks {
                let m = block.dofs.len();
                if !block.psd {
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
            // every later assembly fills this pattern in place
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
        // Once a solve has met negative inertia it keeps projecting: retrying the exact Hessian first
        // cost a second assembly and factorization on almost every iteration of a garment.
        let mut projecting = false;
        for it in 0..self.max_newton {
            // Exact Hessian first (quadratic convergence near a minimum); element blocks projected to
            // PSD only when the factorization reports negative inertia (plan §11.2).
            let (mut g, mut p) = if projecting {
                (Vec::new(), None)
            } else {
                let (g, a) = self.assemble(&self.x, inertia, false);
                let p = self.newton_direction(&a, &g)?;
                (g, p)
            };
            if p.is_none() {
                projecting = true;
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
            // Never step through the body: cap α by additive CCD along the Newton direction.
            let mut alpha = match &self.contact {
                Some(c) => c.step_bound(&self.x, &self.displacement(&p)),
                None => 1.0,
            };
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
        if let Some(c) = &mut self.contact {
            c.update_lag(&x0, &x0, h); // friction pairs frozen at the start of the step
        }
        let mut rep = self.minimize(Some((&xt, h)))?;
        // Lag rounds (IPC): refresh normal forces and contact sets at the new iterate and re-solve,
        // so a contact made during the step has friction within the same step.
        for _ in 0..self.friction_rounds {
            let Some(c) = &mut self.contact else { break };
            if c.params.mu <= 0.0 {
                break;
            }
            c.update_lag(&x0, &self.x, h);
            let again = self.minimize(Some((&xt, h)))?;
            rep.iterations += again.iterations;
            rep.projected_iterations += again.projected_iterations;
            rep.line_search_failures += again.line_search_failures;
            rep.converged = again.converged;
            rep.energy_end = again.energy_end;
            rep.residual = again.residual;
            rep.last_step = again.last_step;
        }
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
        for (i, e) in self.model.membranes.iter().enumerate() {
            let l = e.local(&self.x, &self.model.material(i));
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

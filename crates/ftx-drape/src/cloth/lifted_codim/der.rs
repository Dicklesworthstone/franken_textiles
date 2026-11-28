//! 4-DOF Discrete Elastic Rod (DER) kinematics over parallel-transported
//! Bishop frames (Bergou et al. 2008 / 2010), with exact analytical axial,
//! bending, and material-twist gradients and Hessians and fail-closed
//! `DegenerateSegment` / `AntiparallelTangents` guards.

use super::section_laws::{RodSectionLaw, RodSectionModel, SectionHistory};
use super::{CodimError, DetMath, all_finite3, cross3, dot3, norm3, scale3, sub3};

/// 4-DOF Discrete Elastic Rod curve state and constitutive specification.
#[derive(Clone, Debug, PartialEq)]
pub struct DerCurve {
    /// Nodal positions `x_i` in metres (`N >= 2`, or `N >= 3` if `closed`).
    pub positions: Vec<[f64; 3]>,
    /// Closed loop flag (`true` connects last node back to node 0).
    pub closed: bool,
    /// Per-edge scalar material twist angles `theta_e` in radians (`E` edges).
    pub edge_twists_rad: Vec<f64>,
    /// Reference rest edge lengths `l0_e > 0` in metres (`E`).
    pub rest_lengths_m: Vec<f64>,
    /// Reference edge unit tangents `t_e^0` from the last accepted configuration (`E`).
    pub ref_tangents: Vec<[f64; 3]>,
    /// Reference edge Bishop unit normals `u_e^0` orthogonal to `t_e^0` (`E`).
    pub ref_normals_u: Vec<[f64; 3]>,
    /// Persistent joint reference-twist offsets `m_{j,0}` in radians (`J` joints).
    pub ref_joint_twists_rad: Vec<f64>,
    /// Rest material bending curvatures `[kappa_1_0, kappa_2_0]` (`m^-1`) (`J`).
    pub rest_curvatures: Vec<[f64; 2]>,
    /// Rest twist rate `tau_0` (`rad/m`) (`J`).
    pub rest_twists_rad_m: Vec<f64>,
    /// Constitutive section model per edge (`E`).
    pub edge_sections: Vec<RodSectionModel>,
}

/// Complete DER network state (collection of [`DerCurve`]s).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DerState {
    /// Curves in the DER network.
    pub curves: Vec<DerCurve>,
}

/// Evaluation of a [`DerCurve`]: total strain energy, analytical gradient over
/// `[x_0, ..., x_{N-1}, theta_0, ..., theta_{E-1}]`, sparse symmetric Hessian
/// triplets, and candidate next section histories (`E` edge histories + `J`
/// joint histories).
#[derive(Clone, Debug, PartialEq)]
pub struct DerEval {
    /// Total elastic/stored strain energy in Joules.
    pub energy_j: f64,
    /// Analytical gradient wrt `[positions (3*N), edge_twists (E)]`.
    pub grad: Vec<f64>,
    /// Symmetric local Hessian triplets `(row, col, val)` in `[0..3*N + E)`.
    pub hess_triplets: Vec<(usize, usize, f64)>,
    /// Candidate edge axial histories (committed only on step acceptance).
    pub next_edge_histories: Vec<SectionHistory>,
    /// Candidate joint bending/twist histories (committed only on step acceptance).
    pub next_joint_histories: Vec<SectionHistory>,
}

fn initial_normal(t: [f64; 3]) -> [f64; 3] {
    let ax = t[0].abs();
    let ay = t[1].abs();
    let az = t[2].abs();
    let ref_axis = if ax <= ay && ax <= az {
        [1.0, 0.0, 0.0]
    } else if ay <= az {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let proj = sub3(ref_axis, scale3(t, dot3(ref_axis, t)));
    let len2 = dot3(proj, proj);
    // Exact sqrt via Newton not needed since callers use DetMath in constructors;
    // normalize using caller-supplied DetMath in `from_polyline`.
    let _ = len2;
    proj
}

fn parallel_transport_vec<M: DetMath>(
    u: [f64; 3],
    t0: [f64; 3],
    t1: [f64; 3],
    joint_idx: usize,
) -> Result<[f64; 3], CodimError> {
    let c = dot3(t0, t1);
    if c <= -1.0 + 1e-12 {
        return Err(CodimError::AntiparallelTangents { joint_idx });
    }
    let denom = 1.0 + c;
    let coeff = dot3(u, t1) / denom;
    let raw = sub3(
        u,
        scale3([t0[0] + t1[0], t0[1] + t1[1], t0[2] + t1[2]], coeff),
    );
    let ortho = sub3(raw, scale3(t1, dot3(raw, t1)));
    let len = norm3::<M>(ortho);
    if len <= 1e-15 {
        return Err(CodimError::AntiparallelTangents { joint_idx });
    }
    Ok(scale3(ortho, 1.0 / len))
}

impl DerCurve {
    /// Number of edges (`N` if closed, `N - 1` if open).
    #[must_use]
    pub fn edge_count(&self) -> usize {
        if self.closed {
            self.positions.len()
        } else {
            self.positions.len().saturating_sub(1)
        }
    }

    /// Number of internal/closing joints (`E` if closed, `E - 1` if open).
    #[must_use]
    pub fn joint_count(&self) -> usize {
        if self.closed {
            self.edge_count()
        } else {
            self.edge_count().saturating_sub(1)
        }
    }

    /// Total local DOFs (`3 * positions.len() + edge_count()`).
    #[must_use]
    pub fn dof_count(&self) -> usize {
        3 * self.positions.len() + self.edge_count()
    }

    /// Construct a [`DerCurve`] from an initial polyline, initializing Bishop
    /// reference frames and rest lengths/curvatures.
    pub fn from_polyline<M: DetMath>(
        positions: Vec<[f64; 3]>,
        closed: bool,
        edge_twists_rad: Vec<f64>,
        edge_sections: Vec<RodSectionModel>,
    ) -> Result<Self, CodimError> {
        let n = positions.len();
        if (closed && n < 3) || (!closed && n < 2) {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve requires >= 2 nodes (open) or >= 3 nodes (closed)",
            });
        }
        for &p in &positions {
            if !all_finite3(p) {
                return Err(CodimError::NonFiniteInput {
                    context: "DerCurve::positions",
                });
            }
        }
        let e_count = if closed { n } else { n - 1 };
        if edge_twists_rad.len() != e_count || edge_sections.len() != e_count {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve edge_twists_rad and edge_sections length mismatch",
            });
        }
        if !edge_twists_rad.iter().all(|v| v.is_finite()) {
            return Err(CodimError::NonFiniteInput {
                context: "DerCurve::edge_twists_rad",
            });
        }
        for sec in &edge_sections {
            sec.validate()?;
        }

        let mut rest_lengths_m = Vec::with_capacity(e_count);
        let mut ref_tangents = Vec::with_capacity(e_count);
        for e in 0..e_count {
            let a = e;
            let b = (e + 1) % n;
            let d = sub3(positions[b], positions[a]);
            let l = norm3::<M>(d);
            if !l.is_finite() || l <= 1e-15 {
                return Err(CodimError::DegenerateSegment { segment_idx: e });
            }
            rest_lengths_m.push(l);
            ref_tangents.push(scale3(d, 1.0 / l));
        }

        let mut ref_normals_u = Vec::with_capacity(e_count);
        let raw_u0 = initial_normal(ref_tangents[0]);
        let len_u0 = norm3::<M>(raw_u0);
        ref_normals_u.push(scale3(raw_u0, 1.0 / len_u0));
        for e in 1..e_count {
            let u = parallel_transport_vec::<M>(
                ref_normals_u[e - 1],
                ref_tangents[e - 1],
                ref_tangents[e],
                e,
            )?;
            ref_normals_u.push(u);
        }

        let j_count = if closed { e_count } else { e_count - 1 };
        let mut ref_joint_twists_rad = vec![0.0; j_count];
        if closed {
            let u_ret = parallel_transport_vec::<M>(
                ref_normals_u[e_count - 1],
                ref_tangents[e_count - 1],
                ref_tangents[0],
                e_count - 1,
            )?;
            let sin_psi = dot3(cross3(u_ret, ref_normals_u[0]), ref_tangents[0]);
            let cos_psi = dot3(u_ret, ref_normals_u[0]);
            ref_joint_twists_rad[e_count - 1] = M::atan2(sin_psi, cos_psi);
        }

        Ok(Self {
            positions,
            closed,
            edge_twists_rad,
            rest_lengths_m,
            ref_tangents,
            ref_normals_u,
            ref_joint_twists_rad,
            rest_curvatures: vec![[0.0; 2]; j_count],
            rest_twists_rad_m: vec![0.0; j_count],
            edge_sections,
        })
    }

    /// Validate structural invariants of this curve.
    pub fn validate<M: DetMath>(&self) -> Result<(), CodimError> {
        let n = self.positions.len();
        if (self.closed && n < 3) || (!self.closed && n < 2) {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve node count invalid",
            });
        }
        let e = self.edge_count();
        let j = self.joint_count();
        if self.edge_twists_rad.len() != e
            || self.rest_lengths_m.len() != e
            || self.ref_tangents.len() != e
            || self.ref_normals_u.len() != e
            || self.edge_sections.len() != e
            || self.ref_joint_twists_rad.len() != j
            || self.rest_curvatures.len() != j
            || self.rest_twists_rad_m.len() != j
        {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve array length mismatch",
            });
        }
        for &p in &self.positions {
            if !all_finite3(p) {
                return Err(CodimError::NonFiniteInput {
                    context: "DerCurve::positions",
                });
            }
        }
        for (&th, &l0) in self.edge_twists_rad.iter().zip(self.rest_lengths_m.iter()) {
            if !th.is_finite() || !l0.is_finite() {
                return Err(CodimError::NonFiniteInput {
                    context: "DerCurve edge data",
                });
            }
            if l0 <= 1e-15 {
                return Err(CodimError::DegenerateSegment { segment_idx: 0 });
            }
        }
        for (idx, (&t, &u)) in self
            .ref_tangents
            .iter()
            .zip(self.ref_normals_u.iter())
            .enumerate()
        {
            if !all_finite3(t) || !all_finite3(u) {
                return Err(CodimError::NonFiniteInput {
                    context: "DerCurve reference frame",
                });
            }
            if norm3::<M>(t) <= 1e-12 || norm3::<M>(u) <= 1e-12 {
                return Err(CodimError::DegenerateSegment { segment_idx: idx });
            }
        }
        for sec in &self.edge_sections {
            sec.validate()?;
        }
        Ok(())
    }

    /// Apply `[dx_0, ..., dx_{N-1}, dtheta_0, ..., dtheta_{E-1}]` scaled by `alpha`.
    pub fn apply_increment(&mut self, delta: &[f64], alpha: f64) -> Result<(), CodimError> {
        if delta.len() != self.dof_count() {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve increment length mismatch",
            });
        }
        let n = self.positions.len();
        for i in 0..n {
            self.positions[i][0] += alpha * delta[3 * i];
            self.positions[i][1] += alpha * delta[3 * i + 1];
            self.positions[i][2] += alpha * delta[3 * i + 2];
        }
        let th_off = 3 * n;
        for e in 0..self.edge_count() {
            self.edge_twists_rad[e] += alpha * delta[th_off + e];
        }
        Ok(())
    }

    /// Update time-parallel Bishop reference frames `(ref_tangents, ref_normals_u,
    /// ref_joint_twists_rad)` strictly upon Newton step acceptance.
    pub fn commit_reference_frames<M: DetMath>(&mut self) -> Result<(), CodimError> {
        let n = self.positions.len();
        let e_count = self.edge_count();
        let mut new_t = Vec::with_capacity(e_count);
        let mut new_u = Vec::with_capacity(e_count);
        for e in 0..e_count {
            let a = e;
            let b = (e + 1) % n;
            let d = sub3(self.positions[b], self.positions[a]);
            let l = norm3::<M>(d);
            if l <= 1e-15 {
                return Err(CodimError::DegenerateSegment { segment_idx: e });
            }
            let t = scale3(d, 1.0 / l);
            let u = parallel_transport_vec::<M>(self.ref_normals_u[e], self.ref_tangents[e], t, e)?;
            new_t.push(t);
            new_u.push(u);
        }
        let j_count = self.joint_count();
        for j in 0..j_count {
            let e0 = j;
            let e1 = (j + 1) % e_count;
            let u_tr = parallel_transport_vec::<M>(new_u[e0], new_t[e0], new_t[e1], j)?;
            let sin_m = dot3(cross3(u_tr, new_u[e1]), new_t[e1]);
            let cos_m = dot3(u_tr, new_u[e1]);
            self.ref_joint_twists_rad[j] += M::atan2(sin_m, cos_m);
        }
        self.ref_tangents = new_t;
        self.ref_normals_u = new_u;
        Ok(())
    }

    /// Compute edge unit tangent and transported material frame `(t_e, m1_e, m2_e)`
    /// for edge `e` (used by [`super::junction`] for DER arm coupling).
    pub fn edge_material_frame<M: DetMath>(
        &self,
        e: usize,
    ) -> Result<([f64; 3], [f64; 3], [f64; 3]), CodimError> {
        let n = self.positions.len();
        let e_count = self.edge_count();
        if e >= e_count {
            return Err(CodimError::InvalidTopology {
                detail: "DerCurve edge index out of range",
            });
        }
        let a = e;
        let b = (e + 1) % n;
        let d = sub3(self.positions[b], self.positions[a]);
        let l = norm3::<M>(d);
        if l <= 1e-15 {
            return Err(CodimError::DegenerateSegment { segment_idx: e });
        }
        let t = scale3(d, 1.0 / l);
        let u = parallel_transport_vec::<M>(self.ref_normals_u[e], self.ref_tangents[e], t, e)?;
        let v = cross3(t, u);
        let th = self.edge_twists_rad[e];
        let c = M::cos(th);
        let s = M::sin(th);
        let m1 = [
            c * u[0] + s * v[0],
            c * u[1] + s * v[1],
            c * u[2] + s * v[2],
        ];
        let m2 = [
            -s * u[0] + c * v[0],
            -s * u[1] + c * v[1],
            -s * u[2] + c * v[2],
        ];
        Ok((t, m1, m2))
    }
}

// ---------------------------------------------------------------------------
// Stack-allocated 2nd-order forward-mode AD Jet2<N> (zero mul_add, zero libm)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Jet2<const N: usize> {
    v: f64,
    g: [f64; N],
    h: [[f64; N]; N],
}

impl<const N: usize> Jet2<N> {
    fn constant(v: f64) -> Self {
        Self {
            v,
            g: [0.0; N],
            h: [[0.0; N]; N],
        }
    }

    fn var(v: f64, idx: usize) -> Self {
        let mut g = [0.0; N];
        g[idx] = 1.0;
        Self {
            v,
            g,
            h: [[0.0; N]; N],
        }
    }

    fn add(self, rhs: Self) -> Self {
        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = self.g[i] + rhs.g[i];
            for j in i..N {
                let hij = self.h[i][j] + rhs.h[i][j];
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self {
            v: self.v + rhs.v,
            g,
            h,
        }
    }

    fn sub(self, rhs: Self) -> Self {
        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = self.g[i] - rhs.g[i];
            for j in i..N {
                let hij = self.h[i][j] - rhs.h[i][j];
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self {
            v: self.v - rhs.v,
            g,
            h,
        }
    }

    fn scale(self, s: f64) -> Self {
        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = s * self.g[i];
            for j in i..N {
                let hij = s * self.h[i][j];
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self {
            v: s * self.v,
            g,
            h,
        }
    }

    fn mul(self, rhs: Self) -> Self {
        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = self.g[i] * rhs.v + self.v * rhs.g[i];
            for j in i..N {
                let hij = self.h[i][j] * rhs.v
                    + self.g[i] * rhs.g[j]
                    + self.g[j] * rhs.g[i]
                    + self.v * rhs.h[i][j];
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self {
            v: self.v * rhs.v,
            g,
            h,
        }
    }

    fn unary(self, f: f64, df: f64, d2f: f64) -> Self {
        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = df * self.g[i];
            for j in i..N {
                let hij = df * self.h[i][j] + d2f * self.g[i] * self.g[j];
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self { v: f, g, h }
    }

    fn div(self, rhs: Self) -> Self {
        let inv = 1.0 / rhs.v;
        let inv2 = inv * inv;
        let inv3 = inv2 * inv;
        let rec = rhs.unary(inv, -inv2, 2.0 * inv3);
        self.mul(rec)
    }

    fn sin<M: DetMath>(self) -> Self {
        let s = M::sin(self.v);
        let c = M::cos(self.v);
        self.unary(s, c, -s)
    }

    fn cos<M: DetMath>(self) -> Self {
        let c = M::cos(self.v);
        let s = M::sin(self.v);
        self.unary(c, -s, -c)
    }

    fn atan2<M: DetMath>(y: Self, x: Self) -> Self {
        let v = M::atan2(y.v, x.v);
        let r2 = x.v * x.v + y.v * y.v;
        let r4 = r2 * r2;
        let fx = -y.v / r2;
        let fy = x.v / r2;
        let fxx = 2.0 * x.v * y.v / r4;
        let fyy = -fxx;
        let fxy = (y.v * y.v - x.v * x.v) / r4;

        let mut g = [0.0; N];
        let mut h = [[0.0; N]; N];
        for i in 0..N {
            g[i] = fx * x.g[i] + fy * y.g[i];
            for j in i..N {
                let hij = fx * x.h[i][j]
                    + fy * y.h[i][j]
                    + fxx * x.g[i] * x.g[j]
                    + fyy * y.g[i] * y.g[j]
                    + fxy * (x.g[i] * y.g[j] + y.g[i] * x.g[j]);
                h[i][j] = hij;
                h[j][i] = hij;
            }
        }
        Self { v, g, h }
    }
}

fn jet_dot3<const N: usize>(a: [Jet2<N>; 3], b: [Jet2<N>; 3]) -> Jet2<N> {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}

fn jet_cross3<const N: usize>(a: [Jet2<N>; 3], b: [Jet2<N>; 3]) -> [Jet2<N>; 3] {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

fn jet_parallel_transport<const N: usize>(
    u0: [f64; 3],
    t0: [f64; 3],
    t1: [Jet2<N>; 3],
) -> [Jet2<N>; 3] {
    let t0j = [
        Jet2::constant(t0[0]),
        Jet2::constant(t0[1]),
        Jet2::constant(t0[2]),
    ];
    let u0j = [
        Jet2::constant(u0[0]),
        Jet2::constant(u0[1]),
        Jet2::constant(u0[2]),
    ];
    let c = Jet2::constant(1.0).add(jet_dot3(t0j, t1));
    let coeff = jet_dot3(u0j, t1).div(c);
    [
        u0j[0].sub(coeff.mul(t0j[0].add(t1[0]))),
        u0j[1].sub(coeff.mul(t0j[1].add(t1[1]))),
        u0j[2].sub(coeff.mul(t0j[2].add(t1[2]))),
    ]
}

fn jet_parallel_transport_var<const N: usize>(
    u0: [Jet2<N>; 3],
    t0: [Jet2<N>; 3],
    t1: [Jet2<N>; 3],
) -> [Jet2<N>; 3] {
    let c = Jet2::constant(1.0).add(jet_dot3(t0, t1));
    let coeff = jet_dot3(u0, t1).div(c);
    [
        u0[0].sub(coeff.mul(t0[0].add(t1[0]))),
        u0[1].sub(coeff.mul(t0[1].add(t1[1]))),
        u0[2].sub(coeff.mul(t0[2].add(t1[2]))),
    ]
}

/// Evaluate energy, exact analytical gradient, and symmetric Hessian triplets
/// for a single [`DerCurve`] without mutating committed histories.
#[allow(clippy::too_many_lines)]
pub fn evaluate_der_curve<M: DetMath>(
    curve: &DerCurve,
    edge_histories: &[SectionHistory],
    joint_histories: &[SectionHistory],
) -> Result<DerEval, CodimError> {
    curve.validate::<M>()?;
    let n = curve.positions.len();
    let e_count = curve.edge_count();
    let j_count = curve.joint_count();
    if edge_histories.len() != e_count || joint_histories.len() != j_count {
        return Err(CodimError::InvalidTopology {
            detail: "DerCurve history slice lengths mismatch",
        });
    }

    let ndof = curve.dof_count();
    let mut energy_j = 0.0;
    let mut grad = vec![0.0_f64; ndof];
    let mut hess_triplets: Vec<(usize, usize, f64)> = Vec::new();
    let mut next_edge_histories = Vec::with_capacity(e_count);
    let mut next_joint_histories = Vec::with_capacity(j_count);

    let mut tangents = Vec::with_capacity(e_count);
    let mut lengths = Vec::with_capacity(e_count);

    // 1. Axial stretch energy on every edge.
    for e in 0..e_count {
        let a = e;
        let b = (e + 1) % n;
        let d = sub3(curve.positions[b], curve.positions[a]);
        let l = norm3::<M>(d);
        if !l.is_finite() || l <= 1e-15 {
            return Err(CodimError::DegenerateSegment { segment_idx: e });
        }
        let t = scale3(d, 1.0 / l);
        tangents.push(t);
        lengths.push(l);

        let l0 = curve.rest_lengths_m[e];
        let eps = l / l0 - 1.0;
        let (sec, next_h) = curve.edge_sections[e].evaluate_section::<M>(
            eps,
            [0.0, 0.0],
            [0.0, 0.0],
            0.0,
            &edge_histories[e],
        )?;
        next_edge_histories.push(next_h);

        energy_j += l0 * sec.energy_density_j_m;
        let n_force = sec.stress_6[0];
        let k_ax = sec.tangent_6x6[0][0] / l0;

        for c in 0..3 {
            let g_c = n_force * t[c];
            grad[3 * a + c] -= g_c;
            grad[3 * b + c] += g_c;
        }
        let coef_iso = n_force / l;
        for r in 0..3 {
            for c in 0..3 {
                let diag = if r == c { 1.0 } else { 0.0 };
                let k_rc = k_ax * t[r] * t[c] + coef_iso * (diag - t[r] * t[c]);
                let ra = 3 * a + r;
                let ca = 3 * a + c;
                let rb = 3 * b + r;
                let cb = 3 * b + c;
                hess_triplets.push((ra, ca, k_rc));
                hess_triplets.push((rb, cb, k_rc));
                hess_triplets.push((ra, cb, -k_rc));
                hess_triplets.push((rb, ca, -k_rc));
            }
        }
    }

    // 2. Bending and twist energy on every internal/closing joint.
    let th_off = 3 * n;
    for j in 0..j_count {
        let e0 = j;
        let e1 = (j + 1) % e_count;
        let v0 = e0;
        let v1 = e1;
        let v2 = (e1 + 1) % n;

        let t0 = tangents[e0];
        let t1 = tangents[e1];
        if 1.0 + dot3(t0, t1) <= 1e-12 {
            return Err(CodimError::AntiparallelTangents { joint_idx: j });
        }
        if 1.0 + dot3(curve.ref_tangents[e0], t0) <= 1e-12
            || 1.0 + dot3(curve.ref_tangents[e1], t1) <= 1e-12
        {
            return Err(CodimError::AntiparallelTangents { joint_idx: j });
        }

        let l_bar = 0.5 * (curve.rest_lengths_m[e0] + curve.rest_lengths_m[e1]);
        let inv_l_bar = 1.0 / l_bar;

        // 8 local variables: [t0_0, t0_1, t0_2, t1_0, t1_1, t1_2, theta_0, theta_1].
        let jt0 = [
            Jet2::<8>::var(t0[0], 0),
            Jet2::<8>::var(t0[1], 1),
            Jet2::<8>::var(t0[2], 2),
        ];
        let jt1 = [
            Jet2::<8>::var(t1[0], 3),
            Jet2::<8>::var(t1[1], 4),
            Jet2::<8>::var(t1[2], 5),
        ];
        let jth0 = Jet2::<8>::var(curve.edge_twists_rad[e0], 6);
        let jth1 = Jet2::<8>::var(curve.edge_twists_rad[e1], 7);

        let u0 = jet_parallel_transport(curve.ref_normals_u[e0], curve.ref_tangents[e0], jt0);
        let v0_f = jet_cross3(jt0, u0);
        let u1 = jet_parallel_transport(curve.ref_normals_u[e1], curve.ref_tangents[e1], jt1);
        let v1_f = jet_cross3(jt1, u1);

        let c0 = jth0.cos::<M>();
        let s0 = jth0.sin::<M>();
        let c1 = jth1.cos::<M>();
        let s1 = jth1.sin::<M>();

        let mut m1_0 = [Jet2::constant(0.0); 3];
        let mut m2_0 = [Jet2::constant(0.0); 3];
        let mut m1_1 = [Jet2::constant(0.0); 3];
        let mut m2_1 = [Jet2::constant(0.0); 3];
        for c in 0..3 {
            m1_0[c] = c0.mul(u0[c]).add(s0.mul(v0_f[c]));
            m2_0[c] = c0.mul(v0_f[c]).sub(s0.mul(u0[c]));
            m1_1[c] = c1.mul(u1[c]).add(s1.mul(v1_f[c]));
            m2_1[c] = c1.mul(v1_f[c]).sub(s1.mul(u1[c]));
        }

        let denom_kb = Jet2::constant(1.0).add(jet_dot3(jt0, jt1));
        let kb_raw = jet_cross3(jt0, jt1);
        let kb = [
            kb_raw[0].scale(2.0).div(denom_kb),
            kb_raw[1].scale(2.0).div(denom_kb),
            kb_raw[2].scale(2.0).div(denom_kb),
        ];
        let m1_avg = [
            m1_0[0].add(m1_1[0]).scale(0.5),
            m1_0[1].add(m1_1[1]).scale(0.5),
            m1_0[2].add(m1_1[2]).scale(0.5),
        ];
        let m2_avg = [
            m2_0[0].add(m2_1[0]).scale(0.5),
            m2_0[1].add(m2_1[1]).scale(0.5),
            m2_0[2].add(m2_1[2]).scale(0.5),
        ];

        let k1 = jet_dot3(kb, m2_avg)
            .scale(inv_l_bar)
            .sub(Jet2::constant(curve.rest_curvatures[j][0]));
        let k2 = jet_dot3(kb, m1_avg)
            .scale(-inv_l_bar)
            .sub(Jet2::constant(curve.rest_curvatures[j][1]));

        let u0_on_t1 = jet_parallel_transport_var(u0, jt0, jt1);
        let sin_m = jet_dot3(jet_cross3(u0_on_t1, u1), jt1);
        let cos_m = jet_dot3(u0_on_t1, u1);
        let dm = Jet2::atan2::<M>(sin_m, cos_m);
        let m_ref = Jet2::constant(curve.ref_joint_twists_rad[j]).add(dm);
        let tau = jth1
            .sub(jth0)
            .add(m_ref)
            .scale(inv_l_bar)
            .sub(Jet2::constant(curve.rest_twists_rad_m[j]));

        let (sec, next_h) = curve.edge_sections[e0].evaluate_section::<M>(
            0.0,
            [0.0, 0.0],
            [k1.v, k2.v],
            tau.v,
            &joint_histories[j],
        )?;
        next_joint_histories.push(next_h);

        energy_j += l_bar * sec.energy_density_j_m;

        // Combine sec derivatives wrt [tau, k1, k2] = stress_6[3..6], tangent_6x6[3..6, 3..6].
        let q_jets = [tau, k1, k2];
        let f_q = [
            l_bar * sec.stress_6[3],
            l_bar * sec.stress_6[4],
            l_bar * sec.stress_6[5],
        ];
        let mut g8 = [0.0_f64; 8];
        let mut h8 = [[0.0_f64; 8]; 8];
        for i in 0..8 {
            g8[i] = f_q[0] * q_jets[0].g[i] + f_q[1] * q_jets[1].g[i] + f_q[2] * q_jets[2].g[i];
            for col in i..8 {
                let mut acc = f_q[0] * q_jets[0].h[i][col]
                    + f_q[1] * q_jets[1].h[i][col]
                    + f_q[2] * q_jets[2].h[i][col];
                for a_k in 0..3 {
                    for b_k in 0..3 {
                        let cab = l_bar * sec.tangent_6x6[3 + a_k][3 + b_k];
                        acc += cab * q_jets[a_k].g[i] * q_jets[b_k].g[col];
                    }
                }
                h8[i][col] = acc;
                h8[col][i] = acc;
            }
        }

        // Chain rule from z8 = [t0(x0,x1), t1(x1,x2), theta_0, theta_1] to 11 DOFs:
        // [x_v0(3), x_v1(3), x_v2(3), theta_e0(1), theta_e1(1)].
        let l0_cur = lengths[e0];
        let l1_cur = lengths[e1];
        let inv_l0 = 1.0 / l0_cur;
        let inv_l1 = 1.0 / l1_cur;

        let mut jt0_mat = [[0.0_f64; 3]; 3];
        let mut jt1_mat = [[0.0_f64; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                let diag = if r == c { 1.0 } else { 0.0 };
                jt0_mat[r][c] = inv_l0 * (diag - t0[r] * t0[c]);
                jt1_mat[r][c] = inv_l1 * (diag - t1[r] * t1[c]);
            }
        }

        // 8 x 11 Jacobian B_z = d(z8)/d(dofs11).
        let mut bz = [[0.0_f64; 11]; 8];
        for r in 0..3 {
            for c in 0..3 {
                bz[r][c] = -jt0_mat[r][c];
                bz[r][3 + c] = jt0_mat[r][c];
                bz[3 + r][3 + c] = -jt1_mat[r][c];
                bz[3 + r][6 + c] = jt1_mat[r][c];
            }
        }
        bz[6][9] = 1.0;
        bz[7][10] = 1.0;

        let dof_idx = [
            3 * v0,
            3 * v0 + 1,
            3 * v0 + 2,
            3 * v1,
            3 * v1 + 1,
            3 * v1 + 2,
            3 * v2,
            3 * v2 + 1,
            3 * v2 + 2,
            th_off + e0,
            th_off + e1,
        ];

        for c in 0..11 {
            let mut acc = 0.0;
            for r in 0..8 {
                acc += bz[r][c] * g8[r];
            }
            grad[dof_idx[c]] += acc;
        }

        // Material + tangent-level Hessian: B_z^T * H8 * B_z.
        let mut h8_bz = [[0.0_f64; 11]; 8];
        for r in 0..8 {
            for c in 0..11 {
                let mut acc = 0.0;
                for k in 0..8 {
                    acc += h8[r][k] * bz[k][c];
                }
                h8_bz[r][c] = acc;
            }
        }
        let mut h11 = [[0.0_f64; 11]; 11];
        for i in 0..11 {
            for j_col in i..11 {
                let mut acc = 0.0;
                for r in 0..8 {
                    acc += bz[r][i] * h8_bz[r][j_col];
                }
                h11[i][j_col] = acc;
                h11[j_col][i] = acc;
            }
        }

        // Exact curvature of unit tangent normalization d^2(t_e)/d(e_e)^2:
        // sum_k g_k d^2(e_k/||e||)/(de_p de_q) = (-g_p t_q - g_q t_p - (g.t) delta_pq + 3 (g.t) t_p t_q)/l^2.
        let add_norm_hess = |h11: &mut [[f64; 11]; 11],
                             g_t: [f64; 3],
                             t: [f64; 3],
                             l: f64,
                             a_off: usize,
                             b_off: usize| {
            let gt = dot3(g_t, t);
            let inv_l2 = 1.0 / (l * l);
            for p in 0..3 {
                for q in 0..3 {
                    let diag = if p == q { gt } else { 0.0 };
                    let val =
                        (-g_t[p] * t[q] - g_t[q] * t[p] - diag + 3.0 * gt * t[p] * t[q]) * inv_l2;
                    h11[a_off + p][a_off + q] += val;
                    h11[b_off + p][b_off + q] += val;
                    h11[a_off + p][b_off + q] -= val;
                    h11[b_off + p][a_off + q] -= val;
                }
            }
        };
        add_norm_hess(&mut h11, [g8[0], g8[1], g8[2]], t0, l0_cur, 0, 3);
        add_norm_hess(&mut h11, [g8[3], g8[4], g8[5]], t1, l1_cur, 3, 6);

        for i in 0..11 {
            for col in 0..11 {
                let val = h11[i][col];
                if val != 0.0 {
                    hess_triplets.push((dof_idx[i], dof_idx[col], val));
                }
            }
        }
    }

    Ok(DerEval {
        energy_j,
        grad,
        hess_triplets,
        next_edge_histories,
        next_joint_histories,
    })
}

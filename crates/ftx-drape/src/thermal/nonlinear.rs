//! Conservative temperature-dependent finite volumes; never linearize stored heat as Cp(T)*delta T.
//! Conductivities are evaluated at cell centres; refinement remains an explicit accuracy requirement.

use ftx_pattern::material_catalog::ResolvedMaterial;
use ftx_pattern::material_thermal::ThermalLaw;

use super::{Boundary, Refusal, State, invalid, nonnegative, positive, temperature};

pub(super) const ITERATIONS: usize = 256;

#[derive(Clone, Debug)]
pub(super) struct Variable {
    laws: Vec<ThermalLaw>,
    interfaces: Vec<f64>,
    per_layer: usize,
    domain: [f64; 2],
    resistance_factors: Vec<f64>,
    capacity_factors: Vec<f64>,
}

impl Variable {
    pub(super) fn new(
        layers: &[ResolvedMaterial],
        interfaces: &[f64],
        per_layer: usize,
    ) -> Result<Self, Refusal> {
        let laws = layers
            .iter()
            .map(|m| ThermalLaw::from_material(m.material(), m.allows_estimates()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut domain = [-273.15, f64::MAX];
        for law in &laws {
            if let Some([lo, hi]) = law.domain() {
                domain[0] = domain[0].max(lo);
                domain[1] = domain[1].min(hi);
            }
        }
        if domain[0] >= domain[1] {
            return Err(invalid(
                "layer thermal domains have no common temperature interval",
            ));
        }
        Ok(Self {
            resistance_factors: vec![1.0; laws.len()],
            capacity_factors: vec![1.0; laws.len()],
            laws,
            interfaces: interfaces.to_vec(),
            per_layer,
            domain,
        })
    }

    pub(super) fn scale_areas(&mut self, resistance: &[f64], capacity: &[f64]) {
        self.resistance_factors.copy_from_slice(resistance);
        self.capacity_factors.copy_from_slice(capacity);
    }

    pub(super) fn domain(&self) -> [f64; 2] {
        self.domain
    }

    pub(super) fn check(&self, t: f64) -> Result<(), Refusal> {
        temperature(t)?;
        if t < self.domain[0] || t > self.domain[1] {
            return Err(invalid(format!(
                "temperature {t} outside common characterized interval {:?} C; no extrapolation",
                self.domain
            )));
        }
        Ok(())
    }

    pub(super) fn capacity(&self, i: usize, t: f64) -> Result<f64, Refusal> {
        positive(
            "cell heat capacity",
            self.laws[i / self.per_layer].capacity_at(t)? / self.per_layer as f64
                * self.capacity_factors[i / self.per_layer],
        )
    }

    pub(super) fn heat(&self, i: usize, old: f64, new: f64) -> Result<f64, Refusal> {
        Ok(
            self.laws[i / self.per_layer].sensible_change(old, new)? / self.per_layer as f64
                * self.capacity_factors[i / self.per_layer],
        )
    }

    fn conductances(&self, t: &[f64], b: Boundary) -> Result<Vec<f64>, Refusal> {
        let mut r = Vec::with_capacity(t.len());
        for (i, &t) in t.iter().enumerate() {
            r.push(positive(
                "cell resistance",
                self.laws[i / self.per_layer].resistance_at(t)? / self.per_layer as f64
                    * self.resistance_factors[i / self.per_layer],
            )?);
        }
        let mut paths = vec![b.inside_resistance + r[0] / 2.0];
        for i in 1..t.len() {
            let interface = if i % self.per_layer == 0 {
                self.interfaces[i / self.per_layer - 1]
            } else {
                0.0
            };
            paths.push(r[i - 1] / 2.0 + r[i] / 2.0 + interface);
        }
        paths.push(b.outside_resistance + r[t.len() - 1] / 2.0);
        paths
            .into_iter()
            .map(|r| positive("conductance", 1.0 / positive("thermal path", r)?))
            .collect()
    }

    fn fluxes(t: &[f64], g: &[f64], b: Boundary) -> Vec<f64> {
        let mut flux = vec![g[0] * (b.inside_c - t[0])];
        for i in 1..t.len() {
            flux.push(g[i] * (t[i - 1] - t[i]));
        }
        flux.push(g[t.len()] * (t[t.len() - 1] - b.outside_c));
        flux
    }

    // Check the ORIGINAL nonlinear equations, not just the frozen Picard system or a small update.
    fn accepted(
        &self,
        t: &[f64],
        old: Option<&[f64]>,
        seconds: f64,
        g: &[f64],
        b: Boundary,
    ) -> Result<Option<State>, Refusal> {
        let flux = Self::fluxes(t, g, b);
        let mut stored = 0.0;
        let mut rounding = 0.0;
        let mut local_ok = true;
        for i in 0..t.len() {
            let (heat, round) = match old {
                Some(old) => (
                    self.heat(i, old[i], t[i])?,
                    self.capacity(i, old[i])?.max(self.capacity(i, t[i])?)
                        * (1.0 + old[i].abs() + t[i].abs()),
                ),
                None => (0.0, 0.0),
            };
            let storage_rate = heat / seconds;
            let residual = storage_rate - (flux[i] - flux[i + 1]);
            let tol = 256.0 * f64::EPSILON * round / seconds
                + 1e-11 * (1.0 + storage_rate.abs() + flux[i].abs() + flux[i + 1].abs());
            if [heat, round, storage_rate, residual, tol]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err(invalid("nonlinear thermal residual overflow"));
            }
            local_ok &= residual.abs() <= tol;
            stored += heat;
            rounding += round;
        }
        let inside = flux[0];
        let outside = flux[t.len()];
        let net = if old.is_some() {
            seconds * (inside - outside)
        } else {
            0.0
        };
        let error = stored - net;
        let tolerance = 512.0 * f64::EPSILON * rounding + 1e-9 * (1.0 + stored.abs() + net.abs());
        if [inside, outside, stored, net, error, tolerance]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(invalid("nonlinear thermal energy balance overflow"));
        }
        Ok((local_ok && error.abs() <= tolerance).then(|| State {
            temperature_c: t.to_vec(),
            inside_flux_w_m2: inside,
            outside_flux_w_m2: outside,
            stored_change_j_m2: stored,
            balance_error_j_m2: error,
        }))
    }

    pub(super) fn solve(
        &self,
        old: Option<&[f64]>,
        seconds: f64,
        b: Boundary,
    ) -> Result<State, Refusal> {
        positive("thermal time step", seconds)?;
        self.check(b.inside_c)?;
        self.check(b.outside_c)?;
        nonnegative("inside film resistance", b.inside_resistance)?;
        nonnegative("outside film resistance", b.outside_resistance)?;
        let n = self.laws.len() * self.per_layer;
        if old.is_some_and(|v| v.len() != n) {
            return Err(invalid("thermal state has the wrong cell count"));
        }
        let mut low = b.inside_c.min(b.outside_c);
        let mut high = b.inside_c.max(b.outside_c);
        if let Some(old) = old {
            for &t in old {
                self.check(t)?;
                low = low.min(t);
                high = high.max(t);
            }
        }
        let mut t = old.map_or_else(
            || vec![b.inside_c / 2.0 + b.outside_c / 2.0; n],
            <[f64]>::to_vec,
        );
        for iteration in 0..=ITERATIONS {
            let g = self.conductances(&t, b)?;
            if let Some(state) = self.accepted(&t, old, seconds, &g, b)? {
                return Ok(state);
            }
            if iteration == ITERATIONS {
                break;
            }
            let candidate = match old {
                None => {
                    let total = positive("stack resistance", g.iter().map(|v| 1.0 / v).sum())?;
                    let mut traversed = 0.0;
                    g.iter()
                        .take(n)
                        .map(|v| {
                            traversed += 1.0 / v;
                            let fraction = traversed / total;
                            (1.0 - fraction) * b.inside_c + fraction * b.outside_c
                        })
                        .collect::<Vec<_>>()
                }
                Some(old) => {
                    let mut diag = Vec::with_capacity(n);
                    for i in 0..n {
                        let change = t[i] - old[i];
                        // Secant Cp makes the fixed point use integrated enthalpy exactly. When the
                        // increment vanishes, use the positive tangent without an unstable division.
                        let c = if change == 0.0 {
                            self.capacity(i, t[i])?
                        } else {
                            self.heat(i, old[i], t[i])? / change
                        };
                        positive("secant heat capacity", c)?;
                        diag.push(positive("thermal diagonal", c / seconds + g[i] + g[i + 1])?);
                    }
                    let flux = Self::fluxes(old, &g, b);
                    let mut rhs: Vec<_> = flux.windows(2).map(|f| f[0] - f[1]).collect();
                    // Increment form preserves precision near a steady state. Positive secants and
                    // conductances give an SPD tridiagonal system; fixed-order O(n) elimination.
                    for i in 1..n {
                        let ratio = g[i] / diag[i - 1];
                        diag[i] = positive("thermal pivot", diag[i] - g[i] * ratio)?;
                        rhs[i] += ratio * rhs[i - 1];
                    }
                    let mut delta = vec![0.0; n];
                    for i in (0..n).rev() {
                        let right = if i + 1 == n {
                            0.0
                        } else {
                            g[i + 1] * delta[i + 1]
                        };
                        delta[i] = (rhs[i] + right) / diag[i];
                    }
                    old.iter().zip(delta).map(|(t, d)| t + d).collect()
                }
            };
            for (t, next) in t.iter_mut().zip(candidate) {
                temperature(next)?;
                let tol = 128.0 * f64::EPSILON * (1.0 + low.abs() + high.abs());
                if next < low - tol || next > high + tol {
                    return Err(invalid("nonlinear thermal maximum principle failed"));
                }
                // Only remove floating-point overshoot of the proven maximum-principle bounds.
                // An authored state/boundary is never clamped into the characterization domain.
                *t = *t / 2.0 + next.clamp(low, high) / 2.0;
                self.check(*t)?;
            }
        }
        Err(Refusal::new(
            ftx_core::RefusalCode::NotConverged,
            "nonlinear thermal solve did not converge within 256 damped iterations; reduce the time step or review the characterization",
        ))
    }
}

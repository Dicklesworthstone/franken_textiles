//! Material-coordinate transfer between geometric coverage partitions, with no elapsed heat step.
//!
//! Piecewise-constant donor enthalpy is integrated over polygon AND thickness-cell intersections.
//! Average temperature is not conservative when Cp varies with temperature. This first-order remap
//! mixes unresolved gradients; conservation and boundedness are not an accuracy/garment claim.
//! The same spatial material domain must remain covered: newly entering/leaving cloth needs a
//! separate exposed-cloth state model and is refused, never reset to an invented temperature.

use ftx_core::{
    Refusal, Vec2, Vec3,
    exact::{Sign, orient2d},
    receipt::sha256_hex,
};
use ftx_pattern::material_thermal::ThermalLaw;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Boundary, Coverage, CoverageState, CoverageStep, Surface, invalid};
use crate::thermal::surface::projection::{self, Projection, Request};
use projection::clip::{Work, area, bounds_overlap, intersection};

/// One derived geometric partition and its frozen poses. No mutable projection enters the remapper.
#[derive(Clone, Debug)]
pub struct Frame {
    coverage: Coverage,
    projection: Projection,
    request: Request,
    cloth: Vec<Vec3>,
    body: Vec<Vec3>,
    // Temperature cell order in a coverage column is body-outward. This flag connects it to the
    // persistent oriented cloth triangle, so approaching the other side reverses the cell index.
    reverse: Vec<Vec<bool>>,
    sha256: String,
}

/// Temperatures bound to both a coverage assembly AND the geometry that located its subpolygons.
#[derive(Clone, Debug)]
pub struct State {
    frame_sha256: String,
    inner: CoverageState,
}
impl State {
    /// Region, then inside-to-outside thermal-cell order in the owning frame.
    #[must_use]
    pub fn temperature_c(&self) -> &[Vec<f64>] {
        self.inner.temperature_c()
    }
}

/// A physical heat step, separate from an instantaneous repartition/remap.
#[derive(Clone, Debug)]
pub struct Step {
    frame_sha256: String,
    heat: CoverageStep,
}
impl Step {
    /// Existing coverage fields, including boundary heat and reference-mass storage change.
    #[must_use]
    pub fn heat(&self) -> &CoverageStep {
        &self.heat
    }
    /// Retain the temperature state for the next physical step or geometric remap.
    #[must_use]
    pub fn into_state(self) -> State {
        State {
            frame_sha256: self.frame_sha256,
            inner: self.heat.state,
        }
    }
}

/// Numerical accounting for an instantaneous remap; this is not heat supplied by the environment.
#[derive(Clone, Debug, Serialize)]
pub struct Balance {
    /// Source frame identity.
    pub from_sha256: String,
    /// Destination frame identity.
    pub to_sha256: String,
    /// Conserved material domain, in reference m², counted once per physical cloth triangle.
    pub reference_area_m2: f64,
    /// Largest summed donor/destination allocation residual, in reference m².
    pub allocation_residual_m2: f64,
    /// Summed absolute reference-area changes when convexifying roundoff-scale inward corners.
    /// Original projection receipts are retained unchanged; genuine nonconvexity is refused.
    pub polygon_roundoff_m2: f64,
    /// One enthalpy zero per source triangle (minimum donor temperature), or None when unassigned.
    pub reference_temperature_c: Vec<Option<f64>>,
    /// Sensible heat above those explicitly retained zeroes, before the remap, J.
    pub before_j: f64,
    /// Sensible heat above the SAME zeroes, after the remap, J.
    pub after_j: f64,
    /// Signed after-minus-before; never added to physical boundary heat.
    pub residual_j: f64,
    /// Sum of absolute per-cloth-triangle energy residuals; opposite errors cannot cancel.
    pub absolute_residual_j: f64,
    /// Bounded polygon, thickness-cell and thermal-law evaluation work.
    pub operations: usize,
}

/// New mapped state and its conservative-transfer accounting. A failure returns neither.
#[derive(Clone, Debug)]
pub struct Transfer {
    /// State belonging to the destination frame, ready for [`Frame::advance`].
    pub state: State,
    /// Independent remap energy and material-area balance.
    pub balance: Balance,
}

impl Frame {
    /// Derive a coverage partition from actual geometry using the existing projection closure.
    /// Material topology/reference areas remain immutable; body triangulation may change per frame.
    ///
    /// # Errors
    /// Every existing projection/coverage refusal, including ambiguity and geometry/work budgets.
    pub fn new(
        source: Surface,
        cloth: Vec<Vec3>,
        body: Vec<Vec3>,
        body_triangles: &[[usize; 3]],
        request: Request,
    ) -> Result<Self, Refusal> {
        let projection = projection::project(&source, &cloth, &body, body_triangles, &request)?;
        let mut reverse = Vec::new();
        for region in &projection.layout.regions {
            let [a, b, c] = body_triangles[region.body_triangle].map(|i| body[i]);
            let normal = (b - a).cross(c - a);
            let normal = normal / normal.norm();
            reverse.push(
                region
                    .layers
                    .iter()
                    .map(|p| {
                        let [a, b, c] = source.patches[p.cloth_triangle].vertices.map(|i| cloth[i]);
                        let cloth_normal = (b - a).cross(c - a);
                        let cosine = (cloth_normal / cloth_normal.norm()).dot(normal);
                        if !cosine.is_finite() || cosine == 0.0 {
                            return Err(invalid("unresolved material-normal orientation in remap"));
                        }
                        Ok(cosine < 0.0)
                    })
                    .collect::<Result<Vec<_>, Refusal>>()?,
            );
        }
        let coverage = Coverage::new(source, projection.layout.clone())?;
        let sha256 = sha256_hex(
            &serde_json::to_vec(&(
                "ftx-thermal-motion-frame/1",
                &coverage.sha256,
                &projection.input_sha256,
            ))
            .map_err(|e| invalid(e.to_string()))?,
        );
        Ok(Self {
            coverage,
            projection,
            request,
            cloth,
            body,
            reverse,
            sha256,
        })
    }

    /// Immutable derived geometry, allocations, numerical resolution and input identity.
    #[must_use]
    pub fn projection(&self) -> &Projection {
        &self.projection
    }

    /// Frame identity, including source observations, geometry and request.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Full frozen input receipt. Source geometry validity remains the projection's caller contract.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        json!({"schema":"ftx-thermal-motion-frame/1", "sha256":self.sha256,
            "source":self.coverage.source.snapshot(), "cloth_positions_m":self.cloth,
            "body_positions_m":self.body,"request":self.request,"projection":self.projection,
            "remap":"first-order material-polygon and thickness-cell enthalpy intersections",
            "domain":"same covered spatial cloth portions; no cloth remeshing or material substitution"})
    }

    /// Restore geometry, material observations and the derived spatial partition without a catalog
    /// lookup. Recompute correspondence rather than trusting saved polygons or a verification flag.
    ///
    /// # Errors
    /// Changed/unsupported snapshot content or any source, projection or coverage refusal.
    pub fn from_snapshot(value: &Value) -> Result<Self, Refusal> {
        if value["schema"] != "ftx-thermal-motion-frame/1" {
            return Err(invalid("unknown thermal motion frame schema"));
        }
        for (array, limit) in [
            (&value["cloth_positions_m"], 196_608),
            (&value["body_positions_m"], 196_608),
            (&value["projection"]["layout"]["body_triangles"], 65_536),
        ] {
            if array
                .as_array()
                .is_none_or(|a| a.is_empty() || a.len() > limit)
            {
                return Err(invalid("thermal motion snapshot exceeds geometry budget"));
            }
        }
        let source = Surface::from_snapshot(&value["source"])?;
        let decode = |e| invalid(format!("thermal motion frame: {e}"));
        let cloth = serde_json::from_value(value["cloth_positions_m"].clone()).map_err(decode)?;
        let body = serde_json::from_value(value["body_positions_m"].clone()).map_err(decode)?;
        let triangles: Vec<[usize; 3]> =
            serde_json::from_value(value["projection"]["layout"]["body_triangles"].clone())
                .map_err(decode)?;
        let request = serde_json::from_value(value["request"].clone()).map_err(decode)?;
        let frame = Self::new(source, cloth, body, &triangles, request)?;
        if frame.snapshot() != *value {
            return Err(invalid(
                "thermal motion frame differs from recomputed geometry/material inputs",
            ));
        }
        Ok(frame)
    }

    /// Serialize a resumable material-temperature state with its complete immutable frame. The
    /// digest detects content changes; it is not authentication or proof of a simulation's history.
    /// No time elapses and no temperatures are recomputed, mixed or initialized by saving.
    ///
    /// # Errors
    /// State from another frame, invalid temperature cells, or serialization failure.
    pub fn checkpoint(&self, state: &State) -> Result<Value, Refusal> {
        self.check(state)?;
        let mut value = json!({"schema":"ftx-thermal-motion-state/1",
            "frame":self.snapshot(),"temperature_c":state.temperature_c()});
        let hash = sha256_hex(&serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))?);
        value["sha256"] = json!(hash);
        Ok(value)
    }

    /// Restore a saved state and rebuild its exact frame, including source curves and geometric
    /// correspondence. A material edit cannot reuse these temperatures without an explicit model.
    ///
    /// # Errors
    /// Unsupported fields/schema, tampered digest/geometry, invalid cell shape or temperature domain.
    pub fn from_checkpoint(value: &Value) -> Result<(Self, State), Refusal> {
        if value["schema"] != "ftx-thermal-motion-state/1" {
            return Err(invalid("unknown thermal motion state schema"));
        }
        let frame = Self::from_snapshot(&value["frame"])?;
        let rows = value["temperature_c"]
            .as_array()
            .ok_or_else(|| invalid("thermal motion state needs temperature rows"))?;
        if rows.len() != frame.coverage.models.len()
            || rows
                .iter()
                .zip(&frame.coverage.models)
                .any(|(row, model)| row.as_array().is_none_or(|r| r.len() != model.cells()))
        {
            return Err(invalid(
                "thermal motion state has incorrect cell dimensions",
            ));
        }
        let state = State {
            frame_sha256: frame.sha256.clone(),
            inner: CoverageState {
                sha256: frame.coverage.sha256.clone(),
                temperatures: serde_json::from_value(value["temperature_c"].clone())
                    .map_err(|e| invalid(format!("thermal motion temperatures: {e}")))?,
            },
        };
        if frame.checkpoint(&state)? != *value {
            return Err(invalid("thermal motion state digest or content mismatch"));
        }
        Ok((frame, state))
    }

    /// Total temperature cells for budgeting, excluding massless bare-body paths.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.coverage.cells()
    }

    /// Required boundary-array length.
    #[must_use]
    pub fn body_triangle_count(&self) -> usize {
        self.coverage.body_triangle_count()
    }

    /// Start a new simulation at an explicitly supplied temperature; never called implicitly on remap.
    ///
    /// # Errors
    /// Invalid or uncharacterized temperature.
    pub fn initial(&self, t: f64) -> Result<State, Refusal> {
        Ok(State {
            frame_sha256: self.sha256.clone(),
            inner: self.coverage.initial(t)?,
        })
    }

    fn check(&self, state: &State) -> Result<(), Refusal> {
        if state.frame_sha256 != self.sha256 || state.inner.sha256 != self.coverage.sha256 {
            return Err(invalid(
                "thermal state belongs to another mapped geometry; remap explicitly",
            ));
        }
        if state.inner.temperatures.len() != self.coverage.models.len() {
            return Err(invalid("thermal motion state has incorrect region count"));
        }
        for (row, model) in state.inner.temperatures.iter().zip(&self.coverage.models) {
            if row.len() != model.cells() {
                return Err(invalid("thermal motion state has incorrect cell count"));
            }
            for &t in row {
                model.check_temperature(t)?;
            }
        }
        Ok(())
    }

    /// Solve steady temperatures on this geometry, for initialization/verification when appropriate.
    ///
    /// # Errors
    /// Invalid boundaries or failed existing thermal solve.
    pub fn steady(&self, boundaries: &[Boundary]) -> Result<Step, Refusal> {
        Ok(Step {
            frame_sha256: self.sha256.clone(),
            heat: self.coverage.steady(&self.cloth, &self.body, boundaries)?,
        })
    }

    /// Advance on the frozen geometry for a declared duration. Remapping itself advances no time.
    ///
    /// # Errors
    /// State from a different geometric frame, invalid time/boundaries or failed thermal solve.
    pub fn advance(
        &self,
        state: &State,
        seconds: f64,
        boundaries: &[Boundary],
    ) -> Result<Step, Refusal> {
        self.check(state)?;
        Ok(Step {
            frame_sha256: self.sha256.clone(),
            heat: self.coverage.advance(
                &state.inner,
                &self.cloth,
                &self.body,
                seconds,
                boundaries,
            )?,
        })
    }

    /// Conservatively transfer old temperatures into this frame using material-coordinate overlap.
    /// Body mesh/pose, region ordering and through-thickness discretization may change. No source
    /// material, cloth topology, physical-layer identity or covered material domain may change.
    ///
    /// # Errors
    /// Changed source/physical identity, entering/leaving cloth, unmatched spatial partitions,
    /// work-budget exhaustion, out-of-domain temperatures, or failed local/global energy balance.
    pub fn remap(&self, old: &Self, state: &State) -> Result<Transfer, Refusal> {
        old.check(state)?;
        if self.coverage.source.sha256 != old.coverage.source.sha256
            || self.request.cloth_layers != old.request.cloth_layers
        {
            return Err(invalid(
                "remap requires unchanged source material, cloth topology and layer identities",
            ));
        }
        let source = &self.coverage.source;
        let laws = source
            .columns
            .iter()
            .map(|column| {
                let m = &column.layers[0];
                ThermalLaw::from_material(m.material(), m.allows_estimates())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let costs: Vec<_> = source
            .columns
            .iter()
            .map(|column| {
                column.layers[0]
                    .material()
                    .thermal_response
                    .as_ref()
                    .and_then(|r| r.specific_heat.as_ref())
                    .map_or(1, |s| s.points.len().max(1))
            })
            .collect();
        let mut donors = vec![Vec::new(); source.patches.len()];
        let mut zeros = vec![None::<f64>; source.patches.len()];
        let n = old.coverage.layout.cells_per_layer;
        let m = self.coverage.layout.cells_per_layer;
        for (r, region) in old.coverage.layout.regions.iter().enumerate() {
            for (l, p) in region.layers.iter().enumerate() {
                donors[p.cloth_triangle].push((r, l));
                for &t in &state.inner.temperatures[r][l * n..(l + 1) * n] {
                    laws[source.patches[p.cloth_triangle].column].check_temperature(t)?;
                    zeros[p.cloth_triangle] = Some(zeros[p.cloth_triangle].map_or(t, |v| v.min(t)));
                }
            }
        }
        let mut work = Work::default();
        let (old_polygons, old_roundoff) = old.material_polygons(&mut work)?;
        let (polygons, new_roundoff) = self.material_polygons(&mut work)?;
        let mut used: Vec<Vec<f64>> = old
            .coverage
            .layout
            .regions
            .iter()
            .map(|r| vec![0.; r.layers.len()])
            .collect();
        let mut output = Vec::new();
        let mut destination_error = 0.;
        for (r, region) in self.coverage.layout.regions.iter().enumerate() {
            let mut values = Vec::new();
            for (l, p) in region.layers.iter().enumerate() {
                let polygon = &polygons[r][l];
                let mut intersections = Vec::new();
                let mut received = 0.;
                for &(s, k) in &donors[p.cloth_triangle] {
                    work.add(1)?;
                    let other = &old_polygons[s][k];
                    if !bounds_overlap(polygon, other) {
                        continue;
                    }
                    let overlap = 2. * area(&intersection(polygon, other, &mut work)?).max(0.);
                    if overlap == 0. {
                        continue;
                    }
                    let f = old.coverage.layout.regions[s].layers[k].fraction * overlap
                        / (2. * area(other));
                    if !f.is_finite() || f <= 0. {
                        return Err(invalid("remap intersection fraction overflow"));
                    }
                    used[s][k] += f;
                    received += f;
                    intersections.push((s, k, f));
                }
                check_fraction(received, p.fraction)?;
                destination_error += (received - p.fraction).abs()
                    * source.patches[p.cloth_triangle].reference_area_m2;
                let law = &laws[source.patches[p.cloth_triangle].column];
                for j in 0..m {
                    let mut inputs = Vec::new();
                    let [a, b] = depth(j, m, self.reverse[r][l]);
                    for &(s, k, f) in &intersections {
                        // Only potentially intersecting donor depth cells are visited. This avoids
                        // O(n*m) work when changing a fine through-thickness discretization.
                        let start = (a * n as f64).floor() as usize;
                        let end = ((b * n as f64).ceil() as usize).min(n);
                        for canonical in start..end {
                            work.add(1)?;
                            let overlap = b.min((canonical + 1) as f64 / n as f64)
                                - a.max(canonical as f64 / n as f64);
                            if overlap <= 0. {
                                continue;
                            }
                            let i = if old.reverse[s][k] {
                                n - 1 - canonical
                            } else {
                                canonical
                            };
                            inputs.push((f * overlap, state.inner.temperatures[s][k * n + i]));
                        }
                    }
                    values.push(mix(
                        law,
                        &inputs,
                        costs[source.patches[p.cloth_triangle].column],
                        &mut work,
                    )?);
                }
            }
            output.push(values);
        }
        let mut donor_error = 0.;
        for (r, region) in old.coverage.layout.regions.iter().enumerate() {
            for (l, p) in region.layers.iter().enumerate() {
                check_fraction(used[r][l], p.fraction)?;
                donor_error += (used[r][l] - p.fraction).abs()
                    * source.patches[p.cloth_triangle].reference_area_m2;
            }
        }
        let next = State {
            frame_sha256: self.sha256.clone(),
            inner: CoverageState {
                sha256: self.coverage.sha256.clone(),
                temperatures: output,
            },
        };
        let before = old.energy(&state.inner, &laws, &zeros, &costs, &mut work)?;
        let after = self.energy(&next.inner, &laws, &zeros, &costs, &mut work)?;
        let mut absolute_residual = 0.;
        for (&a, &b) in before.iter().zip(&after) {
            let error = (b - a).abs();
            if !error.is_finite() || error > 2e-9 * (1. + a.abs() + b.abs()) {
                return Err(invalid("material-local remap enthalpy balance failed"));
            }
            absolute_residual += error;
        }
        let before_j = before.iter().sum::<f64>();
        let after_j = after.iter().sum::<f64>();
        let reference_area_m2 = old
            .coverage
            .allocated
            .iter()
            .zip(&source.patches)
            .map(|(f, p)| f * p.reference_area_m2)
            .sum::<f64>();
        let balance = Balance {
            from_sha256: old.sha256.clone(),
            to_sha256: self.sha256.clone(),
            reference_area_m2,
            allocation_residual_m2: donor_error.max(destination_error),
            polygon_roundoff_m2: old_roundoff + new_roundoff,
            reference_temperature_c: zeros,
            before_j,
            after_j,
            residual_j: after_j - before_j,
            absolute_residual_j: absolute_residual,
            operations: work.0,
        };
        if [
            reference_area_m2,
            donor_error,
            destination_error,
            before_j,
            after_j,
            balance.residual_j,
            balance.polygon_roundoff_m2,
            absolute_residual,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err(invalid("remap balance accumulation overflow"));
        }
        Ok(Transfer {
            state: next,
            balance,
        })
    }

    fn material_polygons(&self, work: &mut Work) -> Result<(MaterialPolygons, f64), Refusal> {
        let mut roundoff = 0.;
        let polygons = self
            .projection
            .footprints
            .iter()
            .map(|fp| {
                fp.hits
                    .iter()
                    .map(|hit| {
                        let (polygon, delta) = convex_roundoff(&hit.cloth_polygon, work)?;
                        roundoff += 2.
                            * delta
                            * self.coverage.source.patches[hit.cloth_triangle].reference_area_m2;
                        Ok(polygon)
                    })
                    .collect::<Result<Vec<_>, Refusal>>()
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok((polygons, roundoff))
    }

    fn energy(
        &self,
        state: &CoverageState,
        laws: &[ThermalLaw],
        zeros: &[Option<f64>],
        costs: &[usize],
        work: &mut Work,
    ) -> Result<Vec<f64>, Refusal> {
        let source = &self.coverage.source;
        let n = self.coverage.layout.cells_per_layer;
        let mut energy = vec![0.; source.patches.len()];
        for (r, region) in self.coverage.layout.regions.iter().enumerate() {
            for (l, p) in region.layers.iter().enumerate() {
                let patch = &source.patches[p.cloth_triangle];
                let zero = zeros[p.cloth_triangle]
                    .ok_or_else(|| invalid("newly covered cloth has no thermal history"))?;
                let scale = patch.reference_area_m2 * p.fraction / n as f64;
                for &t in &state.temperatures[r][l * n..(l + 1) * n] {
                    work.add(costs[patch.column])?;
                    energy[p.cloth_triangle] +=
                        scale * laws[patch.column].sensible_change(zero, t)?;
                }
            }
        }
        Ok(energy)
    }
}

type MaterialPolygons = Vec<Vec<Vec<Vec2>>>;

// Projection polygons are convex by construction, but inverse barycentric arithmetic can leave
// almost-duplicate corners with an inward turn. Treating that tiny edge as an infinite clip line
// can erase a finite donor wedge. An exact-sign hull removes only numerically insignificant dents;
// it does not round coordinates, weaken the material-domain test or repair genuine nonconvexity.
fn convex_roundoff(p: &[Vec2], work: &mut Work) -> Result<(Vec<Vec2>, f64), Refusal> {
    if !(3..=256).contains(&p.len())
        || p.iter()
            .any(|v| !v.is_finite() || v.x.abs() > 2. || v.y.abs() > 2.)
    {
        return Err(invalid(
            "invalid material-coordinate polygon for heat remapping",
        ));
    }
    work.add(p.len() * 10)?;
    let mut sorted = p.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    let mut hull: Vec<Vec2> = Vec::new();
    let mut append = |points: &mut dyn Iterator<Item = &Vec2>| {
        let start = hull.len();
        for &c in points {
            while hull.len() >= start + 2 {
                let a = hull[hull.len() - 2];
                let b = hull[hull.len() - 1];
                if orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y]) == Sign::Positive {
                    break;
                }
                hull.pop();
            }
            hull.push(c);
        }
        hull.pop();
    };
    append(&mut sorted.iter());
    append(&mut sorted.iter().rev());
    let original = area(p);
    let corrected = area(&hull);
    let delta = (corrected - original).abs();
    if !original.is_finite()
        || !corrected.is_finite()
        || original <= 0.
        || corrected <= 0.
        || delta > 128. * f64::EPSILON * (1. + original.abs())
    {
        return Err(invalid(
            "material polygon is nonconvex beyond floating-point roundoff",
        ));
    }
    Ok((hull, delta))
}

fn check_fraction(received: f64, expected: f64) -> Result<(), Refusal> {
    // Explicit numerical allowance for the projection's bounded allocation-roundoff corrections.
    // Never normalize away a genuinely entering/leaving portion or a spatial gap.
    if !received.is_finite() || (received - expected).abs() > 5e-10 * expected + 64. * f64::EPSILON
    {
        return Err(invalid(format!(
            "covered material domain changed (received {received:.17e}, expected {expected:.17e}); entering/leaving cloth has no modeled thermal history",
        )));
    }
    if received <= 0. {
        return Err(invalid("destination has no thermal donor"));
    }
    Ok(())
}

fn depth(i: usize, n: usize, reverse: bool) -> [f64; 2] {
    let i = if reverse { n - 1 - i } else { i };
    [i as f64 / n as f64, (i + 1) as f64 / n as f64]
}

fn mix(
    law: &ThermalLaw,
    inputs: &[(f64, f64)],
    knots: usize,
    work: &mut Work,
) -> Result<f64, Refusal> {
    if inputs.is_empty() {
        return Err(invalid("empty enthalpy transfer stencil"));
    }
    work.add(inputs.len())?;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut weight = 0.;
    for &(w, t) in inputs {
        if !w.is_finite() || w <= 0. {
            return Err(invalid("invalid enthalpy transfer weight"));
        }
        law.check_temperature(t)?;
        lo = lo.min(t);
        hi = hi.max(t);
        weight += w;
    }
    if !weight.is_finite() || weight <= 0. {
        return Err(invalid("enthalpy transfer weight overflow"));
    }
    if lo == hi {
        return Ok(lo);
    }
    // Curve integration visits its knots. Bound this work too, not just the number of stencil
    // entries, so a long characterization table cannot evade the remap budget.
    work.add(
        knots
            .checked_mul(inputs.len() + 132)
            .ok_or_else(|| invalid("remap work overflow"))?,
    )?;
    let zero = lo;
    let target = inputs
        .iter()
        .map(|&(w, t)| Ok((w / weight) * law.sensible_change(zero, t)?))
        .collect::<Result<Vec<_>, Refusal>>()?
        .iter()
        .sum::<f64>();
    if !target.is_finite() {
        return Err(invalid("enthalpy average overflow"));
    }
    // Positive Cp gives a strictly monotone inverse. Bracket by donor temperatures, not by an
    // extrapolated material domain; 128 bounded bisections preserve the donor maximum principle.
    for _ in 0..128 {
        let t = lo / 2. + hi / 2.;
        if t == lo || t == hi {
            break;
        }
        if law.sensible_change(zero, t)? < target {
            lo = t;
        } else {
            hi = t;
        }
    }
    let error_lo = (law.sensible_change(zero, lo)? - target).abs();
    let error_hi = (law.sensible_change(zero, hi)? - target).abs();
    let t = if error_lo <= error_hi { lo } else { hi };
    let tolerance = 256. * f64::EPSILON * law.capacity_at(t)? * (1. + zero.abs() + t.abs())
        + 1e-12 * (1. + target.abs());
    if !tolerance.is_finite() || error_lo.min(error_hi) > tolerance {
        return Err(invalid("enthalpy inverse did not converge"));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microscopic_inward_edge_cannot_delete_a_finite_remap_donor() {
        // Retained from the sewn-band CLI regression: inverse barycentric rounding puts two
        // nearly coincident corners in an inward turn. Treating that microscopic edge as an
        // infinite clipping plane removes a donor about 5.14e-8 of the material triangle.
        let target = [
            Vec2::new(0.141_542_294_724_737_98, 0.000_965_775_692_924_402_1),
            Vec2::new(4.273_511_510_613_281e-14, 5.463_578_223_132_216e-16),
            Vec2::new(0.141_381_797_130_168_27, -5.922_597_980_646_374e-16),
        ];
        let donor = [
            Vec2::new(0.002_742_812_421_032_216, 2.776_217_803_427_988e-16),
            Vec2::new(0.002_744_394_779_058_933_5, 1.872_563_797_723_176_4e-5),
            Vec2::new(1.333_450_065_677_077e-15, 2.659_877_951_251_099e-16),
            Vec2::new(1.365_249_613_424_237_6e-15, 2.902_447_992_725_097_4e-16),
        ];
        let mut work = Work::default();
        let broken = intersection(&target, &donor, &mut work).unwrap();
        assert!(area(&broken).abs() < 1e-12);
        let (target, _) = convex_roundoff(&target, &mut work).unwrap();
        let (fixed, correction) = convex_roundoff(&donor, &mut work).unwrap();
        let overlap = intersection(&target, &fixed, &mut work).unwrap();
        assert!(correction < 1e-18);
        // Independent rational-coordinate half-plane intersection, starting from these exact
        // f64 inputs, gives this doubled area. The tiny boundary truncation is real, not zero.
        assert!((2. * area(&overlap) - 5.136_091_243_263_29e-8).abs() < 1e-20);
        assert!((area(&fixed) - area(&donor)).abs() < 1e-18);
    }

    #[test]
    fn convex_roundoff_preserves_convex_domain_despite_duplicates_and_cycle_origin() {
        let original = [
            Vec2::new(0., 0.),
            Vec2::new(0.5, 0.),
            Vec2::new(0.5, 0.),
            Vec2::new(1., 0.),
            Vec2::new(0., 1.),
            Vec2::new(0., 0.),
        ];
        for start in 0..original.len() {
            let mut p = original.to_vec();
            p.rotate_left(start);
            let (hull, correction) = convex_roundoff(&p, &mut Work::default()).unwrap();
            assert_eq!(hull.len(), 3);
            assert_eq!(area(&hull), 0.5);
            assert_eq!(correction, 0.);
        }
    }

    #[test]
    fn roundoff_admission_is_not_a_repair_for_nonconvex_or_invalid_material_geometry() {
        let concave = [
            Vec2::new(0., 0.),
            Vec2::new(1., 0.),
            Vec2::new(0.25, 0.25),
            Vec2::new(0., 1.),
        ];
        assert!(convex_roundoff(&concave, &mut Work::default()).is_err());
        let triangle = [Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(0., 1.)];
        let mut clockwise = triangle;
        clockwise.reverse();
        assert!(convex_roundoff(&clockwise, &mut Work::default()).is_err());
        for v in [f64::NAN, f64::INFINITY, 3.] {
            let mut p = triangle;
            p[0].x = v;
            assert!(convex_roundoff(&p, &mut Work::default()).is_err());
        }
        assert!(convex_roundoff(&[Vec2::ZERO; 3], &mut Work::default()).is_err());
    }
}

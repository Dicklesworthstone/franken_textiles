//! One-way thermal evolution on accepted mechanical samples, using the existing source-locked
//! thermal solvers. Physical time and reference mass are retained; temperature never feeds back
//! into mechanics. A refused history is not a partially successful or zero-heat result.

use ftx_core::{Refusal, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{budget, hash, invalid, motion::Keyframe};
use crate::{
    run::DrapeRun,
    thermal::{
        Boundary,
        steady::request::Film,
        surface::{PatchHeat, Surface, SurfaceState},
    },
};

/// Explicit transient policy for independent cloth sheets during prescribed motion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRequest {
    /// Must be `ftx-motion-surface-heat/1`.
    pub schema: String,
    /// Uniform material temperature at the end of the initial mechanical drape (degrees Celsius).
    /// This is an authored initial condition, not a measured or equilibrated garment temperature.
    pub initial_temperature_c: f64,
    /// Through-thickness cells per retained material sheet, in 1..=64.
    pub cells_per_layer: usize,
    /// One spatially uniform reservoir/film pair per mechanical keyframe interval. Values apply
    /// after the preceding keyframe up to this keyframe; there is no implicit steady-film fallback.
    pub boundaries: Vec<Film>,
    /// Maximum thermal substep in seconds, in 1e-9..=1. Geometry stays at the accepted mechanical
    /// sample's right endpoint during these thermal substeps; no unseen cloth motion is invented.
    pub max_step_s: f64,
    /// Maximum temperature-cell advances for this whole history, in 1..=10000000. Also caps initial
    /// state size. Each attempted history reserves this amount against a 20000000 session budget.
    pub max_cell_steps: usize,
}

// Conservative reservations keep multi-alternative studies bounded, including failed histories.
// Separate units from the mechanical session's vertex/frame allocation budget.
#[derive(Default)]
pub(super) struct Budget {
    reserved_cell_steps: usize,
}

impl Budget {
    fn reserve(&mut self, count: usize) -> Result<(), Refusal> {
        self.reserved_cell_steps = self.reserved_cell_steps.saturating_add(count);
        if self.reserved_cell_steps > 20_000_000 {
            return Err(budget(
                "motion heat exceeds 20000000 reserved session cell steps",
            ));
        }
        Ok(())
    }
}

fn boundary(film: Film) -> Result<Boundary, Refusal> {
    if [film.inside_c, film.outside_c]
        .iter()
        .any(|t| !t.is_finite() || *t < -273.15)
        || [
            film.inside_resistance_m2_k_w,
            film.outside_resistance_m2_k_w,
        ]
        .iter()
        .any(|r| !r.is_finite() || *r < 0.)
    {
        return Err(invalid(
            "motion heat needs finite physical temperatures and nonnegative films",
        ));
    }
    Ok(Boundary {
        inside_c: film.inside_c,
        outside_c: film.outside_c,
        inside_resistance: film.inside_resistance_m2_k_w,
        outside_resistance: film.outside_resistance_m2_k_w,
    })
}

impl SurfaceRequest {
    pub(super) fn start(
        &self,
        run: &DrapeRun,
        keyframes: &[Keyframe],
        work: &mut Budget,
    ) -> Result<SurfaceHistory, Refusal> {
        self.validate(keyframes.len())?;
        work.reserve(self.max_cell_steps)?;
        SurfaceHistory::new(self.clone(), run.thermal_surface(self.cells_per_layer)?)
    }

    fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        if self.schema != "ftx-motion-surface-heat/1"
            || !(1..=16).contains(&intervals)
            || self.boundaries.len() != intervals
            || !(1..=64).contains(&self.cells_per_layer)
            || !self.initial_temperature_c.is_finite()
            || self.initial_temperature_c < -273.15
            || !self.max_step_s.is_finite()
            || !(1e-9..=1.).contains(&self.max_step_s)
            || !(1..=10_000_000).contains(&self.max_cell_steps)
        {
            return Err(invalid(
                "invalid motion surface heat schema, interval count, initial state or work policy",
            ));
        }
        for &film in &self.boundaries {
            boundary(film)?;
        }
        Ok(())
    }
}

pub(super) struct SurfaceHistory {
    request: SurfaceRequest,
    model: Surface,
    state: SurfaceState,
    time_s: f64,
    cell_steps: usize,
    thermal_steps: usize,
    inside_j: f64,
    outside_j: f64,
    stored_j: f64,
    net_j: f64,
    absolute_error_j: f64,
    trace: Vec<Value>,
    final_field: Option<SurfaceField>,
}

struct SurfaceField {
    area_m2: f64,
    inside_heat_w: f64,
    outside_heat_w: f64,
    seconds: f64,
    stored_change_j: f64,
    patches: Vec<PatchHeat>,
}

impl SurfaceHistory {
    fn new(request: SurfaceRequest, model: Surface) -> Result<Self, Refusal> {
        if model.cells() > request.max_cell_steps {
            return Err(budget(
                "motion heat initial cells exceed the whole-history budget",
            ));
        }
        let state = model.initial(request.initial_temperature_c)?;
        Ok(Self {
            request,
            model,
            state,
            time_s: 0.,
            cell_steps: 0,
            thermal_steps: 0,
            inside_j: 0.,
            outside_j: 0.,
            stored_j: 0.,
            net_j: 0.,
            absolute_error_j: 0.,
            trace: Vec::new(),
            final_field: None,
        })
    }

    pub(super) fn advance(
        &mut self,
        positions: &[Vec3],
        time_s: f64,
        interval: usize,
        sample_index: usize,
    ) -> Result<(), Refusal> {
        let seconds = time_s - self.time_s;
        if !time_s.is_finite() || seconds <= 0. || self.trace.len() >= 512 {
            return Err(invalid(
                "motion heat needs increasing accepted sample times and at most 512 samples",
            ));
        }
        let film = *self
            .request
            .boundaries
            .get(interval)
            .ok_or_else(|| invalid("motion heat has no boundary for this interval"))?;
        let boundaries = vec![boundary(film)?; self.model.column_count()];
        let count = (seconds / self.request.max_step_s).ceil();
        let available = (self.request.max_cell_steps - self.cell_steps) / self.model.cells();
        if !count.is_finite() || count < 1. || count > available as f64 {
            return Err(budget(
                "motion heat cell-step budget exhausted; no partial history published",
            ));
        }
        let count = count as usize;
        let dt = seconds / count as f64;
        for _ in 0..count {
            let next = self
                .model
                .advance(&self.state, positions, dt, &boundaries)?;
            self.inside_j += dt * next.inside_heat_w;
            self.outside_j += dt * next.outside_heat_w;
            self.net_j += dt * (next.inside_heat_w - next.outside_heat_w);
            self.stored_j += next.stored_change_j;
            self.absolute_error_j += next.balance_error_j.abs();
            self.final_field = Some(SurfaceField {
                area_m2: next.area_m2,
                inside_heat_w: next.inside_heat_w,
                outside_heat_w: next.outside_heat_w,
                seconds: dt,
                stored_change_j: next.stored_change_j,
                patches: next.patches,
            });
            self.state = next.state;
        }
        if [
            self.inside_j,
            self.outside_j,
            self.net_j,
            self.stored_j,
            self.absolute_error_j,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err(invalid("motion heat energy accumulation overflow"));
        }
        self.cell_steps += count * self.model.cells();
        self.thermal_steps += count;
        self.time_s = time_s;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &t in self.state.temperature_c().iter().flatten() {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        let field = self
            .final_field
            .as_ref()
            .ok_or_else(|| invalid("missing heat step"))?;
        self.trace.push(
            json!({"sample_index":sample_index,"interval_index":interval,
            "time_s":time_s,"thermal_substeps":count,"positions_sha256":hash(&positions)?,
            "temperature_range_c":[lo,hi],"inside_heat_w":field.inside_heat_w,
            "outside_heat_w":field.outside_heat_w,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,
            "net_boundary_energy_j":self.net_j,"stored_change_j":self.stored_j,
            "balance_error_j":self.stored_j-self.net_j}),
        );
        Ok(())
    }

    pub(super) fn finish(
        self,
        path_hash: &str,
        initial_hash: &str,
        duration_s: f64,
    ) -> Result<Value, Refusal> {
        if self.time_s != duration_s || self.trace.is_empty() {
            return Err(invalid(
                "motion heat history does not cover the complete mechanical path",
            ));
        }
        let f = self
            .final_field
            .ok_or_else(|| invalid("missing final heat field"))?;
        let final_field = json!({"temperature_c":self.state.temperature_c(),
            "area_m2":f.area_m2,"inside_heat_w":f.inside_heat_w,"outside_heat_w":f.outside_heat_w,
            "last_step_seconds":f.seconds,"last_step_stored_change_j":f.stored_change_j,
            "patches":f.patches.iter().map(|p| json!({"area_m2":p.area_m2,
                "inside_flux_w_m2":p.inside_flux_w_m2,"outside_flux_w_m2":p.outside_flux_w_m2,
                "last_step_stored_change_j":p.stored_change_j})).collect::<Vec<_>>()});
        Ok(
            json!({"schema":"ftx-motion-surface-heat-result/1","request":self.request,
            "model":self.model.snapshot(),"path_sha256":path_hash,
            "initial_positions_sha256":initial_hash,"duration_s":duration_s,
            "initial_temperature_c":self.request.initial_temperature_c,
            "cell_steps":self.cell_steps,"thermal_steps":self.thermal_steps,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,
            "net_boundary_energy_j":self.net_j,"stored_change_j":self.stored_j,
            "balance_error_j":self.stored_j-self.net_j,
            "absolute_step_balance_error_j":self.absolute_error_j,
            "trace":self.trace,"final":final_field,
            "uncertainty_propagated":false,"physical_validation_claim":false,"comfort_claim":false,
            "scope":"One-way independent-sheet backward Euler on every accepted mechanical sample's current geometry. Storage follows reference mass. Boundary conditions are uniform in space and piecewise constant by keyframe interval; thermal substeps do not interpolate geometry. No inferred layer overlap, body heat loss, thermomechanical feedback, mechanical heating, moisture, comfort or empirical garment claim."}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::{DryThermalModel, surface::Patch};
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn policy() -> SurfaceRequest {
        serde_json::from_value(json!({"schema":"ftx-motion-surface-heat/1",
            "initial_temperature_c":10.,"cells_per_layer":1,"max_step_s":0.5,
            "max_cell_steps":100,"boundaries":[{"inside_c":30.,"outside_c":0.,
                "inside_resistance_m2_k_w":0.1,"outside_resistance_m2_k_w":0.2}]}))
        .unwrap()
    }
    fn model() -> Surface {
        let selection: Selection = serde_json::from_value(json!({"id":"test-heat","allow_estimates":true,
            "record":{"id":"test-heat","name":"Synthetic heat test","form":"woven",
                "sources":{"test":{"reference":"test:heat-history","accessed_on":"2026-10-17",
                    "note":"Synthetic known-answer data, not textile characterization"}},
                "properties":{
                    "areal_density":{"value":0.2,"unit":"kg/m2","evidence":"estimate","source":"test","method":"synthetic"},
                    "specific_heat":{"value":1000.,"unit":"J/(kg*K)","evidence":"estimate","source":"test","method":"synthetic"},
                    "thermal_resistance":{"value":0.4,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"synthetic"}
                }}})).unwrap();
        let m = Catalog::builtin().unwrap().resolve(&selection).unwrap();
        Surface::new(
            vec![DryThermalModel::new(vec![m], vec![], 1).unwrap()],
            3,
            vec![Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            }],
        )
        .unwrap()
    }

    #[test]
    fn moving_area_history_matches_independent_backward_euler_recurrence() {
        let mut history = SurfaceHistory::new(policy(), model()).unwrap();
        let mut temperature = 10.;
        for (i, area) in [0.5, 1., 0.125, 0.5].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            // Two half-second heat steps per accepted pose; capacity is ALWAYS 100 J/K,
            // irrespective of the current exchange area. R/2 plus the declared boundary films.
            for _ in 0..2 {
                temperature = (100. / 0.5 * temperature + area * 30. / 0.3)
                    / (100. / 0.5 + area * (1. / 0.3 + 1. / 0.4));
            }
            history.advance(&x, (i + 1) as f64, 0, i).unwrap();
            assert!((history.state.temperature_c()[0][0] - temperature).abs() < 1e-12);
        }
        let h = history.finish("path", "initial", 4.).unwrap();
        assert!(
            (h["stored_change_j"].as_f64().unwrap() - 100. * (temperature - 10.)).abs() < 1e-10
        );
        assert!(h["balance_error_j"].as_f64().unwrap().abs() < 1e-10);
        assert_eq!(h["cell_steps"], 8);
    }

    #[test]
    fn thermal_policy_and_aggregate_reservation_never_silently_clamp() {
        let good = policy();
        good.validate(1).unwrap();
        assert!(good.validate(2).is_err());
        for bad in [0., f64::NAN, f64::INFINITY, -1.] {
            let mut p = good.clone();
            p.max_step_s = bad;
            assert!(p.validate(1).is_err());
        }
        let mut p = good.clone();
        p.boundaries[0].outside_c = -300.;
        assert!(p.validate(1).is_err());
        p = good;
        p.max_cell_steps = 10_000_001;
        assert!(p.validate(1).is_err());
        let mut b = Budget::default();
        b.reserve(10_000_000).unwrap();
        b.reserve(10_000_000).unwrap();
        assert!(b.reserve(1).is_err());
        assert!(b.reserve(1).is_err());
    }
}

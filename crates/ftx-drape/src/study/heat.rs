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
        steady::{exterior::Exterior, request::Film},
        surface::{PatchHeat, Surface, SurfaceExteriorExchange, SurfaceState},
    },
};

pub mod coverage;

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
    /// Optional qualified replacement of each interval's outside film. Exactly one uniform
    /// environment per keyframe interval; each sheet supplies its own exposed-face emissivity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior_exchange: Option<Vec<Exterior>>,
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
    reserved_geometry_operations: usize,
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

    fn reserve_geometry(&mut self, count: usize) -> Result<(), Refusal> {
        self.reserved_geometry_operations = self.reserved_geometry_operations.saturating_add(count);
        if self.reserved_geometry_operations > 40_000_000 {
            return Err(budget(
                "motion heat exceeds 40000000 reserved session geometry operations",
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
            || self
                .exterior_exchange
                .as_ref()
                .is_some_and(|e| e.len() != intervals)
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
        if let Some(exteriors) = &self.exterior_exchange {
            for (film, exterior) in self.boundaries.iter().zip(exteriors) {
                if film.outside_resistance_m2_k_w != 0. || film.outside_c != exterior.air_c {
                    return Err(invalid(
                        "motion exterior exchange requires zero outside film and outside_c equal to air_c in every interval",
                    ));
                }
            }
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
    convective_j: f64,
    radiative_j: f64,
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
    exterior_exchange: Option<SurfaceExteriorExchange>,
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
            convective_j: 0.,
            radiative_j: 0.,
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
        let exteriors = self
            .request
            .exterior_exchange
            .as_ref()
            .map(|e| {
                e.get(interval)
                    .map(|e| vec![e.clone(); self.model.column_count()])
                    .ok_or_else(|| {
                        invalid("motion heat has no exterior environment for this interval")
                    })
            })
            .transpose()?;
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
            let next = match &exteriors {
                Some(e) => {
                    self.model
                        .advance_exterior(&self.state, positions, dt, &boundaries, e)?
                }
                None => self
                    .model
                    .advance(&self.state, positions, dt, &boundaries)?,
            };
            self.inside_j += dt * next.inside_heat_w;
            self.outside_j += dt * next.outside_heat_w;
            if let Some(e) = &next.exterior_exchange {
                self.convective_j += dt * e.convective_heat_w;
                self.radiative_j += dt * e.radiative_heat_w;
            }
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
                exterior_exchange: next.exterior_exchange,
            });
            self.state = next.state;
        }
        if [
            self.inside_j,
            self.outside_j,
            self.convective_j,
            self.radiative_j,
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
        let mut sample = json!({"sample_index":sample_index,"interval_index":interval,
            "time_s":time_s,"thermal_substeps":count,"positions_sha256":hash(&positions)?,
            "temperature_range_c":[lo,hi],"inside_heat_w":field.inside_heat_w,
            "outside_heat_w":field.outside_heat_w,
            "inside_energy_j":self.inside_j,"outside_energy_j":self.outside_j,
            "net_boundary_energy_j":self.net_j,"stored_change_j":self.stored_j,
            "balance_error_j":self.stored_j-self.net_j});
        if let Some(e) = &field.exterior_exchange {
            sample["exterior_exchange"] = json!({"convective_heat_w":e.convective_heat_w,
                "radiative_heat_w":e.radiative_heat_w,"balance_residual_w":e.balance_residual_w,
                "convective_energy_j":self.convective_j,"radiative_energy_j":self.radiative_j});
        }
        self.trace.push(sample);
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
        let mut final_field = json!({"temperature_c":self.state.temperature_c(),
            "area_m2":f.area_m2,"inside_heat_w":f.inside_heat_w,"outside_heat_w":f.outside_heat_w,
            "last_step_seconds":f.seconds,"last_step_stored_change_j":f.stored_change_j,
            "patches":f.patches.iter().map(|p| json!({"area_m2":p.area_m2,
                "inside_flux_w_m2":p.inside_flux_w_m2,"outside_flux_w_m2":p.outside_flux_w_m2,
                "last_step_stored_change_j":p.stored_change_j})).collect::<Vec<_>>()});
        if let Some(e) = &f.exterior_exchange {
            final_field["exterior_exchange"] = json!(e);
        }
        let mut result = json!({"schema":"ftx-motion-surface-heat-result/1","request":self.request,
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
            "scope":"One-way independent-sheet backward Euler on every accepted mechanical sample's current geometry. Storage follows reference mass. Boundary conditions are uniform in space and piecewise constant by keyframe interval; thermal substeps do not interpolate geometry. No inferred layer overlap, body heat loss, thermomechanical feedback, mechanical heating, moisture, comfort or empirical garment claim."});
        if self.request.exterior_exchange.is_some() {
            result["exterior_exchange"] = json!({"convective_energy_j":self.convective_j,
                "radiative_energy_j":self.radiative_j});
        }
        Ok(result)
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
        model_with_emissivity(None)
    }

    fn model_with_emissivity(emissivity: Option<f64>) -> Surface {
        let mut selection = json!({"id":"test-heat","allow_estimates":true,
        "record":{"id":"test-heat","name":"Synthetic heat test","form":"woven",
            "sources":{"test":{"reference":"test:heat-history","accessed_on":"2026-10-17",
                "note":"Synthetic known-answer data, not textile characterization"}},
            "properties":{
                "areal_density":{"value":0.2,"unit":"kg/m2","evidence":"estimate","source":"test","method":"synthetic"},
                "specific_heat":{"value":1000.,"unit":"J/(kg*K)","evidence":"estimate","source":"test","method":"synthetic"},
                "thermal_resistance":{"value":0.4,"unit":"m2*K/W","evidence":"estimate","source":"test","method":"synthetic"}
            }}});
        if let Some(value) = emissivity {
            selection["record"]["properties"]["emissivity"] = json!({"value":value,
                "unit":"1","evidence":"estimate","source":"test","method":"Synthetic optical test",
                "conditions":{"face":"public","spectral_basis":"total_hemispherical",
                    "specimen":"synthetic-sheet","conditioning":"synthetic-dry",
                    "temperature_min_c":"-80","temperature_max_c":"80"}});
        }
        let selection: Selection = serde_json::from_value(selection).unwrap();
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

    fn exterior_policy() -> SurfaceRequest {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/materials/synthetic-exterior-exchange.json"
        ))
        .unwrap();
        let mut request = policy();
        request.boundaries[0].outside_c = 5.;
        request.boundaries[0].outside_resistance_m2_k_w = 0.;
        request.exterior_exchange = Some(vec![
            serde_json::from_value(fixture["exterior_exchange"].clone()).unwrap(),
        ]);
        request
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
        assert!(h["request"].get("exterior_exchange").is_none());
        assert!(h.get("exterior_exchange").is_none());
        assert!(h["trace"][0].get("exterior_exchange").is_none());
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

    #[test]
    fn exterior_motion_tracks_each_interval_and_material_change_without_heat_reset() {
        let mut request = exterior_policy();
        request.boundaries.push(Film {
            outside_c: 20.,
            ..request.boundaries[0]
        });
        let mut second = request.exterior_exchange.as_ref().unwrap()[0].clone();
        second.air_c = 20.;
        second.radiation.as_mut().unwrap().ambient_c = 25.;
        request.exterior_exchange.as_mut().unwrap().push(second);
        request.validate(2).unwrap();
        let model = model_with_emissivity(Some(0.8));
        let mut history = SurfaceHistory::new(request.clone(), model.clone()).unwrap();
        let mut old = model.initial(request.initial_temperature_c).unwrap();
        let (mut convective, mut radiative) = (0., 0.);
        let mut final_exchange = None;
        for (interval, area) in [0.5, 1.].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            let boundaries = [boundary(request.boundaries[interval]).unwrap()];
            let exteriors = [request.exterior_exchange.as_ref().unwrap()[interval].clone()];
            for _ in 0..2 {
                let next = model
                    .advance_exterior(&old, &x, 0.5, &boundaries, &exteriors)
                    .unwrap();
                let e = next.exterior_exchange.as_ref().unwrap();
                convective += 0.5 * e.convective_heat_w;
                radiative += 0.5 * e.radiative_heat_w;
                final_exchange = next.exterior_exchange;
                old = next.state;
            }
            history
                .advance(&x, (interval + 1) as f64, interval, interval)
                .unwrap();
            assert_eq!(history.state.temperature_c(), old.temperature_c());
            let trace = &history.trace[interval]["exterior_exchange"];
            assert_eq!(trace["convective_energy_j"], json!(convective));
            assert_eq!(trace["radiative_energy_j"], json!(radiative));
            assert_eq!(
                trace["radiative_heat_w"],
                json!(final_exchange.as_ref().unwrap().radiative_heat_w)
            );
        }
        let result = history.finish("path", "initial", 2.).unwrap();
        assert_eq!(result["request"], json!(request));
        assert_eq!(
            result["final"]["exterior_exchange"],
            json!(final_exchange.unwrap())
        );
        assert!(
            (result["outside_energy_j"].as_f64().unwrap() - convective - radiative).abs() < 1e-6
        );
        assert!(
            (result["stored_change_j"].as_f64().unwrap()
                - 100. * (old.temperature_c()[0][0] - 10.))
                .abs()
                < 1e-7
        );
        assert!(result["balance_error_j"].as_f64().unwrap().abs() < 1e-6);
        let mut changed = SurfaceHistory::new(request, model_with_emissivity(Some(0.05))).unwrap();
        for (interval, area) in [0.5, 1.].into_iter().enumerate() {
            let x = [
                Vec3::ZERO,
                Vec3::new(2. * area, 0., 0.),
                Vec3::new(0., 1., 0.),
            ];
            changed
                .advance(&x, (interval + 1) as f64, interval, interval)
                .unwrap();
        }
        let changed = changed.finish("path", "initial", 2.).unwrap();
        assert_eq!(result["request"], changed["request"]);
        assert_eq!(
            result["trace"][1]["positions_sha256"],
            changed["trace"][1]["positions_sha256"]
        );
        assert_ne!(result["model"]["sha256"], changed["model"]["sha256"]);
        assert_ne!(
            result["final"]["temperature_c"],
            changed["final"]["temperature_c"]
        );
    }

    #[test]
    fn exterior_motion_requires_every_interval_and_never_silently_uses_films() {
        let request = exterior_policy();
        request.validate(1).unwrap();
        let mut missing = request.clone();
        missing.exterior_exchange = Some(vec![]);
        assert!(missing.validate(1).is_err());
        missing = request.clone();
        missing.boundaries.push(missing.boundaries[0]);
        assert!(missing.validate(2).is_err());
        let mut bad = request.clone();
        bad.boundaries[0].outside_resistance_m2_k_w = 0.1;
        assert!(bad.validate(1).is_err());
        bad = request.clone();
        bad.boundaries[0].outside_c = 6.;
        assert!(bad.validate(1).is_err());
        let x = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        let mut unqualified = SurfaceHistory::new(request, model()).unwrap();
        assert!(unqualified.advance(&x, 1., 0, 0).is_err());
        assert!(unqualified.finish("path", "initial", 1.).is_err());
    }
}

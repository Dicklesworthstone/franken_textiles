//! Source-qualified exterior exchange for the existing conservative thermal time step.
//!
//! The exposed face has no invented storage. Its temperature balances the half-cell conduction
//! path against prescribed convection and material-following radiation. Every trial uses the
//! existing finite-volume/enthalpy solver, and acceptance checks the original exterior equation.

use serde::Serialize;

use super::steady::exterior::{Exchange, Exterior, MaterialFace};
use super::{Boundary, DryThermalModel, State, invalid, nonnegative, positive};
use ftx_core::Refusal;
use ftx_pattern::material_optics::Side;

const ITERATIONS: usize = 128;

/// Signed exchange per unit of the column's current outer sheet area.
#[derive(Clone, Debug, Serialize)]
pub struct ExteriorFlux {
    /// Solved exposed face temperature in Celsius, distinct from the last cell centre.
    pub surface_c: f64,
    /// Prescribed convection to air in W/m²; negative means heat enters the sheet.
    pub convective_flux_w_m2: f64,
    /// Material-qualified radiation to the enclosure in W/m², retaining its sign.
    pub radiative_flux_w_m2: f64,
    /// Whether radiation was requested, rather than assumed to have zero emissivity.
    pub radiation_enabled: bool,
    /// Actual source-bearing observation chosen for the exposed side in automatic mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_face: Option<MaterialFace>,
    /// Conductive outgoing flux minus convection and radiation, per current outer cloth m².
    pub balance_residual_w_m2: f64,
}

/// One complete temperature field and the boundary exchange used to evolve it.
#[derive(Clone, Debug)]
pub struct ExteriorStep {
    /// The existing finite-volume state, with its sensible-heat conservation residual.
    pub state: State,
    /// Separate surface/ambient exchange residual and heat contributions.
    pub exchange: ExteriorFlux,
}

impl DryThermalModel {
    /// Advance with prescribed convection and optional source-qualified exterior radiation.
    /// The supplied outside film must be zero and `outside_c` must equal `exterior.air_c`.
    /// The air can lie outside a material's measured domain when every material temperature and
    /// the solved face remain inside it; the air is not evaluated as a material state point.
    ///
    /// # Errors
    /// Invalid state/time, conflicting films, missing or inadmissible optical/boundary evidence,
    /// out-of-domain material temperatures, or failed thermal/exterior residuals. No state mutation.
    pub fn advance_exterior(
        &self,
        old: &[f64],
        seconds: f64,
        boundary: Boundary,
        exterior: &Exterior,
    ) -> Result<ExteriorStep, Refusal> {
        self.exterior_step(Some(old), seconds, boundary, exterior, 1.0, Side::Positive)
    }

    /// Steady limit of the same discrete column, with its exposed-face balance solved explicitly.
    /// This storage-capable API keeps the transient discretization; transport-only materials use
    /// [`super::steady::SteadyConductionModel::solve_exterior`] without requiring heat capacity.
    ///
    /// # Errors
    /// The same boundary, material domain, evidence and convergence refusals as a transient step.
    pub fn steady_exterior(
        &self,
        boundary: Boundary,
        exterior: &Exterior,
    ) -> Result<ExteriorStep, Refusal> {
        self.exterior_step(None, 1.0, boundary, exterior, 1.0, Side::Positive)
    }

    // Coverage columns use body-area flux and storage units. Their actual outer cloth may have
    // a different current area, while ExteriorFlux always reports per outer cloth area.
    pub(in crate::thermal) fn exterior_step(
        &self,
        old: Option<&[f64]>,
        seconds: f64,
        boundary: Boundary,
        exterior: &Exterior,
        outer_area_per_model_area: f64,
        side: Side,
    ) -> Result<ExteriorStep, Refusal> {
        positive("exterior outer/model area ratio", outer_area_per_model_area)?;
        if let Some(old) = old {
            positive("thermal time step", seconds)?;
            if old.len() != self.cells() {
                return Err(invalid("thermal state has the wrong cell count"));
            }
            for &t in old {
                self.check_temperature(t)?;
            }
        }
        self.check_temperature(boundary.inside_c)?;
        nonnegative("inside film resistance", boundary.inside_resistance)?;
        if boundary.outside_resistance != 0.0 || boundary.outside_c != exterior.air_c {
            return Err(invalid(
                "exterior exchange replaces the outside film: use zero outside resistance and outside_c equal to air_c",
            ));
        }
        let exchange =
            Exchange::on_side(exterior, self.layers.last().expect("nonempty model"), side)?;
        let solve = |b| match old {
            Some(t) => self.advance(t, seconds, b),
            None => self.steady(b),
        };
        let finish = |state: State, surface_c: f64| -> Result<Option<ExteriorStep>, Refusal> {
            self.check_temperature(surface_c)?;
            let [convective, radiative] = exchange.flux(surface_c)?;
            let outgoing = state.outside_flux_w_m2 / outer_area_per_model_area;
            let residual = outgoing - (convective + radiative);
            // Keep boundary mismatch distinct from the finite-volume energy residual. Scaling
            // each signed term also covers near-cancelling radiation and convection.
            let tolerance = 1e-10 * (1.0 + outgoing.abs() + convective.abs() + radiative.abs());
            if !residual.is_finite() || !tolerance.is_finite() {
                return Err(invalid("exterior transient balance overflow"));
            }
            Ok((residual.abs() <= tolerance).then_some(ExteriorStep {
                state,
                exchange: ExteriorFlux {
                    surface_c,
                    convective_flux_w_m2: convective,
                    radiative_flux_w_m2: radiative,
                    radiation_enabled: exterior.radiation.is_some(),
                    material_face: exchange.material_face.clone(),
                    balance_residual_w_m2: residual,
                },
            }))
        };

        // A prescribed positive h with no radiative flux is exactly the existing resistance-film
        // problem. Keep its single solve and arithmetic whenever the reservoir is in the material
        // domain. A cold air reservoir outside that domain instead uses the face solve below.
        if exchange.emissivity == 0.0
            && exterior.convection_w_m2_k > 0.0
            && self.check_temperature(exterior.air_c).is_ok()
        {
            let r = (1.0 / exterior.convection_w_m2_k) / outer_area_per_model_area;
            if r.is_finite() && r > 0.0 {
                let state = solve(Boundary {
                    outside_resistance: r,
                    ..boundary
                })?;
                let surface = exterior.air_c + state.outside_flux_w_m2 * r;
                if let Some(result) = finish(state, surface)? {
                    return Ok(result);
                }
            }
        }

        // Positive material capacities/conductances bound every accepted temperature by the old
        // material state and active reservoirs. Intersect that enclosure with the actual qualified
        // face/material domain; neither a trial nor a returned solution extrapolates a source law.
        let mut low = boundary.inside_c;
        let mut high = low;
        for &t in old.into_iter().flatten() {
            low = low.min(t);
            high = high.max(t);
        }
        if exterior.convection_w_m2_k > 0.0 {
            low = low.min(exterior.air_c);
            high = high.max(exterior.air_c);
        }
        if exchange.emissivity > 0.0 {
            let radiant = exterior
                .radiation
                .as_ref()
                .expect("qualified radiation")
                .ambient_c;
            low = low.min(radiant);
            high = high.max(radiant);
        }
        low = low.max(exchange.domain[0]);
        high = high.min(exchange.domain[1]);
        if let Some([lo, hi]) = self.temperature_domain() {
            low = low.max(lo);
            high = high.min(hi);
        }
        if low > high || !(high - low).is_finite() {
            return Err(invalid("no qualified transient exterior face interval"));
        }
        for iteration in 0..ITERATIONS {
            let surface = match iteration {
                0 => low,
                1 => high,
                _ => low / 2.0 + high / 2.0,
            };
            let state = solve(Boundary {
                outside_c: surface,
                ..boundary
            })?;
            let [convective, radiative] = exchange.flux(surface)?;
            let residual =
                state.outside_flux_w_m2 / outer_area_per_model_area - (convective + radiative);
            if let Some(result) = finish(state, surface)? {
                return Ok(result);
            }
            if (iteration == 0 && residual < 0.0) || (iteration == 1 && residual > 0.0) {
                return Err(invalid(
                    "exterior equilibrium lies outside the qualified face interval",
                ));
            }
            if iteration > 1 {
                if residual > 0.0 {
                    low = surface;
                } else {
                    high = surface;
                }
            }
        }
        Err(invalid(
            "no residual-qualified exterior thermal step within 128 face iterations; reduce the time step or review characterization",
        ))
    }
}

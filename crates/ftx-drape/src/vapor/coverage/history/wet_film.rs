//! Explicit body-side liquid availability under the existing prescribed-reservoir vapor demand.
//! Water is owned by stable body triangles, never by changing projected regions or cloth layers.

use ftx_core::Refusal;
use ftx_pattern::material_vapor::wet_film as canonical;
use serde_json::Value;

use super::{CoverageRequest, Integration, add, invalid, number};

pub use canonical::WetFilm;
pub(super) use canonical::scalar_equivalents;

fn rates(report: &Value, count: usize) -> Result<Vec<f64>, Refusal> {
    let regions = report["coverage"]["regions"]
        .as_array()
        .ok_or_else(|| invalid("wet-film demand needs canonical covered regions"))?;
    let mut values = vec![0.; count];
    for region in regions {
        let body = region["body_triangle"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n < count)
            .ok_or_else(|| invalid("wet-film demand has an unknown body triangle"))?;
        add(
            &mut values[body],
            number(
                &region["vapor"]["potential_mass_flow_kg_s"],
                "covered-region mass flow",
            )?,
        )?;
    }
    Ok(values)
}

#[derive(Clone)]
pub(super) struct State(canonical::State);

impl State {
    /// Validate the independent reservoir before doing any clipping. Boundary/body validation
    /// reuses the canonical request; any refusal remains local to the vapor history consumer.
    pub(super) fn new(policy: &WetFilm, initial: &CoverageRequest) -> Result<Self, Refusal> {
        policy.validate(initial.map.body_triangles.len())?;
        let body_mesh_sha256 = initial.validate_body_binding()?;
        let mut latent_heat = Vec::with_capacity(initial.map.body_triangles.len());
        for i in 0..initial.map.body_triangles.len() {
            let index = if initial.conditions.boundaries.len() == 1 {
                0
            } else {
                i
            };
            latent_heat.push(initial.conditions.boundaries[index].latent_heat_j_kg);
        }
        canonical::State::new(policy, &body_mesh_sha256, &latent_heat).map(Self)
    }

    /// The declared quadrature-average demand and constant supply act throughout this interval.
    /// This admits exact finite-reservoir depletion for that piecewise-constant approximation;
    /// it does not infer a hidden trajectory of coverage or pressure between samples.
    pub(super) fn advance(
        &self,
        previous: &Value,
        current: &Value,
        integration: Integration,
        seconds: f64,
    ) -> Result<(Self, Value), Refusal> {
        let count = self.0.body_triangle_count();
        let current = rates(current, count)?;
        let previous = match integration {
            Integration::CompositeTrapezoid => Some(rates(previous, count)?),
            Integration::RightEndpoint => None,
        };
        let (next, result) = self.0.advance(&current, previous.as_deref(), seconds)?;
        Ok((Self(next), result))
    }

    pub(super) fn report(&self) -> Result<Value, Refusal> {
        self.0.report()
    }
}

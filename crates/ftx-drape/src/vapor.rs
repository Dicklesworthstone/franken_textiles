//! Potential vapor transport on the same retained material geometry as cloth mechanics.
//!
//! These are isothermal prescribed-reservoir comparisons, independent of dry heat readiness.
//! No water supply, evaporation, condensation, moisture storage or wearer comfort is inferred.

pub mod coverage;

/// Complete paired source endpoint ranges, used for rates and their fixed-time integrals.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TransferBounds {
    pub heat: [f64; 2],
    pub mass: [f64; 2],
}

impl TransferBounds {
    pub(crate) fn from_report(report: &ftx_pattern::material_vapor::Report) -> Option<Self> {
        report.reported_bounds.map(|b| Self {
            heat: b.potential_heat_flow_w,
            mass: b.potential_mass_flow_kg_s,
        })
    }

    pub(crate) fn from_json(value: &serde_json::Value, heat: &str, mass: &str) -> Option<Self> {
        Some(Self {
            heat: serde_json::from_value(value[heat].clone()).ok()?,
            mass: serde_json::from_value(value[mass].clone()).ok()?,
        })
    }
}

/// Fixed positive weights preserve the monotonicity of canonical Ret endpoint envelopes.
/// A missing or unrepresentable contribution invalidates only the optional aggregate range.
#[derive(Clone, Debug)]
pub(crate) struct BoundsSum {
    pub value: Option<TransferBounds>,
    pub unavailable: Vec<String>,
}

impl BoundsSum {
    pub(crate) fn new() -> Self {
        Self {
            value: Some(TransferBounds {
                heat: [0.; 2],
                mass: [0.; 2],
            }),
            unavailable: Vec::new(),
        }
    }

    pub(crate) fn add_scaled(
        &mut self,
        incoming: Option<TransferBounds>,
        scale: f64,
        nominal: [f64; 2],
        label: &str,
    ) {
        let Some(mut total) = self.value else {
            return;
        };
        let result = (|| {
            let next =
                incoming.ok_or("complete representable source Ret bounds are unavailable")?;
            if !scale.is_finite() || scale < 0. {
                return Err("range multiplier must be finite and nonnegative");
            }
            for ((range, previous), value) in [next.heat, next.mass]
                .into_iter()
                .zip([&mut total.heat, &mut total.mass])
                .zip(nominal)
            {
                if range.iter().any(|x| !x.is_finite() || *x < 0.) || range[0] > range[1] {
                    return Err("source endpoints must be finite, nonnegative and ordered");
                }
                let scaled = range.map(|x| x * scale);
                if scaled
                    .iter()
                    .enumerate()
                    .any(|(i, x)| !x.is_finite() || (range[i] > 0. && scale > 0. && *x <= 0.))
                {
                    return Err("scaled endpoint overflows or loses positive transfer");
                }
                if !value.is_finite() || scaled[0] > value || value > scaled[1] {
                    return Err("endpoint range does not contain its nominal contribution");
                }
                for (old, increment) in previous.iter_mut().zip(scaled) {
                    let sum = *old + increment;
                    if !sum.is_finite()
                        || (*old > 0. && increment > 0. && (sum <= *old || sum <= increment))
                    {
                        return Err("endpoint sum overflows or loses positive transfer");
                    }
                    *old = sum;
                }
            }
            Ok(total)
        })();
        match result {
            Ok(total) => self.value = Some(total),
            Err(reason) => {
                self.value = None;
                // The first failed contribution suffices; full per-column/region reasons remain
                // in their canonical reports, so huge meshes do not duplicate them per triangle.
                self.unavailable.push(format!("{label}: {reason}"));
            }
        }
    }

    pub(crate) fn json(&self, heat: &str, mass: &str) -> serde_json::Value {
        match self.value {
            Some(value) => serde_json::json!({heat:value.heat,mass:value.mass}),
            None => serde_json::Value::Null,
        }
    }
}

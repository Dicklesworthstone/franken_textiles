//! Shared conversion of accepted membrane dissipation in the exact retained material triangles.
//! Source kinds retain separate budgets of physical work; neither can supply the other's heat.

use ftx_core::Refusal;
use serde_json::{Value, json};

use super::invalid;
use crate::{run::DrapeRun, thermal::surface::io::retained_materials};

#[derive(Clone, Copy)]
struct Conversion {
    fraction: f64,
    temperature_range_c: [f64; 2],
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Plastic,
    Relaxation,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Plastic => "plastic",
            Self::Relaxation => "Maxwell viscous",
        }
    }
}

pub(super) struct Heating {
    kind: Kind,
    previous_j: Vec<f64>,
    fractions: Vec<Option<Conversion>>,
    initial_j: f64,
    dissipated_j: f64,
    supplied_j: f64,
    materials: Vec<Value>,
}

impl Heating {
    // Preserve the covered-layer plastic consumer's constructor while selecting new sources
    // explicitly. Both consumers retain the same accepted-work and conversion authority.
    pub(super) fn new(run: &DrapeRun, temperature_c: f64) -> Result<Self, Refusal> {
        Self::new_kind(run, temperature_c, Kind::Plastic)
    }

    pub(super) fn new_kind(
        run: &DrapeRun,
        temperature_c: f64,
        kind: Kind,
    ) -> Result<Self, Refusal> {
        let (previous_j, history) = match kind {
            Kind::Plastic => (
                run.scene.solver.membrane_plastic_dissipation_j()?,
                run.scene.solver.membrane_plasticity_report()?,
            ),
            Kind::Relaxation => (
                run.scene.solver.membrane_relaxation_dissipation_j()?,
                run.scene.solver.relaxation_report()?,
            ),
        };
        let previous_j = previous_j.ok_or_else(|| {
            invalid(format!(
                "{} heat requires an active membrane law",
                kind.name()
            ))
        })?;
        let history = history
            .ok_or_else(|| invalid(format!("missing accepted {} membrane history", kind.name())))?;
        let (records, assignment) = retained_materials(
            &run.report["material"],
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
        )?;
        if assignment.len() != previous_j.len() {
            return Err(invalid(
                "dissipation heat must retain every material triangle exactly once",
            ));
        }
        let mut fractions = Vec::with_capacity(records.len());
        let mut laws = Vec::with_capacity(records.len());
        let mut materials = Vec::with_capacity(records.len());
        for record in &records {
            let (fraction, law, property) = match kind {
                Kind::Plastic => (
                    record
                        .membrane_plastic_heat_fraction()?
                        .map(|f| Conversion {
                            fraction: f.fraction,
                            temperature_range_c: f.temperature_range_c,
                        }),
                    record.membrane_plasticity()?.map(|law| law.fingerprint()),
                    "membrane_plastic_heat_fraction",
                ),
                Kind::Relaxation => (
                    record
                        .membrane_relaxation_heat_fraction()?
                        .map(|f| Conversion {
                            fraction: f.fraction,
                            temperature_range_c: f.temperature_range_c,
                        }),
                    record
                        .membrane_relaxation()?
                        .map(|law| law.fingerprint().to_owned()),
                    "membrane_relaxation_heat_fraction",
                ),
            };
            if let Some(fraction) = &fraction {
                materials.push(json!({"material_sha256":record.sha256(),
                    "law_sha256":law,"fraction":fraction.fraction,
                    "temperature_range_c":fraction.temperature_range_c,
                    "observation":record.authored().properties[property]}));
            }
            fractions.push(fraction);
            laws.push(law);
        }
        let mut actual_laws = vec![None; previous_j.len()];
        for element in history["elements"]
            .as_array()
            .ok_or_else(|| invalid("dissipation heat needs retained law identities"))?
        {
            let i = element["triangle"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < previous_j.len())
                .ok_or_else(|| invalid("dissipation heat history has an invalid triangle"))?;
            if actual_laws[i].is_some() {
                return Err(invalid(
                    "dissipation heat history duplicates a material triangle",
                ));
            }
            actual_laws[i] = Some(
                element["law_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("dissipation heat history is missing its law identity"))?
                    .to_owned(),
            );
        }
        let mut selected = Vec::with_capacity(assignment.len());
        for (i, material) in assignment.into_iter().enumerate() {
            if actual_laws[i] != laws[material]
                || (fractions[material].is_none() && previous_j[i] != 0.)
            {
                return Err(invalid(
                    "dissipation heat material/law mapping differs from accepted mechanics",
                ));
            }
            selected.push(fractions[material]);
        }
        let initial_j: f64 = previous_j.iter().sum();
        if !initial_j.is_finite() || previous_j.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(invalid(
                "dissipation heat needs finite nonnegative initial dissipation",
            ));
        }
        let result = Self {
            kind,
            previous_j,
            fractions: selected,
            initial_j,
            dissipated_j: 0.,
            supplied_j: 0.,
            materials,
        };
        result.check_temperatures(&vec![vec![temperature_c]; result.fractions.len()])?;
        Ok(result)
    }

    pub(super) fn check_temperatures(&self, temperatures: &[Vec<f64>]) -> Result<(), Refusal> {
        if temperatures.len() != self.fractions.len() {
            return Err(invalid("dissipation heat temperature topology changed"));
        }
        for (values, fraction) in temperatures.iter().zip(&self.fractions) {
            if let Some(fraction) = fraction {
                let [lo, hi] = fraction.temperature_range_c;
                if values.is_empty() || values.iter().any(|t| !t.is_finite() || *t < lo || *t > hi)
                {
                    return Err(invalid(
                        "dissipation heat temperature exceeds the conversion observation's qualified domain",
                    ));
                }
            }
        }
        Ok(())
    }

    // The caller owns a private thermal history: any failure discards it rather than publishing
    // partial heating. Cumulative mechanics are sampled only after accepted physical steps.
    pub(super) fn increment(
        &mut self,
        cumulative_j: Option<&[f64]>,
        cells: usize,
    ) -> Result<Vec<Vec<f64>>, Refusal> {
        let values = cumulative_j
            .ok_or_else(|| invalid("accepted motion is missing membrane dissipation samples"))?;
        if values.len() != self.previous_j.len() || cells == 0 {
            return Err(invalid("membrane dissipation sample topology changed"));
        }
        let mut heat = Vec::with_capacity(values.len());
        let mut dissipated = 0.;
        let mut supplied = 0.;
        for ((&current, &previous), fraction) in
            values.iter().zip(&self.previous_j).zip(&self.fractions)
        {
            if !current.is_finite() || current < previous {
                return Err(invalid(
                    "accepted membrane dissipation must be finite and nondecreasing",
                ));
            }
            let delta = current - previous;
            if fraction.is_none() && delta != 0. {
                return Err(invalid(
                    "unqualified triangle cannot supply dissipation heat",
                ));
            }
            let q = fraction.as_ref().map_or(0., |f| f.fraction * delta);
            dissipated += delta;
            supplied += q;
            heat.push(vec![q / cells as f64; cells]);
        }
        if !dissipated.is_finite()
            || !supplied.is_finite()
            || !(self.dissipated_j + dissipated).is_finite()
            || !(self.supplied_j + supplied).is_finite()
        {
            return Err(invalid("dissipation heat work accumulation overflows"));
        }
        self.previous_j.copy_from_slice(values);
        self.dissipated_j += dissipated;
        self.supplied_j += supplied;
        Ok(heat)
    }

    pub(super) fn supplied_j(&self) -> f64 {
        self.supplied_j
    }

    pub(super) fn snapshot(&self, include_materials: bool) -> Value {
        let mut result = json!({"distribution":"uniform_reference_thickness",
            "temporal_distribution":"uniform_over_accepted_interval",
            "plastic_dissipation_j":self.dissipated_j,"supplied_heat_j":self.supplied_j,
            "unconverted_dissipation_j":self.dissipated_j-self.supplied_j,
            "initial_plastic_dissipation_j":self.initial_j,
            "scope":"Only accepted irreversible membrane yield dissipation after motion time zero is converted. Initial drape work, elastic/hardening storage, bending, Maxwell and contact friction are excluded. Unconverted dissipation is not assigned another destination; temperature does not feed back into mechanics."});
        if let Kind::Relaxation = self.kind {
            let object = result.as_object_mut().expect("work receipt is an object");
            object.remove("plastic_dissipation_j");
            object.remove("initial_plastic_dissipation_j");
            result["viscous_dissipation_j"] = json!(self.dissipated_j);
            result["initial_viscous_dissipation_j"] = json!(self.initial_j);
            result["scope"] = json!(
                "Only accepted Maxwell dashpot endpoint-quadrature dissipation after motion time zero is converted. Initial drape work, reversible spring storage, backward-Euler numerical loss, plastic yield, bending and contact friction are excluded. Unconverted dissipation is not assigned another destination; temperature does not feed back into mechanics."
            );
        }
        if include_materials {
            result["material_fractions"] = json!(self.materials);
        }
        result
    }
}

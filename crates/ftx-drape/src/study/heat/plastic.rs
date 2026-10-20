//! Conversion of accepted irreversible membrane work in the exact retained material triangles.

use ftx_core::Refusal;
use ftx_pattern::material_membrane_plasticity::PlasticHeatFraction;
use serde_json::{Value, json};

use super::invalid;
use crate::{run::DrapeRun, thermal::surface::io::retained_materials};

pub(super) struct Heating {
    previous_j: Vec<f64>,
    fractions: Vec<Option<PlasticHeatFraction>>,
    initial_j: f64,
    dissipated_j: f64,
    supplied_j: f64,
    materials: Vec<Value>,
}

impl Heating {
    pub(super) fn new(run: &DrapeRun, temperature_c: f64) -> Result<Self, Refusal> {
        let previous_j = run
            .scene
            .solver
            .membrane_plastic_dissipation_j()?
            .ok_or_else(|| invalid("plastic heat requires an active permanent membrane law"))?;
        let history = run
            .scene
            .solver
            .membrane_plasticity_report()?
            .ok_or_else(|| invalid("missing accepted plastic membrane history"))?;
        let (records, assignment) = retained_materials(
            &run.report["material"],
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
        )?;
        if assignment.len() != previous_j.len() {
            return Err(invalid(
                "plastic heat must retain every material triangle exactly once",
            ));
        }
        let mut fractions = Vec::with_capacity(records.len());
        let mut laws = Vec::with_capacity(records.len());
        let mut materials = Vec::with_capacity(records.len());
        for record in &records {
            let fraction = record.membrane_plastic_heat_fraction()?;
            let law = record.membrane_plasticity()?.map(|law| law.fingerprint());
            if let Some(fraction) = &fraction {
                materials.push(json!({"material_sha256":record.sha256(),
                    "law_sha256":law,"fraction":fraction.fraction,
                    "temperature_range_c":fraction.temperature_range_c,
                    "observation":record.authored().properties["membrane_plastic_heat_fraction"]}));
            }
            fractions.push(fraction);
            laws.push(law);
        }
        let mut actual_laws = vec![None; previous_j.len()];
        for element in history["elements"]
            .as_array()
            .ok_or_else(|| invalid("plastic heat needs retained law identities"))?
        {
            let i = element["triangle"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < previous_j.len())
                .ok_or_else(|| invalid("plastic heat history has an invalid triangle"))?;
            if actual_laws[i].is_some() {
                return Err(invalid(
                    "plastic heat history duplicates a material triangle",
                ));
            }
            actual_laws[i] = Some(
                element["law_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("plastic heat history is missing its law identity"))?
                    .to_owned(),
            );
        }
        let mut selected = Vec::with_capacity(assignment.len());
        for (i, material) in assignment.into_iter().enumerate() {
            if actual_laws[i] != laws[material]
                || (fractions[material].is_none() && previous_j[i] != 0.)
            {
                return Err(invalid(
                    "plastic heat material/law mapping differs from accepted mechanics",
                ));
            }
            selected.push(fractions[material]);
        }
        let initial_j: f64 = previous_j.iter().sum();
        if !initial_j.is_finite() || previous_j.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(invalid(
                "plastic heat needs finite nonnegative initial dissipation",
            ));
        }
        let result = Self {
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
            return Err(invalid("plastic heat temperature topology changed"));
        }
        for (values, fraction) in temperatures.iter().zip(&self.fractions) {
            if let Some(fraction) = fraction {
                let [lo, hi] = fraction.temperature_range_c;
                if values.is_empty() || values.iter().any(|t| !t.is_finite() || *t < lo || *t > hi)
                {
                    return Err(invalid(
                        "plastic heat temperature exceeds the conversion observation's qualified domain",
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
            .ok_or_else(|| invalid("accepted motion is missing plastic dissipation samples"))?;
        if values.len() != self.previous_j.len() || cells == 0 {
            return Err(invalid("plastic dissipation sample topology changed"));
        }
        let mut heat = Vec::with_capacity(values.len());
        let mut dissipated = 0.;
        let mut supplied = 0.;
        for ((&current, &previous), fraction) in
            values.iter().zip(&self.previous_j).zip(&self.fractions)
        {
            if !current.is_finite() || current < previous {
                return Err(invalid(
                    "accepted plastic dissipation must be finite and nondecreasing",
                ));
            }
            let delta = current - previous;
            if fraction.is_none() && delta != 0. {
                return Err(invalid("unqualified triangle cannot supply plastic heat"));
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
            return Err(invalid("plastic heat work accumulation overflows"));
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
        if include_materials {
            result["material_fractions"] = json!(self.materials);
        }
        result
    }
}

//! One-way coupling of qualified normal compression to dry transient heat storage/transport.
//!
//! A step first changes the rigid-face clearance quasi-statically, then holds that state while
//! advancing the existing heat solver. Cells follow reference material mass, not current volume.
//! Mechanical work stays a separate ledger: a pressure/thickness curve cannot identify heating.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{
    material_compression::stack,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde::{Deserialize, Serialize};

use super::{Boundary, DryThermalModel, adaptive, invalid, positive};

/// Explicit source-qualified branch reversals through the same material/thermal state machine.
pub mod path;

/// Maximum committed steps in one local compression/thermal history.
pub const MAX_STEPS: usize = 512;
/// Maximum through-thickness cells; the area and material discretization stay fixed.
pub const MAX_CELLS: usize = 1024;

/// Source-qualified initial mechanical state and explicitly supplied thermal state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Existing inverse request: its final gap defines the state at thermal time zero.
    /// Its preparation work precedes this history and is retained separately, not counted twice.
    pub stack: stack::Input,
    /// Constant equal-reference-mass cell count in each layer, 1..=64.
    pub cells_per_layer: usize,
    /// One temperature per cell, in inside-to-outside layer/depth order, AFTER initial preparation.
    pub initial_temperature_c: Vec<f64>,
    /// Source/application receipt for constant thermal properties, prescribed interfaces/films,
    /// the temperature/time range and omission of mechanical-to-thermal energy conversion.
    pub thermal_evidence: String,
}

/// A prescribed hold, not a linearly interpolated squeeze or a detected garment-contact event.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hold {
    /// Positive duration AFTER changing to this gap. No thermal time is assigned to the squeeze.
    pub seconds: f64,
    /// New rigid-face clearance in metres, on the original declared branch and direction.
    pub gap_m: f64,
    /// One explicitly prescribed interlayer resistance per interface, m² K/W, at this hold.
    /// These are not inferred from pressure or silently reused from a previous contact state.
    pub interface_resistance_m2_k_w: Vec<f64>,
    /// Prescribed inner reservoir/surface temperature, degrees Celsius.
    pub inside_c: f64,
    /// Prescribed outer reservoir/surface temperature, degrees Celsius.
    pub outside_c: f64,
    /// Explicit inner film resistance, m² K/W; zero means prescribed face temperature.
    pub inside_resistance_m2_k_w: f64,
    /// Explicit outer film resistance, m² K/W.
    pub outside_resistance_m2_k_w: f64,
}

impl Hold {
    fn boundary(&self) -> Boundary {
        Boundary {
            inside_c: self.inside_c,
            outside_c: self.outside_c,
            inside_resistance: self.inside_resistance_m2_k_w,
            outside_resistance: self.outside_resistance_m2_k_w,
        }
    }
}

/// Explicit quasi-static normal boundary; pressure and final clearance cannot both be imposed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Control {
    /// Prescribed rigid-face separation; the qualified branches determine pressure.
    Gap {
        /// Positive final clearance, metres.
        gap_m: f64,
    },
    /// Prescribed uniform pressure; the qualified branches determine separation.
    Pressure {
        /// Nonnegative normal pressure, Pa, within every active source branch.
        pressure_pa: f64,
    },
}

/// A version-3 hold with one explicit pressure or clearance boundary. All thermal inputs retain
/// the original hold's meaning; pressure does not imply a film or interface heat-transfer law.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlledHold {
    /// Positive duration after the quasi-static change, seconds.
    pub seconds: f64,
    /// Exactly one prescribed mechanical boundary.
    pub control: Control,
    /// One independently prescribed resistance per physical layer interface, m² K/W.
    pub interface_resistance_m2_k_w: Vec<f64>,
    /// Prescribed inner temperature, degrees Celsius.
    pub inside_c: f64,
    /// Prescribed outer temperature, degrees Celsius.
    pub outside_c: f64,
    /// Explicit inner film resistance, m² K/W.
    pub inside_resistance_m2_k_w: f64,
    /// Explicit outer film resistance, m² K/W.
    pub outside_resistance_m2_k_w: f64,
}

impl ControlledHold {
    fn parameters(&self, initial_gap: f64) -> (Hold, Option<f64>) {
        let (gap_m, pressure) = match self.control {
            Control::Gap { gap_m } => (gap_m, None),
            Control::Pressure { pressure_pa } => (initial_gap, Some(pressure_pa)),
        };
        (
            Hold {
                seconds: self.seconds,
                gap_m,
                interface_resistance_m2_k_w: self.interface_resistance_m2_k_w.clone(),
                inside_c: self.inside_c,
                outside_c: self.outside_c,
                inside_resistance_m2_k_w: self.inside_resistance_m2_k_w,
                outside_resistance_m2_k_w: self.outside_resistance_m2_k_w,
            },
            pressure,
        )
    }
}

/// A committed hold's geometry, source identity and separate thermal/mechanical ledgers.
#[derive(Clone, Debug, Serialize)]
pub struct StepReport {
    /// Time since the caller-supplied initial temperature state, seconds.
    pub elapsed_s: f64,
    /// Common normal pressure obtained by the existing inverse solver, Pa.
    pub pressure_pa: f64,
    /// Explicit pressure boundary, present only on pressure-controlled holds. Otherwise pressure
    /// was inferred from the prescribed clearance. No measured contact field is implied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prescribed_pressure_pa: Option<f64>,
    /// Sum of the conditioned layer thicknesses, metres.
    pub thickness_m: f64,
    /// Closure residual relative to this hold's gap, metres.
    pub closure_residual_m: f64,
    /// Pressure times fixed reference area, newtons.
    pub normal_force_n: f64,
    /// Full thicknesses in physical layer order, metres.
    pub layer_thickness_m: Vec<f64>,
    /// Existing frozen material locks in the same layer order; no alternate material authority.
    pub layer_material_sha256: Vec<String>,
    /// Preserved total reference mass, kilograms; never density times compressed volume.
    pub reference_mass_kg: f64,
    /// Through-thickness temperatures in the original material-cell order.
    pub temperature_c: Vec<f64>,
    /// End-of-step implicit inner boundary flux, W/m², positive into the stack.
    pub inside_flux_w_m2: f64,
    /// End-of-step implicit outer boundary flux, W/m², positive out of the stack.
    pub outside_flux_w_m2: f64,
    /// Integrated heat entering this hold, J; adaptive mode sums accepted half steps.
    pub inside_heat_j: f64,
    /// Integrated heat leaving this hold, J; final instantaneous flux is reported separately.
    pub outside_heat_j: f64,
    /// This hold's sensible stored-energy change, joules.
    pub stored_change_j: f64,
    /// Stored change minus entering heat plus leaving heat, joules.
    pub heat_balance_error_j: f64,
    /// Mechanical branch work for this gap change, joules; not injected as heat.
    pub external_work_j: f64,
    /// Sum of gap-change work since time zero, excluding initial preparation work.
    pub cumulative_external_work_j: f64,
    /// Integrated inner boundary heat since time zero, joules.
    pub cumulative_inside_heat_j: f64,
    /// Integrated outer boundary heat since time zero, joules.
    pub cumulative_outside_heat_j: f64,
    /// Sensible stored energy relative to the caller's initial thermal state, joules.
    pub cumulative_stored_change_j: f64,
    /// Cumulative sensible energy balance, not a complete thermomechanical energy balance.
    pub cumulative_heat_balance_error_j: f64,
    /// Optional local time-error estimates and charged work, not a global accuracy bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<adaptive::Report>,
}

/// Stateful local column. There are no public mutable fields or externally substitutable materials.
/// Every failed advance leaves the preceding pressure, temperatures and ledgers unchanged.
#[derive(Clone, Debug)]
pub struct CompressedStack {
    setup: Setup,
    compression: stack::Report,
    capacity: Vec<f64>,
    temperature: Vec<f64>,
    committed_steps: usize,
    last: Option<StepReport>,
}

fn model(
    compression: &stack::Report,
    interfaces: &[f64],
    cells: usize,
) -> Result<DryThermalModel, Refusal> {
    // Embedded selections are self-contained. Do not resolve a moving catalog revision.
    let catalog = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let layers = compression
        .layers
        .iter()
        .map(|l| catalog.resolve(&l.conditioned.selection))
        .collect::<Result<_, _>>()?;
    DryThermalModel::new(layers, interfaces.to_vec(), cells)
}

fn checked_product(a: f64, b: f64, label: &str) -> Result<f64, Refusal> {
    let v = a * b;
    if !v.is_finite() || (a != 0.0 && b != 0.0 && v == 0.0) {
        return Err(invalid(format!(
            "compressed thermal {label} is not representable"
        )));
    }
    Ok(v)
}

fn check_storage(input: &stack::Input) -> Result<(), Refusal> {
    for layer in &input.layers {
        for key in ["areal_density", "specific_heat"] {
            if !layer.preparation.invariants.contains_key(key)
                || layer
                    .preparation
                    .samples
                    .iter()
                    .any(|s| s.properties.contains_key(key))
            {
                return Err(invalid(format!(
                    "{}: compressed thermal storage needs independently qualified pressure-invariant {key}; sampled storage is unsupported",
                    layer.id
                )));
            }
        }
    }
    Ok(())
}

impl CompressedStack {
    /// Prepare the given initial gap and attach the caller's initial temperatures.
    /// No heat evolution or mechanical heating is inferred during initial preparation.
    ///
    /// # Errors
    /// Invalid compression evidence/geometry; missing mass, storage or transport; unqualified
    /// pressure-invariant storage; invalid temperatures; or excess cells. No fiber defaults.
    pub fn new(setup: &Setup) -> Result<Self, Refusal> {
        let cells = setup.stack.layers.len().checked_mul(setup.cells_per_layer);
        if !(1..=64).contains(&setup.cells_per_layer)
            || cells.is_none_or(|n| n == 0 || n > MAX_CELLS)
            || cells != Some(setup.initial_temperature_c.len())
            || setup.thermal_evidence.trim().is_empty()
            || setup.thermal_evidence.len() > 4096
        {
            return Err(invalid(
                "compressed thermal setup needs 1..64 cells per layer, at most 1024 cells, matching initial temperatures and explicit thermal evidence",
            ));
        }
        check_storage(&setup.stack)?;
        let compression = stack::close(&setup.stack)?;
        let thermal = model(
            &compression,
            &vec![0.0; compression.layers.len() - 1],
            setup.cells_per_layer,
        )?;
        // The zero interfaces above only check storage/transport. No initial flux is computed,
        // and every actual hold requires its own explicit interface resistances.
        for &t in &setup.initial_temperature_c {
            thermal.check_temperature(t)?;
        }
        if thermal.variable.is_some() || compression.reference_mass_kg.is_none() {
            return Err(invalid(
                "compressed thermal history requires fixed mass and constant pressure-invariant heat capacity",
            ));
        }
        Ok(Self {
            setup: setup.clone(),
            compression,
            capacity: thermal.capacity,
            temperature: setup.initial_temperature_c.clone(),
            committed_steps: 0,
            last: None,
        })
    }

    /// Current original-format compression receipt, including every exact frozen selection.
    #[must_use]
    pub fn compression(&self) -> &stack::Report {
        &self.compression
    }

    /// Current material-cell temperatures; a failed step never changes them.
    #[must_use]
    pub fn temperatures(&self) -> &[f64] {
        &self.temperature
    }

    /// Last committed hold's ledgers, or None before the first hold.
    #[must_use]
    pub fn last(&self) -> Option<&StepReport> {
        self.last.as_ref()
    }

    /// Solve the new clearance, retain mass-cell temperatures, and advance boundary-driven heat.
    /// The new resistance is held for the entire thermal interval (backward Euler); it is NOT
    /// integrated along a continuous squeeze. The original branch/direction cannot be switched.
    ///
    /// # Errors
    /// Refused compression/thermal solve, unrepresentable time/energy, changed storage or more than
    /// 512 committed steps. Preparation and all ledgers commit together only after every check.
    pub fn advance(&mut self, hold: &Hold) -> Result<StepReport, Refusal> {
        self.advance_inner(hold, None, None)
    }

    /// Advance a hold with the existing adaptive thermal integrator and a caller-owned budget.
    /// Failed attempts retain charged work in `budget` but leave physical state unchanged.
    ///
    /// # Errors
    /// Ordinary source/compression refusals, unmet local tolerances or exhausted adaptive work.
    pub fn advance_adaptive(
        &mut self,
        hold: &Hold,
        budget: &mut adaptive::Budget,
    ) -> Result<StepReport, Refusal> {
        self.advance_inner(hold, None, Some(budget))
    }

    /// Apply an explicit pressure or clearance change, then advance the same conservative heat
    /// state. A pressure plateau is well-posed under pressure control; no inverse is fabricated.
    ///
    /// # Errors
    /// Unqualified sources, domain/direction violations, inconsistent storage, invalid thermal
    /// boundaries or exhausted work. Every failed hold leaves all physical state unchanged.
    pub fn advance_controlled(&mut self, hold: &ControlledHold) -> Result<StepReport, Refusal> {
        let (hold, pressure) = hold.parameters(self.compression.thickness_m);
        self.advance_inner(&hold, pressure, None)
    }

    /// Apply an explicit pressure or clearance boundary with one caller-owned adaptive budget.
    ///
    /// # Errors
    /// As [`Self::advance_controlled`]; failed work remains charged without committing physics.
    pub fn advance_controlled_adaptive(
        &mut self,
        hold: &ControlledHold,
        budget: &mut adaptive::Budget,
    ) -> Result<StepReport, Refusal> {
        let (hold, pressure) = hold.parameters(self.compression.thickness_m);
        self.advance_inner(&hold, pressure, Some(budget))
    }

    fn advance_inner(
        &mut self,
        hold: &Hold,
        prescribed_pressure: Option<f64>,
        budget: Option<&mut adaptive::Budget>,
    ) -> Result<StepReport, Refusal> {
        if self.committed_steps == MAX_STEPS {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "compressed thermal step budget exhausted",
            ));
        }
        positive("compressed thermal hold duration", hold.seconds)?;
        let elapsed = self.last.as_ref().map_or(0.0, |s| s.elapsed_s);
        let next_time = elapsed + hold.seconds;
        if !next_time.is_finite() || next_time <= elapsed {
            return Err(invalid(
                "compressed thermal time increment is not representable",
            ));
        }
        // Always prepare from the ORIGINAL acquisitions. Reconditioning the already prepared
        // material would lose the original branches/properties or recursively accumulate receipts.
        let mut request = self.setup.stack.clone();
        request.gap_m = hold.gap_m;
        for layer in &mut request.layers {
            layer.preparation.pressure_pa = self.compression.pressure_pa;
        }
        let next = match prescribed_pressure {
            Some(pressure) => stack::at_pressure(&request, pressure)?,
            None => stack::close(&request)?,
        };
        let thermal = model(
            &next,
            &hold.interface_resistance_m2_k_w,
            self.setup.cells_per_layer,
        )?;
        if thermal.capacity != self.capacity
            || next.reference_mass_kg != self.compression.reference_mass_kg
        {
            return Err(invalid(
                "compressed thermal material-cell storage or reference mass changed",
            ));
        }
        let (state, integrated_heat, integration) = match budget {
            Some(budget) => {
                let next = thermal.advance_adaptive(
                    &self.temperature,
                    hold.seconds,
                    hold.boundary(),
                    budget,
                )?;
                (
                    next.state,
                    Some([next.inside_heat_j_m2, next.outside_heat_j_m2]),
                    Some(next.integration),
                )
            }
            None => (
                thermal.advance(&self.temperature, hold.seconds, hold.boundary())?,
                None,
                None,
            ),
        };
        let area = request.reference_area_m2;
        let heat = |flux| {
            checked_product(
                checked_product(flux, hold.seconds, "heat per area")?,
                area,
                "heat",
            )
        };
        let (inside, outside) = match integrated_heat {
            Some([inside, outside]) => (
                checked_product(inside, area, "integrated entering heat")?,
                checked_product(outside, area, "integrated leaving heat")?,
            ),
            None => (
                heat(state.inside_flux_w_m2)?,
                heat(state.outside_flux_w_m2)?,
            ),
        };
        let stored = checked_product(state.stored_change_j_m2, area, "stored heat")?;
        let error = stored - (inside - outside);
        let prior = self.last.as_ref();
        let ci = prior.map_or(0.0, |s| s.cumulative_inside_heat_j) + inside;
        let co = prior.map_or(0.0, |s| s.cumulative_outside_heat_j) + outside;
        let cw = prior.map_or(0.0, |s| s.cumulative_external_work_j) + next.external_work_j;
        let cs = checked_product(
            self.capacity
                .iter()
                .zip(&state.temperature_c)
                .zip(&self.setup.initial_temperature_c)
                .map(|((c, t), t0)| c * (t - t0))
                .sum(),
            area,
            "cumulative stored heat",
        )?;
        let ce = cs - (ci - co);
        let rounding: f64 = self
            .capacity
            .iter()
            .zip(&state.temperature_c)
            .zip(&self.setup.initial_temperature_c)
            .map(|((c, t), t0)| c * (1.0 + t.abs() + t0.abs()))
            .sum();
        let tolerance = 1024.0 * f64::EPSILON * rounding * area * (self.committed_steps + 1) as f64
            + 1e-9 * (1.0 + ci.abs().max(co.abs()).max(cs.abs()));
        if [error, ci, co, cw, cs, ce, rounding, tolerance]
            .iter()
            .any(|v| !v.is_finite())
            || ce.abs() > tolerance
        {
            return Err(invalid(
                "compressed thermal cumulative sensible-energy balance failed",
            ));
        }
        let report = StepReport {
            elapsed_s: next_time,
            pressure_pa: next.pressure_pa,
            prescribed_pressure_pa: prescribed_pressure,
            thickness_m: next.thickness_m,
            closure_residual_m: next.closure_residual_m,
            normal_force_n: next.normal_force_n,
            layer_thickness_m: next.layers.iter().map(|l| l.thickness_m).collect(),
            layer_material_sha256: thermal
                .layers
                .iter()
                .map(|m| m.sha256().to_string())
                .collect(),
            reference_mass_kg: next.reference_mass_kg.expect("validated storage"),
            temperature_c: state.temperature_c.clone(),
            inside_flux_w_m2: state.inside_flux_w_m2,
            outside_flux_w_m2: state.outside_flux_w_m2,
            inside_heat_j: inside,
            outside_heat_j: outside,
            stored_change_j: stored,
            heat_balance_error_j: error,
            external_work_j: next.external_work_j,
            cumulative_external_work_j: cw,
            cumulative_inside_heat_j: ci,
            cumulative_outside_heat_j: co,
            cumulative_stored_change_j: cs,
            cumulative_heat_balance_error_j: ce,
            integration,
        };
        self.compression = next;
        self.temperature = state.temperature_c;
        self.committed_steps += 1;
        self.last = Some(report.clone());
        Ok(report)
    }
}

/// Portable, local-only sequence of prescribed compression changes and thermal holds.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// `ftx-compressed-thermal-history/1`.
    pub schema: String,
    /// Source acquisitions, fixed reference area, initial prepared gap and post-preparation heat state.
    pub setup: Setup,
    /// One to 512 prescribed holds. No branch reversal or hidden time interpolation.
    pub holds: Vec<Hold>,
    /// Optional local time-error control; one work budget spans the complete history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<adaptive::Options>,
}

fn adaptive_budget(
    options: Option<&adaptive::Options>,
) -> Result<Option<adaptive::Budget>, Refusal> {
    options
        .map(|options| {
            if options.max_cell_solves > 262_144 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "adaptive compression history permits at most 262144 attempted cell solves",
                ));
            }
            adaptive::Budget::new(options.clone())
        })
        .transpose()
}

/// Complete successful history. Any refused step returns an error instead of a partial success.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-compressed-thermal-history-report/1`.
    pub schema: &'static str,
    /// Original typed request, including all source observations and boundary assumptions.
    pub input: Input,
    /// Identity of that request, not an authentication or validation certificate.
    pub input_sha256: String,
    /// Initial mechanical preparation, whose work is outside the subsequent heat history.
    pub initial_compression: stack::Report,
    /// One cell per completed prescribed hold, with actual states and conservation diagnostics.
    pub steps: Vec<StepReport>,
    /// Final ordinary material selections, replayable through existing downstream consumers.
    pub final_compression: stack::Report,
    /// Scope of the one-way sequential coupling and its unmodeled energy terms.
    pub no_claim: &'static str,
}

/// Execute a bounded history through the ordinary compression and heat authorities.
///
/// # Errors
/// Unsupported schema, excessive work/output, or any refused setup/hold. The caller's input is
/// never modified and no partial report or usable replacement selection is returned on failure.
pub fn evaluate(input: &Input) -> Result<Report, Refusal> {
    if input.schema != "ftx-compressed-thermal-history/1" {
        return Err(invalid("expected ftx-compressed-thermal-history/1"));
    }
    let work = input
        .setup
        .stack
        .layers
        .len()
        .checked_mul(input.setup.cells_per_layer)
        .and_then(|n| n.checked_mul(input.holds.len()));
    if input.holds.is_empty() || input.holds.len() > MAX_STEPS || work.is_none_or(|n| n > 262_144) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "compressed thermal history needs 1..512 holds and at most 262144 cell-holds",
        ));
    }
    let mut budget = adaptive_budget(input.adaptive.as_ref())?;
    let mut state = CompressedStack::new(&input.setup)?;
    let initial_compression = state.compression().clone();
    let mut steps = Vec::with_capacity(input.holds.len());
    for hold in &input.holds {
        steps.push(match &mut budget {
            Some(budget) => state.advance_adaptive(hold, budget)?,
            None => state.advance(hold)?,
        });
    }
    let input_sha256 = sha256_hex(&serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?);
    Ok(Report {
        schema: "ftx-compressed-thermal-history-report/1",
        input: input.clone(),
        input_sha256,
        initial_compression,
        steps,
        final_compression: state.compression().clone(),
        no_claim: "One-way quasi-static rigid-face compression followed by dry constant-property thermal holds, at fixed reference area and pressure-invariant mass/heat capacity. Interface/film resistances are explicitly prescribed, not inferred from pressure. Mechanical work is retained separately, not converted to heat or split into recovery/dissipation. No continuous squeeze dynamics, branch switching, recovery time, thermomechanical feedback, moisture, garment contact field, empirical textile validation or comfort claim.",
    })
}

/// Bounded JSON boundary shared by native and WebAssembly callers. Version 1 retains its original
/// single-branch contract; version 2 adds acquired-branch legs; version 3 selects one explicit
/// pressure or clearance boundary per hold. No version infers a loading/unloading reversal.
///
/// # Errors
/// More than 64 MiB, invalid/unsupported schema or fields, or any refused setup, transition or hold.
/// No partial report is serialized and original frozen selection strings are retained exactly.
pub fn evaluate_json(input: &str) -> Result<String, Refusal> {
    if input.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "compressed thermal input exceeds 64 MiB",
        ));
    }
    // The header selects a parser; the full strict typed parse below still rejects unknown fields.
    #[derive(Deserialize)]
    struct Header {
        schema: String,
    }
    let parse_error = |e: serde_json::Error| invalid(e.to_string());
    let header: Header = serde_json::from_str(input).map_err(parse_error)?;
    match header.schema.as_str() {
        "ftx-compressed-thermal-history/1" => {
            let input: Input = serde_json::from_str(input).map_err(parse_error)?;
            serde_json::to_string(&evaluate(&input)?).map_err(parse_error)
        }
        "ftx-compressed-thermal-history/2" => {
            let input: path::Input = serde_json::from_str(input).map_err(parse_error)?;
            serde_json::to_string(&path::evaluate(&input)?).map_err(parse_error)
        }
        "ftx-compressed-thermal-history/3" => {
            let input: path::ControlledInput = serde_json::from_str(input).map_err(parse_error)?;
            serde_json::to_string(&path::evaluate_controlled(&input)?).map_err(parse_error)
        }
        _ => Err(invalid(
            "expected ftx-compressed-thermal-history/1, /2 or /3",
        )),
    }
}

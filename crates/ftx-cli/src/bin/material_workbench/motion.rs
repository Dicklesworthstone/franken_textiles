//! Timed prescribed poses: conservative instantaneous remap, then heat on the arrival geometry.
//! Playback frames are not timestamps. A material change never resets or borrows an old heat state.
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_drape::thermal::{
    Boundary,
    microclimate::MicroclimateExterior,
    steady::exterior::Exterior,
    surface::coverage::{
        ExteriorConditions,
        motion::{ExposedConditions, Frame, State},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    invalid, io, read,
    thermal_coverage::{self, Film},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Initial {
    Uniform {
        time_seconds: f64,
        drape_report: String,
        coverage: String,
        temperature_c: f64,
    },
    Resume {
        checkpoint: String,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Interval {
    /// End time of the interval, in physical seconds, strictly after the preceding time.
    time_seconds: f64,
    drape_report: String,
    coverage: String,
    /// Right-endpoint fields used throughout this interval; no implicit pose interpolation.
    boundaries: Vec<Film>,
    /// Optional qualified exchange on outermost covered faces, in destination body-triangle order.
    /// One environment broadcasts; the covered outside film must be zero and match its air state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exterior_exchange: Option<Vec<Exterior>>,
    /// Optional validity-gated moist-air Nusselt correlation replacement on outermost covered faces
    /// (`Evidence::Estimate`; mutually exclusive with `exterior_exchange`). One entry broadcasts;
    /// otherwise destination body-triangle order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    microclimate_exterior: Option<Vec<MicroclimateExterior>>,
    /// Bare-body films are explicit whenever covered exterior exchange is selected. Cloth optics
    /// cannot supply skin emissivity, and an unused bare film must not be silently ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bare_boundaries: Option<Vec<Film>>,
    /// Optional source-cloth face conditions for material currently outside body coverage.
    /// One broadcasts to every source cloth triangle; otherwise retain source-triangle order.
    #[serde(default)]
    exposed_boundaries: Vec<Film>,
    /// Independent exterior replacement for explicitly conditioned uncovered cloth. One entry
    /// broadcasts; otherwise keep source-triangle order, including inactive covered triangles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exposed_exterior_exchange: Option<Vec<Exterior>>,
    /// Independent validity-gated moist-air Nusselt correlation replacement for uncovered cloth
    /// (`Evidence::Estimate`; mutually exclusive with `exposed_exterior_exchange`). One entry
    /// broadcasts; otherwise keep source-triangle order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exposed_microclimate_exterior: Option<Vec<MicroclimateExterior>>,
    steps: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    initial: Initial,
    frames: Vec<Interval>,
}

struct Inputs {
    base: PathBuf,
    bytes: usize,
}
impl Inputs {
    fn load(&mut self, name: &str) -> Result<(Value, String), Refusal> {
        if name.trim().is_empty() {
            return Err(invalid("motion input path is empty"));
        }
        let path = self.base.join(name);
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(|e| io(name, e))?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| io(name, e))?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("motion input size overflow"))?;
        if bytes.len() > MAX_BYTES || self.bytes > MAX_INPUT_BYTES {
            return Err(invalid(
                "motion inputs exceed 64 MiB per file or 256 MiB total read budget",
            ));
        }
        let hash = sha256_hex(&bytes);
        let value = serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{name}: {e}")))?;
        Ok((value, hash))
    }
}

type DocumentLoader<'a> = &'a mut dyn FnMut(&str) -> Result<(Value, String), Refusal>;

fn load_frame(
    load: DocumentLoader<'_>,
    report: &str,
    map: &str,
) -> Result<(Frame, usize, Value), Refusal> {
    let (report, report_hash) = load(report)?;
    let (map, map_hash) = load(map)?;
    let (frame, source_work) = thermal_coverage::motion_frame(&report, &map)?;
    let receipt = json!({"drape_report_sha256":report_hash,"mapping_file_sha256":map_hash,
        "drape_thermal_frame_sha256":report["thermal_surface"]["sha256"],
        "frame_sha256":frame.sha256()});
    Ok((frame, source_work, receipt))
}

fn seal(mut value: Value) -> Result<Value, Refusal> {
    value["sha256"] = json!(sha256_hex(
        &serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))?
    ));
    Ok(value)
}
fn verify_seal(value: &Value) -> Result<(), Refusal> {
    let mut input = value.clone();
    input
        .as_object_mut()
        .ok_or_else(|| invalid("motion receipt must be an object"))?
        .remove("sha256");
    if seal(input)? != *value {
        return Err(invalid("motion receipt digest mismatch"));
    }
    Ok(())
}
fn time(t: f64) -> Result<(), Refusal> {
    if !t.is_finite() || t < 0. {
        return Err(invalid("motion time must be finite nonnegative seconds"));
    }
    Ok(())
}
fn restore(value: &Value) -> Result<(Frame, State, f64), Refusal> {
    // A complete earlier command result can be passed directly; no extraction command is needed.
    let checkpoint = if value["schema"] == "ftx-thermal-motion-report/1" {
        verify_seal(value)?;
        &value["checkpoint"]
    } else {
        value
    };
    verify_seal(checkpoint)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Saved {
        schema: String,
        time_seconds: f64,
        state: Value,
        sha256: String,
    }
    let saved: Saved =
        serde_json::from_value(checkpoint.clone()).map_err(|e| invalid(e.to_string()))?;
    if saved.schema != "ftx-thermal-motion-checkpoint/1" || saved.sha256.len() != 64 {
        return Err(invalid("unknown thermal motion checkpoint schema"));
    }
    time(saved.time_seconds)?;
    let (frame, state) = Frame::from_checkpoint(&saved.state)?;
    Ok((frame, state, saved.time_seconds))
}

fn exterior_environments(
    input: &[Exterior],
    boundaries: &[Boundary],
) -> Result<Vec<Exterior>, Refusal> {
    let environments = if input.len() == 1 {
        vec![input[0].clone(); boundaries.len()]
    } else if input.len() == boundaries.len() {
        input.to_vec()
    } else {
        return Err(invalid(
            "motion needs one exterior environment or one per matching body/source triangle",
        ));
    };
    for (b, e) in boundaries.iter().zip(&environments) {
        if b.outside_resistance != 0. || b.outside_c != e.air_c {
            return Err(invalid(
                "motion exterior exchange requires zero outside film and outside_c equal to air_c",
            ));
        }
    }
    Ok(environments)
}

fn resolve_microclimate_environments(
    input: &[MicroclimateExterior],
    boundaries: &[Boundary],
) -> Result<(Vec<Exterior>, Value), Refusal> {
    if input.len() != 1 && input.len() != boundaries.len() {
        return Err(invalid(
            "motion needs one exterior environment or one per matching body/source triangle",
        ));
    }
    let resolved = input
        .iter()
        .map(MicroclimateExterior::resolve)
        .collect::<Result<Vec<_>, _>>()?;
    let raw: Vec<Exterior> = resolved.iter().map(|r| r.exterior.clone()).collect();
    let receipts: Vec<Value> = resolved.iter().map(|r| r.receipt.clone()).collect();
    let environments = exterior_environments(&raw, boundaries)?;
    Ok((environments, json!(receipts)))
}

#[derive(Default, Serialize)]
struct ExteriorEnergy {
    convective_energy_j: f64,
    radiative_energy_j: f64,
}
impl ExteriorEnergy {
    fn add(&mut self, seconds: f64, convective_w: f64, radiative_w: f64) {
        self.convective_energy_j += seconds * convective_w;
        self.radiative_energy_j += seconds * radiative_w;
    }
}

#[derive(Default, Serialize)]
struct Totals {
    incoming_energy_j: f64,
    outgoing_energy_j: f64,
    net_boundary_energy_j: f64,
    physical_stored_change_j: f64,
    signed_remap_residual_j: f64,
    absolute_remap_residual_j: f64,
    largest_step_balance_error_j: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    exterior_exchange: Option<ExteriorEnergy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exposed_exterior_exchange: Option<ExteriorEnergy>,
}
impl Totals {
    fn check(&self) -> Result<(), Refusal> {
        if [
            self.incoming_energy_j,
            self.outgoing_energy_j,
            self.net_boundary_energy_j,
            self.physical_stored_change_j,
            self.signed_remap_residual_j,
            self.absolute_remap_residual_j,
            self.largest_step_balance_error_j,
            self.physical_stored_change_j - self.net_boundary_energy_j,
            self.physical_stored_change_j + self.signed_remap_residual_j,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(invalid("motion energy accumulation overflow"));
        }
        for e in [
            self.exterior_exchange.as_ref(),
            self.exposed_exterior_exchange.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !e.convective_energy_j.is_finite()
                || !e.radiative_energy_j.is_finite()
                || !(e.convective_energy_j + e.radiative_energy_j).is_finite()
            {
                return Err(invalid("motion exterior energy accumulation overflow"));
            }
        }
        Ok(())
    }
}

pub(super) fn run(path: &str) -> Result<Value, Refusal> {
    let input: Input = read(path)?;
    let mut inputs = Inputs {
        base: Path::new(path).parent().unwrap_or(Path::new(".")).into(),
        bytes: 0,
    };
    evaluate_with_loader(input, &mut |name| inputs.load(name))
}

fn evaluate_with_loader(input: Input, load: DocumentLoader<'_>) -> Result<Value, Refusal> {
    if input.schema != "ftx-thermal-motion/1" || !(1..=128).contains(&input.frames.len()) {
        return Err(invalid(
            "thermal motion needs its schema and 1..128 timed destination frames",
        ));
    }
    // Refuse malformed schedules before loading the referenced geometry. Checkpoint time is checked
    // again after restoration; every interval is positive and its substep remains representable.
    let mut last_time = match &input.initial {
        Initial::Uniform { time_seconds, .. } => {
            time(*time_seconds)?;
            Some(*time_seconds)
        }
        Initial::Resume { .. } => None,
    };
    let mut steps = 0usize;
    for f in &input.frames {
        time(f.time_seconds)?;
        steps = steps
            .checked_add(f.steps)
            .ok_or_else(|| invalid("motion steps overflow"))?;
        if f.steps == 0
            || steps > 100_000
            || f.boundaries.is_empty()
            || f.boundaries.len() > 65_536
            || f.exposed_boundaries.len() > 65_536
            || f.exterior_exchange
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || f.microclimate_exterior
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || f.bare_boundaries
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || f.exposed_exterior_exchange
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || f.exposed_microclimate_exterior
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || last_time.is_some_and(|t| f.time_seconds <= t)
        {
            return Err(invalid(
                "motion times must increase, with positive steps and at most 100000 total steps",
            ));
        }
        if f.exterior_exchange.is_some() && f.microclimate_exterior.is_some() {
            return Err(invalid(
                "supply either exterior_exchange or microclimate_exterior, not both",
            ));
        }
        if f.exposed_exterior_exchange.is_some() && f.exposed_microclimate_exterior.is_some() {
            return Err(invalid(
                "supply either exposed_exterior_exchange or exposed_microclimate_exterior, not both",
            ));
        }
        let has_covered_exterior =
            f.exterior_exchange.is_some() || f.microclimate_exterior.is_some();
        if has_covered_exterior != f.bare_boundaries.is_some() {
            return Err(invalid(
                "covered exterior exchange requires explicit bare_boundaries; omit both for film-only intervals",
            ));
        }
        let has_exposed_exterior =
            f.exposed_exterior_exchange.is_some() || f.exposed_microclimate_exterior.is_some();
        if has_exposed_exterior && f.exposed_boundaries.is_empty() {
            return Err(invalid(
                "exposed exterior exchange requires explicit exposed_boundaries",
            ));
        }
        last_time = Some(f.time_seconds);
    }
    let (mut frame, mut state, start, initial_receipt) = match &input.initial {
        Initial::Uniform {
            time_seconds,
            drape_report,
            coverage,
            temperature_c,
        } => {
            let (f, _, receipt) = load_frame(load, drape_report, coverage)?;
            let state = f.initial(*temperature_c)?;
            (f, state, *time_seconds, receipt)
        }
        Initial::Resume { checkpoint } => {
            let (value, hash) = load(checkpoint)?;
            let (f, s, t) = restore(&value)?;
            (f, s, t, json!({"checkpoint_file_sha256":hash}))
        }
    };
    let mut previous = start;
    let mut work = 0usize;
    let mut geometric_work = frame.projection().geometry_operations;
    let mut totals = Totals::default();
    let mut trace = Vec::new();
    let mut trace_bytes = 0usize;
    let mut final_field = Value::Null;
    let mut any_microclimate = false;
    for (i, f) in input.frames.iter().enumerate() {
        let dt = (f.time_seconds - previous) / f.steps as f64;
        if f.time_seconds <= previous || !dt.is_finite() || dt <= 0. {
            return Err(invalid(
                "motion interval or thermal substep is not finite and positive",
            ));
        }
        let (next, source_work, receipt) = load_frame(load, &f.drape_report, &f.coverage)?;
        let unit = next
            .cells()
            .checked_add(next.body_triangle_count())
            .and_then(|n| n.checked_add(source_work));
        work = unit
            .and_then(|n| n.checked_mul(f.steps))
            .and_then(|n| work.checked_add(n))
            .filter(|n| *n <= 10_000_000)
            .ok_or_else(|| invalid("motion exceeds ten million cell/geometry steps"))?;
        geometric_work = geometric_work
            .checked_add(next.projection().geometry_operations)
            .filter(|n| *n <= 20_000_000)
            .ok_or_else(|| invalid("motion exceeds twenty million geometry/remap operations"))?;
        let boundaries: Vec<Boundary> = if f.boundaries.len() == 1 {
            vec![f.boundaries[0].boundary(); next.body_triangle_count()]
        } else if f.boundaries.len() == next.body_triangle_count() {
            f.boundaries.iter().map(Film::boundary).collect()
        } else {
            return Err(invalid(
                "motion needs one boundary or one per destination body triangle",
            ));
        };
        let bare_boundaries = f
            .bare_boundaries
            .as_ref()
            .map(|films| {
                if films.len() == 1 {
                    Ok(vec![films[0].boundary(); next.body_triangle_count()])
                } else if films.len() == next.body_triangle_count() {
                    Ok(films.iter().map(Film::boundary).collect::<Vec<_>>())
                } else {
                    Err(invalid(
                        "motion needs one bare boundary or one per destination body triangle",
                    ))
                }
            })
            .transpose()?;
        let (environments, covered_microclimate) = if let Some(e) = f.exterior_exchange.as_deref() {
            (Some(exterior_environments(e, &boundaries)?), None)
        } else if let Some(mc) = f.microclimate_exterior.as_deref() {
            let (env, receipts) = resolve_microclimate_environments(mc, &boundaries)?;
            any_microclimate = true;
            (Some(env), Some(receipts))
        } else {
            (None, None)
        };
        let covered_exterior = environments.as_deref().map(|e| ExteriorConditions {
            environments: e,
            bare_boundaries: bare_boundaries
                .as_deref()
                .expect("validated bare conditions"),
        });
        let has_exposed_exterior =
            f.exposed_exterior_exchange.is_some() || f.exposed_microclimate_exterior.is_some();
        let exposed_boundaries: Option<Vec<Boundary>> = if f.exposed_boundaries.is_empty() {
            None
        } else if !next.has_exposed_cloth() && !has_exposed_exterior {
            return Err(invalid(
                "exposed boundaries were supplied but the destination frame has no uncovered cloth",
            ));
        } else if f.exposed_boundaries.len() == 1 {
            Some(vec![
                f.exposed_boundaries[0].boundary();
                next.cloth_triangle_count()
            ])
        } else if f.exposed_boundaries.len() == next.cloth_triangle_count() {
            Some(f.exposed_boundaries.iter().map(Film::boundary).collect())
        } else {
            return Err(invalid(
                "motion needs one exposed boundary or one per source cloth triangle",
            ));
        };
        let (exposed_environments, exposed_microclimate) =
            if let Some(e) = f.exposed_exterior_exchange.as_deref() {
                (
                    Some(exterior_environments(
                        e,
                        exposed_boundaries
                            .as_deref()
                            .expect("validated exposed conditions"),
                    )?),
                    None,
                )
            } else if let Some(mc) = f.exposed_microclimate_exterior.as_deref() {
                let (env, receipts) = resolve_microclimate_environments(
                    mc,
                    exposed_boundaries
                        .as_deref()
                        .expect("validated exposed conditions"),
                )?;
                any_microclimate = true;
                (Some(env), Some(receipts))
            } else {
                (None, None)
            };
        let exposed_conditions = exposed_boundaries.as_deref().map(|b| ExposedConditions {
            boundaries: b,
            environments: exposed_environments.as_deref(),
        });
        let remap = if frame.sha256() == next.sha256() {
            None
        } else {
            let transfer = next.remap(&frame, &state)?;
            geometric_work = geometric_work
                .checked_add(transfer.balance.operations)
                .filter(|n| *n <= 20_000_000)
                .ok_or_else(|| {
                    invalid("motion exceeds twenty million geometry/remap operations")
                })?;
            totals.signed_remap_residual_j += transfer.balance.residual_j;
            totals.absolute_remap_residual_j += transfer.balance.absolute_residual_j;
            state = transfer.state;
            Some(transfer.balance)
        };
        frame = next;
        let mut interval_stored = 0.;
        let mut interval_net = 0.;
        let mut covered_energy = ExteriorEnergy::default();
        let mut exposed_energy = ExteriorEnergy::default();
        for j in 0..f.steps {
            let step = frame.advance_with_exchange(
                &state,
                dt,
                &boundaries,
                covered_exterior,
                exposed_conditions,
            )?;
            let heat = step.heat();
            totals.incoming_energy_j += dt * step.inside_heat_w();
            totals.outgoing_energy_j += dt * step.outside_heat_w();
            if let Some(e) = &heat.exterior_exchange {
                totals.exterior_exchange.get_or_insert_default().add(
                    dt,
                    e.convective_heat_w,
                    e.radiative_heat_w,
                );
                covered_energy.add(dt, e.convective_heat_w, e.radiative_heat_w);
            }
            if let Some(e) = step.exposed_exterior_exchange() {
                totals
                    .exposed_exterior_exchange
                    .get_or_insert_default()
                    .add(dt, e.convective_heat_w, e.radiative_heat_w);
                exposed_energy.add(dt, e.convective_heat_w, e.radiative_heat_w);
            }
            // Accumulate storage heat before adding the cancelling bare-body path to each total.
            totals.net_boundary_energy_j += dt * step.net_heat_w();
            totals.physical_stored_change_j += step.stored_change_j();
            totals.largest_step_balance_error_j = totals
                .largest_step_balance_error_j
                .max(step.balance_error_j().abs());
            interval_stored += step.stored_change_j();
            interval_net += dt * step.net_heat_w();
            totals.check()?;
            if [
                interval_stored,
                interval_net,
                covered_energy.convective_energy_j,
                covered_energy.radiative_energy_j,
                exposed_energy.convective_energy_j,
                exposed_energy.radiative_energy_j,
            ]
            .iter()
            .any(|v| !v.is_finite())
            {
                return Err(invalid("motion interval energy overflow"));
            }
            if j + 1 == f.steps {
                let mut entry = json!({"from_time_seconds":previous,"time_seconds":f.time_seconds,
                    "step_seconds":dt,"steps":f.steps,"inputs":receipt,"remap":remap,
                    "geometry_operations":frame.projection().geometry_operations,
                    "partition_roundoff_m2":frame.projection().partition_roundoff_m2,
                    "unresolved_footprint_area_m2":frame.projection().unresolved_footprint_area_m2,
                    "covered_area_m2":heat.covered_area_m2,"bare_heat_w":heat.bare_heat_w,
                    "inside_heat_w":step.inside_heat_w(),"outside_heat_w":step.outside_heat_w(),
                    "covered_inside_heat_w":heat.inside_heat_w,"covered_outside_heat_w":heat.outside_heat_w,
                    "exposed_inside_heat_w":step.exposed_inside_heat_w(),
                    "exposed_outside_heat_w":step.exposed_outside_heat_w(),
                    "net_heat_w":step.net_heat_w(),"physical_stored_change_j":interval_stored,
                    "net_boundary_energy_j":interval_net,"heat_balance_error_j":interval_stored-interval_net});
                if let Some(e) = &heat.exterior_exchange {
                    entry["exterior_exchange"] = json!({"convective_heat_w":e.convective_heat_w,
                        "radiative_heat_w":e.radiative_heat_w,"balance_residual_w":e.balance_residual_w,
                        "convective_energy_j":covered_energy.convective_energy_j,
                        "radiative_energy_j":covered_energy.radiative_energy_j});
                }
                if let Some(e) = step.exposed_exterior_exchange() {
                    entry["exposed_exterior_exchange"] = json!({"convective_heat_w":e.convective_heat_w,
                        "radiative_heat_w":e.radiative_heat_w,"balance_residual_w":e.balance_residual_w,
                        "convective_energy_j":exposed_energy.convective_energy_j,
                        "radiative_energy_j":exposed_energy.radiative_energy_j});
                }
                if let Some(receipts) = &covered_microclimate {
                    entry["microclimate_convection"] = receipts.clone();
                }
                if let Some(receipts) = &exposed_microclimate {
                    entry["exposed_microclimate_convection"] = receipts.clone();
                }
                trace_bytes = trace_bytes
                    .checked_add(
                        serde_json::to_vec(&entry)
                            .map_err(|e| invalid(e.to_string()))?
                            .len(),
                    )
                    .filter(|n| *n <= MAX_BYTES / 4)
                    .ok_or_else(|| invalid("motion trace exceeds its output budget"))?;
                trace.push(entry);
                if i + 1 == input.frames.len() {
                    final_field = thermal_coverage::field(heat);
                    if let Some(e) = &heat.exterior_exchange {
                        final_field["exterior_exchange"] = json!(e);
                    }
                    if let Some(e) = step.exposed_exterior_exchange() {
                        final_field["exposed_exterior_exchange"] = json!(e);
                    }
                    final_field["exposed_temperature_c"] = json!(step.exposed_temperature_c());
                    final_field["exposed_inside_heat_w"] = json!(step.exposed_inside_heat_w());
                    final_field["exposed_outside_heat_w"] = json!(step.exposed_outside_heat_w());
                    final_field["total_inside_heat_w"] = json!(step.inside_heat_w());
                    final_field["total_outside_heat_w"] = json!(step.outside_heat_w());
                    final_field["total_stored_change_j"] = json!(step.stored_change_j());
                }
            }
            state = step.into_state();
        }
        previous = f.time_seconds;
    }
    totals.check()?;
    let checkpoint = seal(json!({"schema":"ftx-thermal-motion-checkpoint/1",
        "time_seconds":previous,"state":frame.checkpoint(&state)?}))?;
    let mut body = json!({"schema":"ftx-thermal-motion-report/1","input":input,"initial_inputs":initial_receipt,
        "from_time_seconds":start,"time_seconds":previous,"trace":trace,"final":final_field,
        "energy":totals,"heat_balance_error_j":totals.physical_stored_change_j-totals.net_boundary_energy_j,
        "material_energy_change_including_remap_j":totals.physical_stored_change_j+totals.signed_remap_residual_j,
        "checkpoint":checkpoint,"cell_geometry_steps":work,"geometry_remap_operations":geometric_work,
        "scope":"prescribed right-endpoint poses and boundary fields; conservative covered/exposed material remap, then backward-Euler heat substeps",
        "energy_scope":"this invocation only; remap error is numerical, not boundary heat",
        "exposed_boundary_policy":"uncovered cloth is adiabatic unless exposed_boundaries are explicitly supplied for an interval",
        "not_simulated":["interpolated mechanical motion","in-plane gradients within one exposed source-triangle fraction",
            "cloth remeshing","moisture","compression","air-gap transport","human thermoregulation"],
        "comfort_claim":false,"empirical_validation_claim":false});
    if any_microclimate {
        body["not_simulated"].as_array_mut().unwrap().extend([
            json!("empirical garment-film / wind-tunnel validation"),
            json!("interlayer or finite-view-factor radiation"),
            json!("solar/spectral irradiation"),
        ]);
        body["exterior_model"] = json!(
            "validity-gated moist-air Nusselt correlation convection (Evidence::Estimate; model-form discrepancy allowance retained); optional source-qualified opaque-gray large-isothermal-enclosure radiation; not a measured garment film"
        );
    }
    let out = seal(body)?;
    if serde_json::to_vec_pretty(&out)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        >= MAX_BYTES
    {
        return Err(invalid(
            "motion report exceeds 64 MiB restart limit; reduce mesh or split the sequence",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::Vec3;
    use ftx_drape::thermal::{
        DryThermalModel,
        surface::{
            Patch, Surface,
            projection::{self, Request as ProjectionRequest, Settings},
        },
    };
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};

    fn two_triangle_report_and_mapping() -> (Value, Value) {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../tests/fixtures/materials/synthetic-thermal.json"
        ))
        .unwrap();
        let catalog = Catalog::builtin().unwrap();
        let layers = fixture["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let selection: Selection = serde_json::from_value(v.clone()).unwrap();
                catalog.resolve(&selection).unwrap()
            })
            .collect();
        let column = DryThermalModel::new(
            layers,
            serde_json::from_value(fixture["interface_resistance_m2_k_w"].clone()).unwrap(),
            2,
        )
        .unwrap();
        let patches = vec![
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [1, 3, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
        ];
        // Cloth covers x in [0, 1] at z = 0.005; body covers x in [0, 0.5] at z = 0.0,
        // leaving an exact exposed cloth overhang for exposed_microclimate_exterior tests.
        let cloth_positions = vec![
            Vec3::new(0., 0., 0.005),
            Vec3::new(1., 0., 0.005),
            Vec3::new(0., 1., 0.005),
            Vec3::new(1., 1., 0.005),
        ];
        let body_positions = vec![
            Vec3::new(0., 0., 0.),
            Vec3::new(0.5, 0., 0.),
            Vec3::new(0., 1., 0.),
            Vec3::new(0.5, 1., 0.),
        ];
        let body_triangles = [[0, 1, 2], [1, 3, 2]];
        let surface = Surface::new(vec![column], cloth_positions.len(), patches).unwrap();
        let material = json!({"basis": "synthetic CLI motion test", "layers": fixture["layers"]});
        let mut frame = json!({
            "schema": "ftx-drape-thermal-surface/1",
            "available": true,
            "model": surface.snapshot(),
            "material_report_sha256": sha256_hex(&serde_json::to_vec(&material).unwrap()),
            "positions_m": cloth_positions,
            "triangle_instance": [0, 0],
            "instances": ["synthetic"]
        });
        frame["sha256"] = json!(sha256_hex(&serde_json::to_vec(&frame).unwrap()));
        let report = json!({
            "thermal_surface": frame,
            "material": material
        });
        let settings = Settings {
            max_distance_m: 0.05,
            min_normal_cosine: 0.5,
            separation_tolerance_m: 1e-4,
            cells_per_layer: 2,
            interfaces: Vec::new(),
        };
        let proj = projection::project(
            &surface,
            &cloth_positions,
            &body_positions,
            &body_triangles,
            &ProjectionRequest {
                schema: "ftx-thermal-projection/1".into(),
                cloth_layers: vec!["shell".into(), "shell".into()],
                settings: settings.clone(),
            },
        )
        .unwrap();
        let mapping = json!({
            "schema": "ftx-drape-thermal-coverage/1",
            "drape_thermal_frame_sha256": report["thermal_surface"]["sha256"],
            "body_positions_m": body_positions,
            "layout": proj.layout,
            "projection": {
                "request": {
                    "schema": "ftx-drape-thermal-projection/1",
                    "instance_layers": {"synthetic": "shell"},
                    "settings": settings
                },
                "body_obj_sha256": sha256_hex(b"synthetic-motion-body-obj"),
                "result": proj
            }
        });
        (report, mapping)
    }

    #[test]
    fn microclimate_exterior_drives_covered_and_exposed_motion_with_exact_parity_and_guards() {
        let (report, mapping) = two_triangle_report_and_mapping();
        let report_hash = sha256_hex(&serde_json::to_vec(&report).unwrap());
        let map_hash = sha256_hex(&serde_json::to_vec(&mapping).unwrap());
        let run_in_memory = |raw: Value| -> Result<Value, Refusal> {
            let input: Input = serde_json::from_value(raw).map_err(|e| invalid(e.to_string()))?;
            evaluate_with_loader(input, &mut |name| match name {
                "drape.json" => Ok((report.clone(), report_hash.clone())),
                "map.json" => Ok((mapping.clone(), map_hash.clone())),
                other => Err(invalid(format!("unexpected fixture path {other}"))),
            })
        };

        let forced_mc = json!({
            "air": {"temperature_c": 18.0, "pressure_pa": 101_325.0, "relative_humidity": 0.5},
            "correlation": "churchill_bernstein_cylinder",
            "characteristic_length_m": 0.25,
            "wind_speed_m_s": 2.0,
            "surface_temperature_range_c": [-50.0, 100.0],
            "allow_estimates": true
        });
        let natural_mc = json!({
            "air": {"temperature_c": 18.0, "pressure_pa": 101_325.0, "relative_humidity": 0.5},
            "correlation": "churchill_chu_vertical_plate",
            "characteristic_length_m": 0.30,
            "delta_t_k": 10.0,
            "surface_temperature_range_c": [-50.0, 100.0],
            "allow_estimates": true
        });
        let resolved_forced = serde_json::from_value::<MicroclimateExterior>(forced_mc.clone())
            .unwrap()
            .resolve()
            .unwrap();
        let resolved_natural = serde_json::from_value::<MicroclimateExterior>(natural_mc.clone())
            .unwrap()
            .resolve()
            .unwrap();

        let mc_req = json!({
            "schema": "ftx-thermal-motion/1",
            "initial": {
                "kind": "uniform",
                "time_seconds": 0.0,
                "drape_report": "drape.json",
                "coverage": "map.json",
                "temperature_c": 26.0
            },
            "frames": [{
                "time_seconds": 10.0,
                "drape_report": "drape.json",
                "coverage": "map.json",
                "boundaries": [{
                    "inside_c": 34.0,
                    "outside_c": 18.0,
                    "inside_resistance_m2_k_w": 0.05,
                    "outside_resistance_m2_k_w": 0.0
                }],
                "microclimate_exterior": [forced_mc.clone()],
                "bare_boundaries": [{
                    "inside_c": 34.0,
                    "outside_c": 18.0,
                    "inside_resistance_m2_k_w": 0.05,
                    "outside_resistance_m2_k_w": 0.10
                }],
                "exposed_boundaries": [{
                    "inside_c": 18.0,
                    "outside_c": 18.0,
                    "inside_resistance_m2_k_w": 0.10,
                    "outside_resistance_m2_k_w": 0.0
                }],
                "exposed_microclimate_exterior": [forced_mc.clone(), natural_mc.clone()],
                "steps": 4
            }]
        });

        let mc_out = run_in_memory(mc_req.clone()).unwrap();
        verify_seal(&mc_out).unwrap();
        assert_eq!(
            mc_out["trace"][0]["microclimate_convection"][0]["correlation"],
            "churchill_bernstein_cylinder"
        );
        assert_eq!(
            mc_out["trace"][0]["exposed_microclimate_convection"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            mc_out["trace"][0]["exposed_microclimate_convection"][1]["correlation"],
            "churchill_chu_vertical_plate"
        );
        assert!(
            mc_out["exterior_model"]
                .as_str()
                .unwrap()
                .contains("Nusselt")
        );
        assert!(
            mc_out["not_simulated"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "empirical garment-film / wind-tunnel validation")
        );
        assert!(mc_out["heat_balance_error_j"].as_f64().unwrap().abs() < 1e-11);

        // Exact numerical parity against manually pre-resolved Exterior environments.
        let mut ext_req = mc_req.clone();
        let f0 = ext_req["frames"][0].as_object_mut().unwrap();
        f0.remove("microclimate_exterior");
        f0.remove("exposed_microclimate_exterior");
        f0.insert(
            "exterior_exchange".into(),
            json!([resolved_forced.exterior]),
        );
        f0.insert(
            "exposed_exterior_exchange".into(),
            json!([resolved_forced.exterior, resolved_natural.exterior]),
        );
        let ext_out = run_in_memory(ext_req).unwrap();
        assert!(ext_out.get("exterior_model").is_none());
        assert!(ext_out["trace"][0].get("microclimate_convection").is_none());
        assert!(
            ext_out["trace"][0]
                .get("exposed_microclimate_convection")
                .is_none()
        );
        assert_eq!(mc_out["energy"], ext_out["energy"]);
        assert_eq!(mc_out["final"], ext_out["final"]);

        // Resuming from a microclimate report checkpoint verifies its seal and advances cleanly.
        let mut resume_req = json!({
            "schema": "ftx-thermal-motion/1",
            "initial": {
                "kind": "resume",
                "checkpoint": "prev.json"
            },
            "frames": [mc_req["frames"][0].clone()]
        });
        resume_req["frames"][0]["time_seconds"] = json!(20.0);
        let resume_input: Input = serde_json::from_value(resume_req).unwrap();
        let prev_hash = sha256_hex(&serde_json::to_vec(&mc_out).unwrap());
        let resumed = evaluate_with_loader(resume_input, &mut |name| match name {
            "prev.json" => Ok((mc_out.clone(), prev_hash.clone())),
            "drape.json" => Ok((report.clone(), report_hash.clone())),
            "map.json" => Ok((mapping.clone(), map_hash.clone())),
            other => Err(invalid(format!("unexpected fixture path {other}"))),
        })
        .unwrap();
        verify_seal(&resumed).unwrap();
        assert_eq!(resumed["from_time_seconds"], 10.0);
        assert_eq!(resumed["time_seconds"], 20.0);
        assert!(resumed["heat_balance_error_j"].as_f64().unwrap().abs() < 1e-11);

        // Fail-closed guards:
        // 1. Denied estimate consent refuses.
        let mut denied = mc_req.clone();
        denied["frames"][0]["microclimate_exterior"][0]["allow_estimates"] = json!(false);
        assert!(run_in_memory(denied).is_err());

        // 2. Combining exterior_exchange and microclimate_exterior refuses.
        let mut both_cov = mc_req.clone();
        both_cov["frames"][0]["exterior_exchange"] = json!([resolved_forced.exterior]);
        assert!(
            run_in_memory(both_cov)
                .unwrap_err()
                .message
                .contains("not both")
        );

        // 3. Combining exposed_exterior_exchange and exposed_microclimate_exterior refuses.
        let mut both_exp = mc_req.clone();
        both_exp["frames"][0]["exposed_exterior_exchange"] = json!([resolved_natural.exterior]);
        assert!(
            run_in_memory(both_exp)
                .unwrap_err()
                .message
                .contains("not both")
        );

        // 4. Missing bare_boundaries with microclimate_exterior refuses.
        let mut no_bare = mc_req.clone();
        no_bare["frames"][0]
            .as_object_mut()
            .unwrap()
            .remove("bare_boundaries");
        assert!(
            run_in_memory(no_bare)
                .unwrap_err()
                .message
                .contains("bare_boundaries")
        );

        // 5. Missing exposed_boundaries with exposed_microclimate_exterior refuses.
        let mut no_exp_b = mc_req.clone();
        no_exp_b["frames"][0]["exposed_boundaries"] = json!([]);
        assert!(
            run_in_memory(no_exp_b)
                .unwrap_err()
                .message
                .contains("exposed_boundaries")
        );

        // 6. Wrong per-triangle count refuses.
        let mut wrong_count = mc_req.clone();
        wrong_count["frames"][0]["microclimate_exterior"] =
            json!([forced_mc.clone(), forced_mc.clone(), forced_mc]);
        assert!(run_in_memory(wrong_count).is_err());

        // 7. Mismatched air temperature vs boundary outside_c refuses.
        let mut bad_air = mc_req;
        bad_air["frames"][0]["microclimate_exterior"][0]["air"]["temperature_c"] = json!(22.0);
        assert!(
            run_in_memory(bad_air)
                .unwrap_err()
                .message
                .contains("outside_c equal to air_c")
        );
    }
}

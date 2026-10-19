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

    fn frame(&mut self, report: &str, map: &str) -> Result<(Frame, usize, Value), Refusal> {
        let (report, report_hash) = self.load(report)?;
        let (map, map_hash) = self.load(map)?;
        let (frame, source_work) = thermal_coverage::motion_frame(&report, &map)?;
        let receipt = json!({"drape_report_sha256":report_hash,"mapping_file_sha256":map_hash,
            "drape_thermal_frame_sha256":report["thermal_surface"]["sha256"],
            "frame_sha256":frame.sha256()});
        Ok((frame, source_work, receipt))
    }
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
            || f.bare_boundaries
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || f.exposed_exterior_exchange
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 65_536)
            || last_time.is_some_and(|t| f.time_seconds <= t)
        {
            return Err(invalid(
                "motion times must increase, with positive steps and at most 100000 total steps",
            ));
        }
        if f.exterior_exchange.is_some() != f.bare_boundaries.is_some() {
            return Err(invalid(
                "covered exterior exchange requires explicit bare_boundaries; omit both for film-only intervals",
            ));
        }
        if f.exposed_exterior_exchange.is_some() && f.exposed_boundaries.is_empty() {
            return Err(invalid(
                "exposed exterior exchange requires explicit exposed_boundaries",
            ));
        }
        last_time = Some(f.time_seconds);
    }
    let mut inputs = Inputs {
        base: Path::new(path).parent().unwrap_or(Path::new(".")).into(),
        bytes: 0,
    };
    let (mut frame, mut state, start, initial_receipt) = match &input.initial {
        Initial::Uniform {
            time_seconds,
            drape_report,
            coverage,
            temperature_c,
        } => {
            let (f, _, receipt) = inputs.frame(drape_report, coverage)?;
            let state = f.initial(*temperature_c)?;
            (f, state, *time_seconds, receipt)
        }
        Initial::Resume { checkpoint } => {
            let (value, hash) = inputs.load(checkpoint)?;
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
    for (i, f) in input.frames.iter().enumerate() {
        let dt = (f.time_seconds - previous) / f.steps as f64;
        if f.time_seconds <= previous || !dt.is_finite() || dt <= 0. {
            return Err(invalid(
                "motion interval or thermal substep is not finite and positive",
            ));
        }
        let (next, source_work, receipt) = inputs.frame(&f.drape_report, &f.coverage)?;
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
        let environments = f
            .exterior_exchange
            .as_deref()
            .map(|e| exterior_environments(e, &boundaries))
            .transpose()?;
        let covered_exterior = environments.as_deref().map(|e| ExteriorConditions {
            environments: e,
            bare_boundaries: bare_boundaries
                .as_deref()
                .expect("validated bare conditions"),
        });
        let exposed_boundaries: Option<Vec<Boundary>> = if f.exposed_boundaries.is_empty() {
            None
        } else if !next.has_exposed_cloth() && f.exposed_exterior_exchange.is_none() {
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
        let exposed_environments = f
            .exposed_exterior_exchange
            .as_deref()
            .map(|e| {
                exterior_environments(
                    e,
                    exposed_boundaries
                        .as_deref()
                        .expect("validated exposed conditions"),
                )
            })
            .transpose()?;
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
    let out = seal(
        json!({"schema":"ftx-thermal-motion-report/1","input":input,"initial_inputs":initial_receipt,
        "from_time_seconds":start,"time_seconds":previous,"trace":trace,"final":final_field,
        "energy":totals,"heat_balance_error_j":totals.physical_stored_change_j-totals.net_boundary_energy_j,
        "material_energy_change_including_remap_j":totals.physical_stored_change_j+totals.signed_remap_residual_j,
        "checkpoint":checkpoint,"cell_geometry_steps":work,"geometry_remap_operations":geometric_work,
        "scope":"prescribed right-endpoint poses and boundary fields; conservative covered/exposed material remap, then backward-Euler heat substeps",
        "energy_scope":"this invocation only; remap error is numerical, not boundary heat",
        "exposed_boundary_policy":"uncovered cloth is adiabatic unless exposed_boundaries are explicitly supplied for an interval",
        "not_simulated":["interpolated mechanical motion","in-plane gradients within one exposed source-triangle fraction",
            "cloth remeshing","moisture","compression","air-gap transport","human thermoregulation"],
        "comfort_claim":false,"empirical_validation_claim":false}),
    )?;
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

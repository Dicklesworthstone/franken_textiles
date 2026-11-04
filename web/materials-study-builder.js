// Guided request authoring only. Rust still owns material admission, planning and comparison.
export const MAX_GUIDED_STUDY_CANDIDATES = 7;
const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const own = (value, key) => Object.hasOwn(value, key);

// Presence enables guided criteria; the native consumer admits the complete source policy.
export function hasTextileWaterPolicy(surface) {
  return surface?.surface_water != null || surface?.surface_water_alternatives != null ||
    surface?.coupled_water_alternatives != null;
}

const METRIC_CONSUMERS = {
  stock_area_m2: "cutting", stock_mass_kg: "cutting", assembly_mass_kg: "assembly_mass",
  cutting_modeled_path_seconds: "cutting_program",
  used_stock_count: "finite_stock", used_stock_gross_area_m2: "finite_stock", used_stock_mass_kg: "finite_stock",
  finite_stock_modeled_path_seconds: "finite_stock_program",
  cutting_job_stock_area_m2: "cutting_job", cutting_job_stock_mass_kg: "cutting_job",
  cutting_job_modeled_path_seconds: "cutting_job_program", cutting_job_selected_order_cost: "cutting_job_purchasing",
  sewing_modeled_production_seconds: "sewing", sewing_parallel_makespan_seconds: "sewing",
  selected_observed_order_cost: "purchasing", thread_observed_order_cost: "sewing_purchasing",
  cut_and_sew_selected_order_cost: "cut_and_sew_purchasing", thread_required_length_m: "sewing_consumables",
  simulated_cloth_mass_kg: "drape", max_absolute_edge_strain_pct: "drape",
  independent_surface_power_w: "surface",
  min_warp_green_strain: "material_strain", max_warp_green_strain: "material_strain",
  min_weft_green_strain: "material_strain", max_weft_green_strain: "material_strain",
  min_engineering_shear_green_strain: "material_strain", max_engineering_shear_green_strain: "material_strain",
  motion_max_absolute_edge_strain_pct: "motion",
  motion_min_warp_green_strain: "motion_material_strain", motion_max_warp_green_strain: "motion_material_strain",
  motion_min_weft_green_strain: "motion_material_strain", motion_max_weft_green_strain: "motion_material_strain",
  motion_min_engineering_shear_green_strain: "motion_material_strain", motion_max_engineering_shear_green_strain: "motion_material_strain",
  body_contact_resultant_n: "contact_loads", max_body_contact_vertex_force_n: "contact_loads",
  motion_peak_body_contact_resultant_n: "motion_contact_loads", motion_max_body_contact_vertex_force_n: "motion_contact_loads",
  body_coverage_power_w: "coverage",
  independent_vapor_potential_heat_flow_w: "vapor_surface", independent_vapor_potential_mass_flow_kg_s: "vapor_surface",
  covered_vapor_potential_heat_flow_w: "vapor_coverage", covered_vapor_potential_mass_flow_kg_s: "vapor_coverage",
  motion_covered_vapor_potential_energy_j: "motion_vapor_coverage", motion_covered_vapor_potential_mass_kg: "motion_vapor_coverage",
  motion_wet_film_evaporated_mass_kg: "motion_wet_film", motion_wet_film_remaining_liquid_mass_kg: "motion_wet_film",
  motion_wet_film_unmet_demand_mass_kg: "motion_wet_film", motion_wet_film_supplied_mass_kg: "motion_wet_film",
  motion_wet_film_body_reservoir_latent_energy_j: "motion_wet_film",
  textile_water_evaporated_mass_kg: "transient_surface", textile_water_remaining_mass_kg: "transient_surface",
  textile_water_supplied_mass_kg: "transient_surface", textile_water_condensed_mass_kg: "transient_surface",
  textile_water_drained_mass_kg: "transient_surface", textile_water_latent_energy_j: "transient_surface",
  textile_water_condensation_latent_energy_j: "transient_surface",
  transient_surface_inside_energy_j: "transient_surface", transient_surface_net_boundary_energy_j: "transient_surface",
  transient_surface_min_sample_temperature_c: "transient_surface", transient_surface_max_sample_temperature_c: "transient_surface",
  transient_surface_final_min_temperature_c: "transient_surface", transient_surface_final_max_temperature_c: "transient_surface",
  transient_coverage_inside_energy_j: "transient_coverage", transient_coverage_net_boundary_energy_j: "transient_coverage",
  transient_coverage_min_sample_temperature_c: "transient_coverage", transient_coverage_max_sample_temperature_c: "transient_coverage",
  transient_coverage_final_min_temperature_c: "transient_coverage", transient_coverage_final_max_temperature_c: "transient_coverage",
};
const REPORTED_BOUND_METRICS = new Set([
  "assembly_mass_kg", "independent_surface_power_w", "body_coverage_power_w",
  "independent_vapor_potential_heat_flow_w", "independent_vapor_potential_mass_flow_kg_s",
  "covered_vapor_potential_heat_flow_w", "covered_vapor_potential_mass_flow_kg_s",
  "motion_covered_vapor_potential_energy_j", "motion_covered_vapor_potential_mass_kg",
  "motion_wet_film_evaporated_mass_kg", "motion_wet_film_remaining_liquid_mass_kg",
  "motion_wet_film_unmet_demand_mass_kg", "motion_wet_film_supplied_mass_kg", "motion_wet_film_body_reservoir_latent_energy_j",
]);

function object(value, allowed, required, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${label} must be an object.`);
  if (Object.keys(value).some(key => !allowed.includes(key)) || required.some(key => !own(value, key))) {
    throw Error(`${label} has missing or unsupported fields.`);
  }
  return value;
}
function name(value, label) {
  if (typeof value !== "string" || !value.trim()) throw Error(`${label} must be a nonempty name.`);
  return value;
}
function nonnegative(value, label) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) throw Error(`${label} must be finite and nonnegative.`);
}
function allowance(value, label) {
  if (own(value, "seam_allowance_mm") && value.seam_allowance_mm !== null) nonnegative(value.seam_allowance_mm, label);
  return own(value, "seam_allowance_mm") ? value.seam_allowance_mm : null;
}
function uniqueNames(values, label, maximum) {
  if (!Array.isArray(values) || !values.length || values.length > maximum) throw Error(`${label} needs 1–${maximum} entries.`);
  const seen = new Set();
  for (const value of values) {
    name(value, label);
    if (seen.has(value)) throw Error(`${label} must be unique.`);
    seen.add(value);
  }
  return seen;
}
function bytes(value, maximum = MAX_REQUEST_BYTES, message = "Guided study request exceeds 16 MiB.") {
  // Count UTF-8 without allocating another copy of a potentially repeated material snapshot.
  let count = 0;
  for (let i = 0; i < value.length; i++) {
    const c = value.charCodeAt(i);
    if (c < 0x80) count++;
    else if (c < 0x800) count += 2;
    else if (c >= 0xd800 && c <= 0xdbff && value.charCodeAt(i + 1) >= 0xdc00 && value.charCodeAt(i + 1) <= 0xdfff) {
      count += 4; i++;
    } else count += 3;
    if (count > maximum) throw Error(message);
  }
  return count;
}
function parse(raw, label) {
  if (typeof raw !== "string") throw Error(`${label} must retain its original JSON text.`);
  bytes(raw);
  try { return JSON.parse(raw); }
  catch { throw Error(`${label} must be valid JSON.`); }
}
function sourceObject(raw, label) {
  const value = parse(raw, label);
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${label} must be a JSON object.`);
  return value;
}

function frozenStudySelection(value) {
  const selection = object(value,
    ["id", "record", "overrides", "sources", "allow_estimates", "expected_sha256"],
    ["id", "record", "expected_sha256"], "Frozen material selection");
  name(selection.id, "Material ID");
  if (!selection.record || typeof selection.record !== "object" || Array.isArray(selection.record)
    || !own(selection.record, "id") || selection.record.id !== selection.id
    || typeof selection.expected_sha256 !== "string" || !/^[0-9a-f]{64}$/.test(selection.expected_sha256)
    || (own(selection, "allow_estimates") && typeof selection.allow_estimates !== "boolean")) {
    throw Error("Inspect the material first to retain its matching record, source lock and estimate policy.");
  }
  return selection;
}
function configurationChanges(raw, pieces) {
  const changes = object(parse(raw, "Saved material assignments"), ["schema", "default", "pieces", "interfaces"],
    ["schema"], "Saved material assignments");
  if (changes.schema !== "ftx-material-bindings/1") throw Error("Saved material assignments need ftx-material-bindings/1.");
  if (changes.default != null) frozenStudySelection(changes.default);
  let count = 0;
  if (own(changes, "pieces")) {
    if (!changes.pieces || typeof changes.pieces !== "object" || Array.isArray(changes.pieces)) {
      throw Error("Saved piece assignments must be an object of exact source-piece IDs.");
    }
    for (const [id, selection] of Object.entries(changes.pieces)) {
      if (!pieces.has(id)) throw Error("Saved material assignments name a missing source piece.");
      frozenStudySelection(selection); count++;
    }
  }
  if (changes.interfaces != null && (!Array.isArray(changes.interfaces) || changes.interfaces.length > 2016)) {
    throw Error("Saved material interfaces must retain a complete table of at most 2016 pairs.");
  }
  if (changes.default == null && !count && changes.interfaces == null) {
    throw Error("Save at least one material assignment or an explicit interface replacement; the unchanged baseline is already included.");
  }
  // Rust checks every supplied interface against the effective material hashes and its source law.
  // Validation never reserializes this fragment: omitted/null interfaces and [] remain distinct.
}

/** One explicit boundary broadcasts over the current native material columns; no old model lock is inferred. */
export function buildMaterialSurfaceConditions(boundary) {
  const fields = ["inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w"];
  object(boundary, fields, fields, "Uniform sheet heat conditions");
  for (const key of ["inside_c", "outside_c"]) {
    if (typeof boundary[key] !== "number" || !Number.isFinite(boundary[key]) || boundary[key] < -273.15) {
      throw Error(`Sheet heat ${key} must be finite and at least -273.15 °C.`);
    }
  }
  for (const key of ["inside_resistance_m2_k_w", "outside_resistance_m2_k_w"]) nonnegative(boundary[key], `Sheet heat ${key}`);
  return JSON.stringify({ schema: "ftx-steady-surface-conditions/1", boundaries: [boundary] });
}

/** Author the existing forward vapor-potential boundary, independently of dry heat or available water. */
export function buildMaterialVaporSurfaceConditions(boundary) {
  const fields = ["inside_vapor_pressure_pa", "outside_vapor_pressure_pa", "inside_film_resistance_m2_pa_w",
    "outside_film_resistance_m2_pa_w", "latent_heat_j_kg"];
  object(boundary, fields, fields, "Uniform sheet vapor conditions");
  for (const key of ["inside_vapor_pressure_pa", "outside_vapor_pressure_pa"]) nonnegative(boundary[key], `Sheet vapor ${key}`);
  if (boundary.outside_vapor_pressure_pa > boundary.inside_vapor_pressure_pa) {
    throw Error("Sheet vapor outside pressure must not exceed inside pressure; this potential request does not qualify reverse transport.");
  }
  for (const key of ["inside_film_resistance_m2_pa_w", "outside_film_resistance_m2_pa_w", "latent_heat_j_kg"]) {
    if (typeof boundary[key] !== "number" || !Number.isFinite(boundary[key]) || boundary[key] <= 0) {
      throw Error(`Sheet vapor ${key} must be finite and positive.`);
    }
  }
  return JSON.stringify({ schema: "ftx-vapor-surface-conditions/1", boundaries: [boundary] });
}

/** Author prescribed source-model endpoints; Rust owns their geometry and numerical continuation. */
export function buildMaterialMotionRequest(state) {
  object(state, ["keyframes", "max_step_s", "max_trials", "max_pairs", "max_sample_vertices", "transient_surface"],
    ["keyframes", "max_step_s", "max_trials", "max_pairs"], "Guided motion");
  if (!Array.isArray(state.keyframes) || !state.keyframes.length || state.keyframes.length > 16) {
    throw Error("Motion needs 1–16 explicitly authored endpoints.");
  }
  if (typeof state.max_step_s !== "number" || !Number.isFinite(state.max_step_s) ||
    state.max_step_s < 1e-9 || state.max_step_s > 1) {
    throw Error("Motion maximum step must be finite and from 1e-9 to 1 second.");
  }
  for (const [key, maximum] of [["max_trials", 512], ["max_pairs", 1000000], ["max_sample_vertices", 2000000]]) {
    if (key === "max_sample_vertices" && !own(state, key)) continue;
    if (!Number.isSafeInteger(state[key]) || state[key] < 1 || state[key] > maximum) {
      throw Error(`Motion ${key} must be an integer from 1 to ${maximum}.`);
    }
  }
  let previous = 0, minimumTrials = 0;
  for (const frame of state.keyframes) {
    object(frame, ["time_s", "arm_abduction_deg", "leg_pose"], ["time_s", "arm_abduction_deg"], "Motion endpoint");
    if (typeof frame.time_s !== "number" || !Number.isFinite(frame.time_s) || frame.time_s > 60 ||
      frame.time_s - previous < 1e-9) {
      throw Error("Motion endpoint times must increase by at least 1e-9 seconds, through at most 60 seconds after the initial drape.");
    }
    if (typeof frame.arm_abduction_deg !== "number" || !Number.isFinite(frame.arm_abduction_deg) ||
      frame.arm_abduction_deg < 0 || frame.arm_abduction_deg > 180) {
      throw Error("Motion arm abduction must be finite and from 0 to 180 degrees.");
    }
    if (own(frame, "leg_pose") && frame.leg_pose !== null) {
      const pose = object(frame.leg_pose, ["posture", "fraction"], ["posture", "fraction"], "Motion leg pose");
      if (!["seated", "squat"].includes(pose.posture) || typeof pose.fraction !== "number" ||
        !Number.isFinite(pose.fraction) || pose.fraction < 0 || pose.fraction > 1) {
        throw Error("Motion leg pose needs seated or squat and a finite fraction from 0 to 1.");
      }
    }
    minimumTrials += Math.ceil((frame.time_s - previous) / state.max_step_s - 1e-12);
    previous = frame.time_s;
  }
  if (minimumTrials > state.max_trials) throw Error("Motion requires more nominal substeps than max_trials.");
  if (own(state, "transient_surface")) {
    const fields = ["schema", "initial_temperature_c", "cells_per_layer", "boundaries", "max_step_s", "max_cell_steps"];
    const heat = object(state.transient_surface, fields, fields, "Guided dry sheet heat");
    if (heat.schema !== "ftx-motion-surface-heat/1") throw Error("Guided dry sheet heat needs ftx-motion-surface-heat/1.");
    const temperature = (value, label) => {
      if (typeof value !== "number" || !Number.isFinite(value) || value < -273.15) {
        throw Error(`${label} must be finite and at least -273.15 °C.`);
      }
    };
    temperature(heat.initial_temperature_c, "Initial sheet temperature");
    for (const [key, maximum] of [["cells_per_layer", 64], ["max_cell_steps", 10000000]]) {
      if (!Number.isSafeInteger(heat[key]) || heat[key] < 1 || heat[key] > maximum) {
        throw Error(`Guided dry sheet heat ${key} must be an integer from 1 to ${maximum}.`);
      }
    }
    if (typeof heat.max_step_s !== "number" || !Number.isFinite(heat.max_step_s) || heat.max_step_s < 1e-9 || heat.max_step_s > 1) {
      throw Error("Maximum thermal substep must be finite and from 1e-9 to 1 second.");
    }
    if (!Array.isArray(heat.boundaries) || heat.boundaries.length !== state.keyframes.length) {
      throw Error("Save one complete sheet-heat boundary for each motion endpoint interval.");
    }
    const filmFields = ["inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w"];
    for (const film of heat.boundaries) {
      object(film, filmFields, filmFields, "Saved sheet-heat boundary");
      temperature(film.inside_c, "Inside reservoir temperature"); temperature(film.outside_c, "Outside reservoir temperature");
      nonnegative(film.inside_resistance_m2_k_w, "Inside film resistance");
      nonnegative(film.outside_resistance_m2_k_w, "Outside film resistance");
    }
  }
  return JSON.stringify({ schema: "ftx-drape-motion/1", ...state });
}

function motionPolicy(value) {
  const required = ["schema", "keyframes", "max_step_s", "max_trials", "max_pairs"];
  const advanced = ["transient_surface", "transient_coverage", "vapor_coverage_history"];
  object(value, [...required, "max_sample_vertices", ...advanced], required, "Prescribed motion");
  if (value.schema !== "ftx-drape-motion/1" || !Array.isArray(value.keyframes) ||
    !value.keyframes.length || value.keyframes.length > 16) {
    throw Error("Prescribed motion needs ftx-drape-motion/1 with 1–16 keyframes.");
  }
  if (!Number.isFinite(value.max_step_s) || value.max_step_s < 1e-9 || value.max_step_s > 1) {
    throw Error("Motion maximum substep must be finite and in 1e-9–1 seconds.");
  }
  for (const [key, maximum] of [["max_trials", 512], ["max_pairs", 1000000], ["max_sample_vertices", 2000000]]) {
    if (key === "max_sample_vertices" && !own(value, key)) continue;
    if (!Number.isSafeInteger(value[key]) || value[key] < 1 || value[key] > maximum) {
      throw Error(`Motion ${key} must be an integer from 1 to ${maximum}.`);
    }
  }
  let previous = 0, minimumTrials = 0;
  for (const frame of value.keyframes) {
    object(frame, ["time_s", "arm_abduction_deg", "leg_pose"], ["time_s", "arm_abduction_deg"], "Motion keyframe");
    if (!Number.isFinite(frame.time_s) || frame.time_s > 60 || frame.time_s - previous < 1e-9 ||
      !Number.isFinite(frame.arm_abduction_deg) || frame.arm_abduction_deg < 0 || frame.arm_abduction_deg > 180) {
      throw Error("Motion needs keyframes separated by at least 1e-9 seconds through 60 seconds and arm angles in 0–180 degrees.");
    }
    if (frame.leg_pose != null) {
      const pose = object(frame.leg_pose, ["posture", "fraction"], ["posture", "fraction"], "Motion leg pose");
      if (!["seated", "squat"].includes(pose.posture) || !Number.isFinite(pose.fraction) || pose.fraction < 0 || pose.fraction > 1) {
        throw Error("Motion leg pose needs seated or squat with a finite fraction in 0–1.");
      }
    }
    minimumTrials += Math.ceil((frame.time_s - previous) / value.max_step_s - 1e-12);
    previous = frame.time_s;
  }
  if (minimumTrials > value.max_trials) throw Error("Motion requires more nominal substeps than max_trials.");
  for (const key of advanced) {
    if (value[key] != null && (typeof value[key] !== "object" || Array.isArray(value[key]))) {
      throw Error(`Motion ${key} must retain its original object or null.`);
    }
  }
}

function materialStrainPolicy(value) {
  object(value, ["schema", "instances", "max_triangle_samples"], ["schema", "max_triangle_samples"], "Material strain");
  if (value.schema !== "ftx-material-strain/1" || !Number.isSafeInteger(value.max_triangle_samples) ||
    value.max_triangle_samples < 1 || value.max_triangle_samples > 64000000) {
    throw Error("Material strain needs ftx-material-strain/1 and an explicit integer triangle-sample budget from 1 to 64000000.");
  }
  if (value.instances != null) {
    uniqueNames(value.instances, "Material strain instance IDs", 4096);
    for (const id of value.instances) bytes(id, 4096, "Material strain instance IDs must not exceed 4096 UTF-8 bytes.");
  }
}

function contactLoadsPolicy(value) {
  const budgets = ["max_vertex_samples", "max_pair_evaluations"];
  object(value, ["schema", "instances", ...budgets], ["schema", ...budgets], "Body contact loads");
  if (value.schema !== "ftx-contact-loads/1" || budgets.some(key => !Number.isSafeInteger(value[key]) ||
    value[key] < 1 || value[key] > 64000000)) {
    throw Error("Body contact loads need ftx-contact-loads/1 and explicit integer vertex/pair budgets from 1 to 64000000.");
  }
  if (value.instances != null) {
    uniqueNames(value.instances, "Body contact-load instance IDs", 4096);
    for (const id of value.instances) bytes(id, 4096, "Body contact-load instance IDs must not exceed 4096 UTF-8 bytes.");
  }
}

function coveragePolicy(value, label, thermal = false) {
  const fields = ["body_triangle_ids", "instance_layers", "settings", "conditions"];
  object(value, [...fields, ...(thermal ? ["thermal_bridges"] : [])], fields, label);
  if (!Array.isArray(value.body_triangle_ids) || value.body_triangle_ids.some(id => !Number.isSafeInteger(id) || id < 0)) {
    throw Error(`${label} needs original integer body-triangle indices, without numeric coercion.`);
  }
  for (const key of ["instance_layers", "settings", "conditions"]) {
    if (!value[key] || typeof value[key] !== "object" || Array.isArray(value[key])) throw Error(`${label} ${key} must be an object.`);
  }
  if (Object.values(value.instance_layers).some(layer => typeof layer !== "string")) {
    throw Error(`${label} needs original string instance-to-layer identities.`);
  }
  if (value.thermal_bridges != null && (typeof value.thermal_bridges !== "object" || Array.isArray(value.thermal_bridges))) {
    throw Error("Coverage thermal_bridges must retain its original object or null.");
  }
  // Rust admits actual body indices, complete layers, projection, boundaries and source protocols
  // independently. An invalid physical domain must not erase unrelated drape/manufacturing output.
}

/** Author one fresh drape with authored or original motion; source files remain verbatim and Rust owns numerical admission. */
export function buildMaterialScenarioRequest(state) {
  object(state, ["fitSpec", "allowNoFit", "options", "maxMeshPoints", "maxVertexFrames", "surface",
    "motion", "maxMotionTrials", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage"],
    ["allowNoFit", "options", "maxMeshPoints", "maxVertexFrames"], "Guided scenario");
  if (typeof state.allowNoFit !== "boolean") throw Error("Declare the numerical-only study without a fit contract explicitly.");
  let fitSpec = "{}";
  if (own(state, "fitSpec")) {
    sourceObject(state.fitSpec, "Fit contract"); fitSpec = state.fitSpec;
  } else if (!state.allowNoFit) throw Error("Choose a fit contract or explicitly allow a numerical-only study without one.");
  const fields = ["spacing", "sew_steps", "drape_steps", "step", "max_settle_steps"];
  const options = object(state.options, [...fields, "self_contact"], fields, "Drape options");
  for (const key of ["spacing", "step"]) {
    if (typeof options[key] !== "number" || !Number.isFinite(options[key]) || options[key] <= 0) {
      throw Error("Drape " + key + " must be finite and positive.");
    }
  }
  for (const key of ["sew_steps", "drape_steps", "max_settle_steps"]) {
    if (!Number.isSafeInteger(options[key]) || options[key] < 0 || options[key] > 256) {
      throw Error("Drape " + key + " must be an integer from 0 to 256.");
    }
  }
  if (options.sew_steps + options.drape_steps < 6) throw Error("Request at least six sewing and gravity steps; the settling tail does not supply the initial metrics window.");
  if (options.sew_steps + options.drape_steps + options.max_settle_steps > 256) {
    throw Error("Combined sewing, gravity and settling steps must not exceed 256.");
  }
  if (options.max_settle_steps > 0 && !Number.isFinite((options.drape_steps + options.max_settle_steps) * options.step)) {
    throw Error("The declared gravity and settling duration must be finite.");
  }
  if (own(options, "self_contact") && options.self_contact !== null) {
    const keys = ["dhat", "thickness", "kappa", "mu", "eps_v"];
    const contact = object(options.self_contact, keys, keys, "Self-contact policy");
    for (const key of keys) {
      nonnegative(contact[key], "Self-contact " + key);
      if (!["thickness", "mu"].includes(key) && contact[key] === 0) throw Error("Self-contact " + key + " must be positive.");
    }
  }
  if (!Number.isSafeInteger(state.maxMeshPoints) || state.maxMeshPoints < 3 || state.maxMeshPoints > 32768) {
    throw Error("Scenario mesh budget must be an integer from 3 to 32768.");
  }
  if (!Number.isSafeInteger(state.maxVertexFrames) || state.maxVertexFrames < 1 || state.maxVertexFrames > 64000000) {
    throw Error("Scenario vertex-frame budget must be an integer from 1 to 64000000.");
  }
  if (own(state, "surface")) {
    const surface = sourceObject(state.surface, "Independent-sheet heat conditions");
    if (surface.schema !== "ftx-steady-surface-conditions/1" || !Array.isArray(surface.boundaries) ||
      !surface.boundaries.length || surface.boundaries.length > 65536) {
      throw Error("Supply original ftx-steady-surface-conditions/1 heat conditions with 1–65536 boundaries.");
    }
  }
  if (own(state, "coverage")) coveragePolicy(sourceObject(state.coverage, "Body coverage"), "Body coverage", true);
  if (own(state, "vaporCoverage")) coveragePolicy(sourceObject(state.vaporCoverage, "Vapor coverage"), "Vapor coverage");
  if (own(state, "vaporSurface")) {
    const vapor = sourceObject(state.vaporSurface, "Independent-sheet vapor conditions");
    object(vapor, ["schema", "boundaries", "model_sha256", "required_conditions", "required_method", "allow_mixed_protocols"],
      ["schema", "boundaries"], "Independent-sheet vapor conditions");
    if (vapor.schema !== "ftx-vapor-surface-conditions/1" || !Array.isArray(vapor.boundaries) ||
      !vapor.boundaries.length || vapor.boundaries.length > 65536) {
      throw Error("Supply original ftx-vapor-surface-conditions/1 vapor conditions with 1–65536 boundaries.");
    }
  }
  let motion;
  if (own(state, "motion")) {
    if (typeof state.motion === "string") {
      motionPolicy(sourceObject(state.motion, "Prescribed motion"));
      motion = state.motion;
    } else motion = buildMaterialMotionRequest(state.motion);
    if (!own(state, "maxMotionTrials")) {
      throw Error("Prescribed motion and its explicit integer scenario motion-trial budget must be supplied together.");
    }
    if (!Number.isSafeInteger(state.maxMotionTrials) || state.maxMotionTrials < 1 || state.maxMotionTrials > 32768) {
      throw Error("Prescribed motion requires an explicit integer scenario motion-trial budget from 1 to 32768.");
    }
  } else if (own(state, "maxMotionTrials")) {
    throw Error("A scenario motion-trial budget requires a prescribed motion request; both must be supplied together.");
  }
  if (own(state, "materialStrain")) materialStrainPolicy(sourceObject(state.materialStrain, "Material strain"));
  if (own(state, "contactLoads")) contactLoadsPolicy(sourceObject(state.contactLoads, "Body contact loads"));
  const parts = ['{"schema":"ftx-drape-scenario/1","options":', JSON.stringify(options), ',"fit_spec":', fitSpec,
    ',"max_mesh_points":', String(state.maxMeshPoints), ',"max_vertex_frames":', String(state.maxVertexFrames),
    ...(own(state, "surface") ? [',"surface":', state.surface] : []),
    ...(motion === undefined ? [] : [',"motion":', motion, ',"max_motion_trials":', String(state.maxMotionTrials)]),
    ...(own(state, "materialStrain") ? [',"material_strain":', state.materialStrain] : []),
    ...(own(state, "contactLoads") ? [',"contact_loads":', state.contactLoads] : []),
    ...(own(state, "coverage") ? [',"coverage":', state.coverage] : []),
    ...(own(state, "vaporSurface") ? [',"vapor_surface":', state.vaporSurface] : []),
    ...(own(state, "vaporCoverage") ? [',"vapor_coverage":', state.vaporCoverage] : []), '}'];
  let size = 0;
  for (const part of parts) {
    size += bytes(part);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided scenario request exceeds 16 MiB.");
  }
  return parts.join("");
}

/** Assemble exact scenario snapshots; Rust still owns every case's physical and policy admission. */
export function buildMaterialScenarioMatrixRequest(state) {
  object(state, ["maxEvaluations", "cases"], ["maxEvaluations", "cases"], "Guided named scenarios");
  if (!Array.isArray(state.cases) || !state.cases.length || state.cases.length > 8) {
    throw Error("A named scenario matrix needs 1–8 cases.");
  }
  const context = { maxEvaluations: state.maxEvaluations, cases: [] };
  const parts = []; let size = 0, frames = 0, trials = 0;
  const append = value => {
    size += bytes(value);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided named scenario request exceeds 16 MiB.");
    parts.push(value);
  };
  append('{"schema":"ftx-drape-scenario-matrix/1","max_evaluations":');
  if (!Number.isSafeInteger(state.maxEvaluations) || state.maxEvaluations < state.cases.length || state.maxEvaluations > 64) {
    throw Error("Named scenarios need an integer evaluation allowance covering every case, at most 64.");
  }
  append(String(state.maxEvaluations)); append(',"cases":[');
  for (const [index, item] of state.cases.entries()) {
    object(item, ["key", "scenario", "comparison"], ["key", "scenario"], "Named scenario snapshot");
    name(item.key, "Scenario case key");
    bytes(item.key, 128, "Scenario case keys must not exceed 128 UTF-8 bytes.");
    const scenario = sourceObject(item.scenario, "Original scenario snapshot");
    if (scenario.schema !== "ftx-drape-scenario/1") throw Error("Each named case needs an original ftx-drape-scenario/1 snapshot.");
    if (!Number.isSafeInteger(scenario.max_vertex_frames) || scenario.max_vertex_frames < 1 || scenario.max_vertex_frames > 64000000) {
      throw Error("Each scenario needs an explicit integer vertex-frame budget from 1 to 64000000.");
    }
    const motionTrials = own(scenario, "max_motion_trials") ? scenario.max_motion_trials : 4096;
    if (!Number.isSafeInteger(motionTrials) || motionTrials < 1 || motionTrials > 32768) {
      throw Error("Scenario motion-trial budgets must be integers from 1 to 32768; omission uses 4096.");
    }
    frames += scenario.max_vertex_frames;
    if (scenario.motion != null) trials += motionTrials;
    if (frames > 64000000 || trials > 32768) {
      throw Error("Named scenarios exceed 64000000 aggregate vertex frames or 32768 active motion trials.");
    }
    context.cases.push({ key: item.key, surface: scenario.surface != null,
      motion: scenario.motion != null, materialStrain: scenario.material_strain != null, contactLoads: scenario.contact_loads != null,
      coverage: scenario.coverage != null, vaporSurface: scenario.vapor_surface != null, vaporCoverage: scenario.vapor_coverage != null,
      motionVaporHistory: scenario.motion?.vapor_coverage_history != null,
      motionWetFilm: scenario.motion?.vapor_coverage_history?.wet_film != null,
      transientSurface: scenario.motion?.transient_surface != null, transientCoverage: scenario.motion?.transient_coverage != null,
      textileWater: hasTextileWaterPolicy(scenario.motion?.transient_surface),
      ...(own(item, "comparison") ? { comparison: parse(item.comparison, "Original case comparison") } : {}) });
    append(`${index ? "," : ""}{"key":${JSON.stringify(item.key)},"scenario":`); append(item.scenario);
    if (own(item, "comparison")) { append(',"comparison":'); append(item.comparison); }
    append("}");
  }
  matrixContext(context);
  append("]}");
  return parts.join("");
}

function stockOptions(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${label} must be an options object.`);
  allowance(value, `${label} seam allowance`);
  return value;
}
function finiteInventory(value) {
  const request = object(value, ["schema", "stocks", "options", "pricing"], ["schema", "stocks", "options"], "Finite stock");
  if (request.schema !== "ftx-finite-stock-request/1" || !Array.isArray(request.stocks)) {
    throw Error("Finite stock requires an ftx-finite-stock-request/1 inventory.");
  }
  stockOptions(request.options, "Finite-stock options");
  return request;
}
function completeCuttingJob(value) {
  const request = object(value, ["schema", "routes", "material_routes", "roll", "finite_stock", "purchasing"],
    ["schema"], "Complete cutting job");
  if (!["ftx-cutting-job/1", "ftx-cutting-job/2"].includes(request.schema)) {
    throw Error("Use an ftx-cutting-job/1 or /2 complete cutting request.");
  }
  if (!request.roll && !request.finite_stock) throw Error("A complete cutting job needs its original stock planning inputs.");
  if (request.roll !== undefined && request.roll !== null) stockOptions(request.roll, "Job roll options");
  if (request.finite_stock !== undefined && request.finite_stock !== null) finiteInventory(request.finite_stock);
  if (request.purchasing !== undefined && request.purchasing !== null) {
    const purchasing = object(request.purchasing, ["quotes", "market", "finite_bindings"], ["quotes", "market"], "Job purchasing");
    for (const key of ["quotes", "market"]) {
      if (!purchasing[key] || typeof purchasing[key] !== "object" || Array.isArray(purchasing[key])) {
        throw Error("Job purchasing needs its original quote book and market objects.");
      }
    }
  }
  return request;
}

function linkedValues(values) {
  if (!Array.isArray(values) || !values.length || values.length > 64 ||
    Array.from(values).some((value, index) => typeof value !== "number" || !Number.isFinite(value) ||
      (index > 0 && values[index - 1] >= value))) {
    throw Error("Linked design needs 1–64 finite strictly increasing millimetre values.");
  }
  return values;
}

function linkedSite(value, pieces, values) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error("Linked alteration site must be an object.");
  const move = own(value, "op");
  const fields = move ? ["op", "piece", "vertex", "dx_multiplier", "dy_multiplier"] : ["piece", "axis", "at_mm", "multiplier"];
  const site = object(value, fields, fields, "Linked alteration site");
  name(site.piece, "Altered source piece");
  if (!pieces.has(site.piece)) throw Error("Alteration names a missing source piece.");
  let multipliers;
  if (move) {
    if (site.op !== "move") throw Error("A vertex site requires the explicit move operation.");
    name(site.vertex, "Altered source vertex");
    multipliers = [site.dx_multiplier, site.dy_multiplier];
    if (multipliers.some(n => typeof n !== "number" || !Number.isFinite(n)) || multipliers.every(n => n === 0)) {
      throw Error("Linked vertex moves need finite x/y multipliers, at least one nonzero.");
    }
  } else {
    if (!["x", "y"].includes(site.axis) || typeof site.at_mm !== "number" || !Number.isFinite(site.at_mm) ||
      typeof site.multiplier !== "number" || !Number.isFinite(site.multiplier) || site.multiplier === 0) {
      throw Error("Linked alterations need an x/y axis, finite cut coordinate and finite nonzero multiplier.");
    }
    multipliers = [site.multiplier];
  }
  for (const value of values) for (const multiplier of multipliers) {
    const displacement = value * multiplier;
    if (!Number.isFinite(displacement) || (value !== 0 && multiplier !== 0 && displacement === 0)) {
      throw Error("Linked design displacement overflows or underflows; no alteration is silently dropped.");
    }
  }
  return move ? { op: "move", piece: site.piece, vertex: site.vertex,
    dx_multiplier: site.dx_multiplier, dy_multiplier: site.dy_multiplier } :
    { piece: site.piece, axis: site.axis, at_mm: site.at_mm, multiplier: site.multiplier };
}

function adaptiveDomain(variable) {
  if (!Array.isArray(variable.boundsMm) || variable.boundsMm.length !== 2) throw Error("Adaptive bounds need exactly two millimetre coordinates.");
  const [low, high] = variable.boundsMm, span = high - low;
  if ([low, high, span, variable.initialMm, variable.initialStepMm, variable.minimumStepMm]
    .some(n => typeof n !== "number" || !Number.isFinite(n)) || low >= high ||
    variable.initialMm < low || variable.initialMm > high || variable.minimumStepMm <= 0 ||
    variable.initialStepMm < variable.minimumStepMm || variable.initialStepMm > span) {
    throw Error("Adaptive variables need finite ordered bounds, an in-domain initial value and 0 < minimum step <= initial step <= domain span.");
  }
  return [low, high, variable.initialMm];
}

function adaptiveGuidance(design, candidates, comparison, matrix) {
  if (!["fit_band_violation_mm", "consumer_comparison", "consumer_pareto", "consumer_worst_case", "consumer_matrix_pareto"].includes(design.objective)) {
    throw Error("Choose fit-band, nominated-case, worst-case or Pareto adaptive guidance.");
  }
  if (design.objective === "fit_band_violation_mm") {
    if (own(design, "materialKey") || own(design, "caseKey")) throw Error("Fit-band discovery does not accept a guidance materialKey or caseKey.");
    return;
  }
  name(design.materialKey, "Adaptive guidance materialKey");
  if (design.materialKey !== "baseline" && !candidates.some(candidate => candidate?.key === design.materialKey)) {
    throw Error("Adaptive materialKey must identify the unchanged baseline or an existing material choice.");
  }
  const allCases = ["consumer_worst_case", "consumer_matrix_pareto"].includes(design.objective);
  if (!matrix && (allCases || own(design, "caseKey"))) {
    throw Error("Named-case adaptive guidance requires a scenario matrix.");
  }
  let effective = comparison;
  if (matrix) {
    const policies = matrix.cases.map(item => item.comparison ?? comparison);
    if (allCases) {
      if (own(design, "caseKey")) throw Error("All-case adaptive guidance does not accept a caseKey.");
      if (design.objective === "consumer_matrix_pareto") {
        if (policies.some(policy => !Array.isArray(policy?.objectives) || !policy.objectives.length || policy.objectives.length > 16) ||
          policies.reduce((count, policy) => count + policy.objectives.length, 0) < 2) {
          throw Error("All-case Pareto guidance needs 1–16 objectives per case and at least two case/objective coordinates.");
        }
        return;
      }
      const first = policies[0];
      if (policies.some(policy => !Array.isArray(policy?.objectives) || policy.objectives.length !== 1 ||
        policy.basis !== first.basis || policy.objectives[0]?.metric !== first.objectives[0]?.metric ||
        policy.objectives[0]?.direction !== first.objectives[0]?.direction)) {
        throw Error("Worst-case guidance needs one identical metric, direction and comparison basis in every case.");
      }
      return;
    }
    name(design.caseKey, "Adaptive guidance caseKey");
    const index = matrix.cases.findIndex(item => item.key === design.caseKey);
    if (index < 0) throw Error("Adaptive caseKey must identify an existing named scenario case.");
    effective = policies[index];
  }
  const count = Array.isArray(effective?.objectives) ? effective.objectives.length : 0;
  if (design.objective === "consumer_comparison" ? count !== 1 : count < 2 || count > 16) {
    throw Error("Single-consumer guidance needs exactly one comparison objective; Pareto guidance needs 2–16.");
  }
}

/** Author ordered canonical seam adjustments; root finding and geometry admission stay in Rust. */
export function buildGuidedDesignAfterOperations(after, pieceIds, seamIds) {
  if (!Array.isArray(after) || after.length > 16) throw Error("Choose at most 16 ordered seam adjustments.");
  if (!after.length) return [];
  const pieces = uniqueNames(pieceIds, "Pattern source pieces", 4096);
  if (!Array.isArray(seamIds)) throw Error("Seam adjustments require the original source seam identities.");
  const counts = new Map();
  for (const id of seamIds) if (typeof id === "string") counts.set(id, (counts.get(id) || 0) + 1);
  const seam = id => {
    name(id, "Adjusted source seam");
    if (counts.get(id) !== 1) throw Error(`Seam adjustment needs exactly one original source seam named ${id}.`);
  };
  return Array.from(after, value => {
    if (value?.op === "true_seam") {
      object(value, ["op", "seam", "end"], ["op", "seam", "end"], "Seam truing");
      seam(value.seam);
      if (!["start", "end"].includes(value.end)) throw Error("Seam truing needs an explicit B-side start or end endpoint.");
      return { op: "true_seam", seam: value.seam, end: value.end };
    }
    if (value?.op !== "cap_ease") throw Error("Guided seam adjustments support only true_seam and cap_ease.");
    object(value, ["op", "piece", "seams", "target_ratio"], ["op", "piece", "seams", "target_ratio"], "Cap ease");
    name(value.piece, "Cap source piece");
    if (!pieces.has(value.piece)) throw Error("Cap ease names a missing source piece.");
    if (!Array.isArray(value.seams) || !value.seams.length) throw Error("Cap ease needs at least one original source seam.");
    const selected = new Set();
    for (const id of value.seams) {
      seam(id);
      if (selected.has(id)) throw Error("Cap-ease seam names must be unique.");
      selected.add(id);
    }
    if (typeof value.target_ratio !== "number" || !Number.isFinite(value.target_ratio) || value.target_ratio <= 0) {
      throw Error("Cap-ease target ratio must be finite and positive.");
    }
    return { op: "cap_ease", piece: value.piece, seams: [...value.seams], target_ratio: value.target_ratio };
  });
}

function linkedDesign(value, pieces, candidates, comparison, matrix, seamIds) {
  const multiple = !!value && typeof value === "object" && own(value, "variables");
  const mode = multiple && own(value, "mode") ? value.mode : "grid";
  if (!["grid", "adaptive"].includes(mode)) throw Error("Choose grid or adaptive linked design mode.");
  const adaptive = mode === "adaptive";
  const fields = multiple ? ["variables", "maxDesigns", "fitSpec"] :
    ["pieceIds", "axis", "atMm", "multiplier", "values", "maxDesigns", "fitSpec"];
  const required = adaptive ? [...fields, "mode", "objective", "maxEvaluations"] : fields;
  const allowed = adaptive ? [...required, "materialKey", "caseKey"] : multiple ? [...fields, "mode", "maxCandidates"] : fields;
  const design = object(value, [...allowed, "after"], required, "Linked design");
  const after = own(design, "after") ? buildGuidedDesignAfterOperations(design.after, [...pieces], seamIds) : undefined;
  if (adaptive) {
    if (!Number.isSafeInteger(design.maxEvaluations) || design.maxEvaluations < 1 || design.maxEvaluations > 256) {
      throw Error("The adaptive evaluation allowance must be an integer from 1 to 256.");
    }
    adaptiveGuidance(design, candidates, comparison, matrix);
  }
  let authored = design.variables;
  if (!multiple) {
    const selected = uniqueNames(design.pieceIds, "Altered source pieces", 16);
    if ([...selected].some(id => !pieces.has(id))) throw Error("Alteration names a missing source piece.");
    authored = [{ key: "linked_adjustment", values: design.values,
      sites: design.pieceIds.map(piece => ({ piece, axis: design.axis, at_mm: design.atMm, multiplier: design.multiplier })) }];
  }
  if (!Array.isArray(authored) || !authored.length || authored.length > 3) throw Error("Linked design needs 1–3 variables.");
  const keys = new Set();
  let totalPoints = 1, totalSites = after?.length ?? 0;
  const variables = Array.from(authored, value => {
    const fields = adaptive ? ["key", "boundsMm", "initialMm", "initialStepMm", "minimumStepMm", "sites"] : ["key", "values", "sites"];
    const variable = object(value, fields, fields, "Linked variable");
    name(variable.key, "Linked variable key");
    if (keys.has(variable.key)) throw Error("Linked variable keys must be unique.");
    keys.add(variable.key);
    const values = adaptive ? adaptiveDomain(variable) : linkedValues(variable.values);
    if (!Array.isArray(variable.sites) || !variable.sites.length) throw Error("Each linked variable needs alteration sites.");
    totalSites += variable.sites.length;
    if (totalSites > 16) throw Error("Linked design exceeds 16 ordered alteration operations including variable sites and seam adjustments.");
    const sites = Array.from(variable.sites, site => linkedSite(site, pieces, values));
    if (adaptive) return { key: variable.key, bounds_mm: variable.boundsMm, initial_mm: variable.initialMm,
      initial_step_mm: variable.initialStepMm, minimum_step_mm: variable.minimumStepMm, sites };
    totalPoints *= values.length;
    return { key: variable.key, values_mm: values, sites };
  });
  let maxCandidates;
  if (!adaptive) {
    if (!own(design, "maxCandidates") && totalPoints > 256) {
      throw Error("Linked grid exceeds 256 points; declare maxCandidates from 1 to 256 for explicit prefix screening.");
    }
    maxCandidates = own(design, "maxCandidates") ? design.maxCandidates : totalPoints;
    if (!Number.isSafeInteger(maxCandidates) || maxCandidates < 1 || maxCandidates > 256) {
      throw Error("The linked fit-screening allowance must be an integer from 1 to 256.");
    }
  }
  if (!Number.isSafeInteger(design.maxDesigns) || design.maxDesigns < 1 || design.maxDesigns > 31) {
    throw Error("The passing-design allowance must be an integer from 1 to 31.");
  }
  const retained = adaptive ? design.maxDesigns : Math.min(totalPoints, design.maxDesigns);
  if ((retained + 1) * (candidates.length + 1) > 32) {
    throw Error("Linked design exceeds 32 expanded design/material alternatives including both baselines; reduce its passing-design allowance or material shortlist.");
  }
  if (typeof design.fitSpec !== "string") throw Error("Design fit contract must retain its original JSON text.");
  bytes(design.fitSpec, 256 * 1024, "Design fit contract exceeds 256 KiB of UTF-8 source.");
  const fit = sourceObject(design.fitSpec, "Design fit contract");
  if ((own(fit, "schema") && fit.schema !== "fabric-fit/1") || !Array.isArray(fit.points) || !fit.points.length || fit.points.length > 32) {
    throw Error("Design fit needs 1–32 explicit points and fabric-fit/1 when a schema is supplied.");
  }
  const search = JSON.stringify(adaptive ? { objective: design.objective,
    ...(own(design, "materialKey") ? { material_key: design.materialKey } : {}),
    ...(own(design, "caseKey") ? { case_key: design.caseKey } : {}), variables,
    ...(after === undefined ? {} : { after }),
    max_evaluations: design.maxEvaluations, max_designs: design.maxDesigns } :
    { variables, ...(after === undefined ? {} : { after }), max_designs: design.maxDesigns, fit_screening: { max_candidates: maxCandidates } });
  bytes(search, 256 * 1024, `Linked design ${mode} exceeds 256 KiB of authored domain data.`);
  return { [adaptive ? "adaptive" : "grid"]: search, fitSpec: design.fitSpec, retainedDesigns: retained + 1,
    probes: adaptive && design.objective !== "fit_band_violation_mm" ? design.maxEvaluations : 0 };
}

function scenarioConsumers(value) {
  const enabled = key => value != null && own(value, key) && value[key] === true;
  return { surface: enabled("surface"), motion: enabled("motion"), material_strain: enabled("materialStrain"),
    motion_material_strain: enabled("motion") && enabled("materialStrain"), contact_loads: enabled("contactLoads"),
    motion_contact_loads: enabled("motion") && enabled("contactLoads"), coverage: enabled("coverage"),
    vapor_surface: enabled("vaporSurface"), vapor_coverage: enabled("vaporCoverage"),
    motion_vapor_coverage: enabled("motion") && enabled("vaporCoverage") && enabled("motionVaporHistory"),
    motion_wet_film: enabled("motion") && enabled("vaporCoverage") && enabled("motionVaporHistory") && enabled("motionWetFilm"),
    transient_surface: enabled("motion") && enabled("transientSurface"),
    transient_coverage: enabled("motion") && enabled("transientCoverage") };
}

function hasTextileWater(value) {
  return value != null && own(value, "textileWater") && value.textileWater === true && scenarioConsumers(value).transient_surface;
}

function matrixContext(value) {
  const matrix = object(value, ["maxEvaluations", "cases"], ["maxEvaluations", "cases"], "Named scenario context");
  if (!Array.isArray(matrix.cases) || !matrix.cases.length || matrix.cases.length > 8 ||
    !Number.isSafeInteger(matrix.maxEvaluations) || matrix.maxEvaluations < matrix.cases.length || matrix.maxEvaluations > 64) {
    throw Error("Named scenarios need 1–8 cases and an integer evaluation allowance covering every case, at most 64.");
  }
  const keys = new Set();
  for (const item of matrix.cases) {
    object(item, ["key", "surface", "motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage",
      "motionVaporHistory", "motionWetFilm", "transientSurface", "transientCoverage", "textileWater", "comparison"], ["key", "surface"], "Named scenario case");
    name(item.key, "Scenario case key");
    bytes(item.key, 128, "Scenario case keys must not exceed 128 UTF-8 bytes.");
    if (keys.has(item.key)) throw Error("Scenario case keys must be unique.");
    keys.add(item.key);
    if (typeof item.surface !== "boolean") throw Error("Each named case needs an explicit surface-request boolean.");
    for (const key of ["motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
      "transientSurface", "transientCoverage", "textileWater"]) {
      if (own(item, key) && typeof item[key] !== "boolean") throw Error(`Named case ${key} availability must be a boolean when supplied.`);
    }
    if (item.comparison != null) {
      const policy = item.comparison;
      if (typeof policy !== "object" || Array.isArray(policy) ||
        !["nominal", "reported_bounds"].includes(policy.basis) || !Array.isArray(policy.objectives) ||
        !policy.objectives.length || policy.objectives.length > 16 || policy.objectives.some(objective =>
          !objective || typeof objective !== "object" || Array.isArray(objective) ||
          typeof objective.metric !== "string" || !objective.metric.trim() ||
          !["minimize", "maximize"].includes(objective.direction))) {
        throw Error("Each supplied case comparison needs a declared basis and 1–16 explicit metric/direction objectives.");
      }
      // This is an original engine policy, not a guided-policy draft. Rust admits its complete
      // metrics, guards, consumers and protocols; the browser only reads guidance coordinates.
    }
  }
  return matrix;
}

/**
 * Author an unchanged baseline and 1–7 frozen choices, optionally with linked grid/adaptive discovery.
 * `pieceIds` are the current pattern's source IDs. `target` is default or named pieces.
 * `cutting` uses authored numeric roll options or an exact original policy file. Optional
 * `finiteStock` and `cuttingJob` retain original inventory/job JSON instead of roll cutting.
 * sewing/selection and purchasing inputs always retain their original JSON texts.
 * `assemblyMass` is a canonical inventory
 * request (original JSON text for sourced extras, or an authored object with no extras).
 * Objectives, inclusive constraints and required consumers are explicit; no completeness or
 * protocol consent is inferred. Original source text is embedded verbatim, including locked numbers.
 * Optional scenario context describes the separately dispatched drape/heat/vapor/motion/strain/load request, is never
 * serialized into the study, and does not enable scenario consumers in manufacturing-only calls.
 * Optional consumer flags default to false; motionVaporHistory, motionWetFilm, transientSurface and transientCoverage
 * describe actual requested motion subpolicies, independently of the steady surface/coverage consumers.
 * textileWater describes the actual nested surface-water policy; its metrics share the transient_surface consumer.
 * Its optional matrix retains ordered case
 * keys, consumer availability, original parsed comparison
 * policies and the callback allowance. Only guidance selection and capacity are read here; the
 * caller retains the original matrix JSON for native admission and exact replay.
 * Optional `design` authors ordered linked grid or adaptive variables and retains the original fit contract;
 * the original single-slash-variable shape remains supported without changing its serialized request.
 * Its optional `after` applies explicit seam adjustments after those sites, under the same operation cap.
 * `seamIds` are original source identities for these adjustments and never enter the study request.
 * Selected-body dispatch and canonical geometry/fit/consumer admission remain with the caller and Rust.
 */
export function buildMaterialStudyRequest(state, scenario) {
  let matrix;
  if (scenario !== undefined) {
    object(scenario, ["drape", "surface", "motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage",
      "motionVaporHistory", "motionWetFilm", "transientSurface", "transientCoverage", "textileWater", "matrix"], ["drape", "surface"], "Scenario comparison context");
    if (scenario.drape !== true || typeof scenario.surface !== "boolean") {
      throw Error("Scenario comparison needs explicit drape: true and a boolean surface request.");
    }
    for (const key of ["motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
      "transientSurface", "transientCoverage", "textileWater"]) {
      if (own(scenario, key) && typeof scenario[key] !== "boolean") throw Error(`Scenario ${key} availability must be a boolean when supplied.`);
    }
    if (own(scenario, "matrix")) {
      matrix = matrixContext(scenario.matrix);
      for (const key of ["surface", "motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
        "transientSurface", "transientCoverage", "textileWater"]) {
        if ((own(scenario, key) && scenario[key] === true) !== matrix.cases.some(item => own(item, key) && item[key] === true)) {
          throw Error(`Named scenario ${key} availability must describe its actual cases.`);
        }
      }
    }
  }
  object(state, ["candidates", "target", "pieceIds", "seamIds", "cutting", "finiteStock", "cuttingJob", "assemblyMass", "objectives", "basis",
    "sewing", "sewingSelection", "purchasing", "cutAndSewPurchasing", "constraints", "requiredConsumers", "design"],
    ["candidates", "pieceIds", "objectives", "basis"], "Guided study");
  const pieces = uniqueNames(state.pieceIds, "Pattern source pieces", 4096);
  if (!Array.isArray(state.candidates) || !state.candidates.length || state.candidates.length > MAX_GUIDED_STUDY_CANDIDATES) {
    throw Error("Add 1–7 material choices; the unchanged baseline is included separately.");
  }
  // A complete saved configuration carries its own assignments; only individual selections use
  // the shared target. An unused target must not block configuration-only comparisons.
  const needsTarget = state.candidates.some(candidate => candidate && own(candidate, "selection"));
  const target = needsTarget ? object(state.target, ["scope", "ids"], ["scope"], "Material target") : null;
  if (target?.scope === "pieces") {
    if (!own(target, "ids")) throw Error("Choose source pieces for the material target.");
    const selected = uniqueNames(target.ids, "Selected source pieces", 4096);
    if ([...selected].some(id => !pieces.has(id))) throw Error("Material target names a missing source piece.");
  } else if (target && (target.scope !== "default" || own(target, "ids"))) throw Error("Choose a default or explicit piece target.");
  if (!["nominal", "reported_bounds"].includes(state.basis)) throw Error("Choose nominal or reported-bound comparison.");
  const design = own(state, "design") ? linkedDesign(state.design, pieces, state.candidates,
    { basis: state.basis, objectives: state.objectives }, matrix, state.seamIds) : null;
  if (matrix) {
    const slots = ((design?.retainedDesigns ?? 1) * (state.candidates.length + 1) + (design?.probes ?? 0)) * matrix.cases.length;
    if (slots > matrix.maxEvaluations) {
      throw Error(`Named scenarios require ${slots} reserved case evaluations including probes, retained designs and both baselines; the matrix allows ${matrix.maxEvaluations}.`);
    }
  }

  if (["cutting", "finiteStock", "cuttingJob"].filter(key => own(state, key)).length > 1) {
    throw Error("Choose one stock plan: roll cutting, finite inventory or a complete cutting job.");
  }
  if (own(state, "cuttingJob") && own(state, "purchasing")) {
    throw Error("A complete cutting job retains its own purchasing; omit separate fabric purchasing.");
  }
  const finiteStock = own(state, "finiteStock") ? finiteInventory(sourceObject(state.finiteStock, "Finite-stock inventory")) : null;
  const cuttingJob = own(state, "cuttingJob") ? completeCuttingJob(sourceObject(state.cuttingJob, "Complete cutting job")) : null;

  const rawCutting = typeof state.cutting === "string";
  const cutting = own(state, "cutting") ? object(rawCutting ? sourceObject(state.cutting, "Cutting options") : state.cutting,
    ["copies", "gap_mm", "edge_margin_mm", "end_margin_mm", "seam_allowance_mm",
      ...(rawCutting ? ["width_basis", "maximum_marker_length_m", "nesting", "program"] : [])],
    ["copies", "gap_mm", "edge_margin_mm", "end_margin_mm"], "Cutting options") : null;
  if (cutting) {
    if (!Number.isSafeInteger(cutting.copies) || cutting.copies < 1 || cutting.copies > 4096) throw Error("Garment copies must be an integer from 1 to 4096.");
    for (const key of ["gap_mm", "edge_margin_mm", "end_margin_mm"]) nonnegative(cutting[key], key);
    allowance(cutting, "Cutting seam allowance");
  }
  if (own(state, "sewing") && own(state, "sewingSelection")) throw Error("Choose a sewing policy or recipe selection, not both.");
  const sewing = own(state, "sewing") ? sourceObject(state.sewing, "Sewing policy") : null;
  const selection = own(state, "sewingSelection") ? sourceObject(state.sewingSelection, "Sewing selection") : null;
  const sewingOptions = selection ? selection.options : sewing;
  if (selection && (!sewingOptions || typeof sewingOptions !== "object" || Array.isArray(sewingOptions))) {
    throw Error("Sewing selection needs its complete template options.");
  }
  let purchasing = null;
  if (own(state, "purchasing")) {
    purchasing = object(state.purchasing, ["quotes", "market"], ["quotes", "market"], "Purchasing");
    sourceObject(purchasing.quotes, "Quote book"); sourceObject(purchasing.market, "Market scope");
    if (!cutting) throw Error("Fabric purchasing requires roll cutting.");
  }
  if (own(state, "cutAndSewPurchasing") && typeof state.cutAndSewPurchasing !== "boolean") {
    throw Error("Combined fabric and thread purchasing must be an explicit boolean.");
  }
  const combinedPurchasing = state.cutAndSewPurchasing === true;
  if (combinedPurchasing && ((!purchasing && !cuttingJob?.purchasing) || !sewingOptions?.consumables?.purchasing)) {
    throw Error("Combined fabric and thread purchasing requires roll purchasing and sewing thread purchasing, or a complete job with its own purchasing and sewing thread purchasing.");
  }
  let mass = null, massJson = null;
  if (own(state, "assemblyMass")) {
    const raw = typeof state.assemblyMass === "string";
    mass = object(raw ? parse(state.assemblyMass, "Assembly inventory") : state.assemblyMass,
      ["schema", "inventory_complete", "extras", "seam_allowance_mm"],
      ["schema", "inventory_complete", "extras"], "Assembly inventory");
    if (mass.schema !== "ftx-assembly-mass/1" || typeof mass.inventory_complete !== "boolean"
      || !Array.isArray(mass.extras) || mass.extras.length > 1024) throw Error("Supply an explicit assembly inventory and completeness declaration.");
    if (!raw && mass.extras.length) throw Error("Retain the original inventory JSON text when supplying sourced extras.");
    const ids = new Set();
    for (const extra of mass.extras) {
      if (!extra || typeof extra !== "object" || Array.isArray(extra) || !own(extra, "id")) throw Error("Each extra needs its own inventory ID.");
      name(extra.id, "Extra inventory ID");
      if (ids.has(extra.id)) throw Error("Extra inventory IDs must be unique.");
      ids.add(extra.id);
    }
    const massAllowance = allowance(mass, "Assembly seam allowance");
    const cuttingPolicies = [cutting, finiteStock?.options, cuttingJob?.roll, cuttingJob?.finite_stock?.options].filter(Boolean);
    if (cuttingPolicies.some(policy => massAllowance !== allowance(policy, "Cutting seam allowance"))) {
      throw Error("Use the same seam allowance override for cutting and assembly mass.");
    }
    massJson = raw ? state.assemblyMass : JSON.stringify(mass);
  }
  if (!cutting && !finiteStock && !cuttingJob && !mass && !sewingOptions && !scenario) throw Error("Choose cutting, sewing or assembly mass to evaluate.");
  const consumers = {
    cutting: !!cutting, cutting_program: !!cutting?.program,
    finite_stock: !!finiteStock, finite_stock_program: !!finiteStock?.options.program,
    cutting_job: !!cuttingJob,
    cutting_job_program: !!(cuttingJob?.roll?.program || cuttingJob?.finite_stock?.options.program),
    cutting_job_purchasing: !!cuttingJob?.purchasing,
    sewing: !!sewingOptions, purchasing: !!purchasing,
    sewing_consumables: !!sewingOptions?.consumables,
    sewing_purchasing: !!sewingOptions?.consumables?.purchasing,
    cut_and_sew_purchasing: combinedPurchasing, assembly_mass: !!mass,
    drape: !!scenario, ...scenarioConsumers(scenario),
  };
  if (matrix) {
    for (const consumer of ["motion_material_strain", "motion_contact_loads", "motion_vapor_coverage", "motion_wet_film", "transient_surface", "transient_coverage"]) {
      consumers[consumer] = matrix.cases.some(item => scenarioConsumers(item)[consumer]);
    }
  }
  const requireConsumer = consumer => {
    if (typeof consumer !== "string" || !own(consumers, consumer)) throw Error(`Unsupported guided consumer: ${consumer}.`);
    if (!consumers[consumer]) throw Error(`Comparison requires requested ${consumer} inputs.`);
  };
  const textileWater = matrix ? matrix.cases.some(hasTextileWater) : hasTextileWater(scenario);
  const metric = (key, basis) => {
    if (typeof key !== "string" || !own(METRIC_CONSUMERS, key)) throw Error(`Unsupported guided comparison metric: ${key}.`);
    requireConsumer(METRIC_CONSUMERS[key]);
    if (key.startsWith("textile_water_") && !textileWater) {
      throw Error("Textile-water criteria require motion and an original transient-surface water policy in the same case.");
    }
    if (key === "assembly_mass_kg" && !mass.inventory_complete) {
      throw Error("Assembly-mass comparison requires an explicit complete inventory declaration.");
    }
    // Rust admits the actual source endpoints, geometry and thermal/vapor law; authoring never
    // supplies a range or changes signed heat into an absolute value.
    if (basis === "reported_bounds" && !REPORTED_BOUND_METRICS.has(key)) {
      throw Error("Guided reported-bound comparison supports complete assembly mass, steady heat, vapor potential and final body-water totals only; use an explicit nominal basis for other guided metrics and guards.");
    }
  };
  if (!Array.isArray(state.objectives) || !state.objectives.length || state.objectives.length > 16) throw Error("Choose 1–16 comparison objectives.");
  const metrics = new Set();
  for (const objective of state.objectives) {
    object(objective, ["metric", "direction"], ["metric", "direction"], "Comparison objective");
    if (!["minimize", "maximize"].includes(objective.direction)) throw Error("Choose a supported objective and direction.");
    if (metrics.has(objective.metric)) throw Error("Comparison objectives must be unique.");
    metrics.add(objective.metric);
    metric(objective.metric, state.basis);
  }
  const constraints = own(state, "constraints") ? state.constraints : [];
  if (!Array.isArray(constraints) || constraints.length > 64) throw Error("Choose at most 64 comparison constraints.");
  metrics.clear();
  for (const constraint of constraints) {
    object(constraint, ["metric", "minimum", "maximum", "basis"], ["metric"], "Comparison constraint");
    if (metrics.has(constraint.metric)) throw Error("Comparison constraint metrics must be unique.");
    metrics.add(constraint.metric);
    const basis = own(constraint, "basis") ? constraint.basis : state.basis;
    if (!["nominal", "reported_bounds"].includes(basis)) throw Error("Choose nominal or reported-bound constraint basis.");
    const bounds = ["minimum", "maximum"].filter(key => own(constraint, key) && constraint[key] !== null);
    if (!bounds.length || bounds.some(key => typeof constraint[key] !== "number" || !Number.isFinite(constraint[key]))
      || (bounds.length === 2 && constraint.minimum > constraint.maximum)) {
      throw Error("Comparison constraints need finite ordered inclusive bounds.");
    }
    metric(constraint.metric, basis);
  }
  const required = own(state, "requiredConsumers") ? state.requiredConsumers : [];
  if (!Array.isArray(required) || required.length > 16) throw Error("Choose at most 16 required consumers.");
  const requiredSet = new Set();
  for (const consumer of required) {
    if (requiredSet.has(consumer)) throw Error("Required consumers must be unique.");
    requiredSet.add(consumer); requireConsumer(consumer);
  }
  if (matrix) {
    const needed = new Set([...required, ...[...state.objectives, ...constraints].map(item => METRIC_CONSUMERS[item.metric])]);
    const needsTextileWater = [...state.objectives, ...constraints].some(item => item.metric.startsWith("textile_water_"));
    for (const item of matrix.cases) {
      if (item.comparison != null) continue;
      const missing = Object.entries(scenarioConsumers(item)).find(([consumer, available]) => needed.has(consumer) && !available);
      if (missing) throw Error(`Named case ${item.key} inherits a comparison requiring ${missing[0]} inputs it does not request.`);
      if (needsTextileWater && !hasTextileWater(item)) {
        throw Error(`Named case ${item.key} inherits a comparison requiring textile-water inputs it does not request.`);
      }
    }
  }

  const parts = []; let size = 0;
  const append = value => {
    size += bytes(value);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided study request exceeds 16 MiB.");
    parts.push(value);
  };
  append(`{"schema":"${design ? "ftx-design-study/1" : "ftx-material-study/1"}","variants":[{"key":"baseline","changes":{"schema":"ftx-material-bindings/1"}}`);
  const keys = new Set(["baseline"]), identities = new Set(), configurations = new Set();
  for (const candidate of state.candidates) {
    object(candidate, ["key", "selection", "changes"], ["key"], "Material choice");
    if (own(candidate, "selection") === own(candidate, "changes")) {
      throw Error("Each material choice needs one frozen selection or one saved configuration.");
    }
    name(candidate.key, "Material choice name");
    if (keys.has(candidate.key)) throw Error("Material choice names must be unique; baseline is reserved.");
    keys.add(candidate.key);
    if (own(candidate, "changes")) {
      configurationChanges(candidate.changes, pieces);
      if (configurations.has(candidate.changes)) throw Error("This exact saved configuration is already included.");
      configurations.add(candidate.changes);
      append(`,{"key":${JSON.stringify(candidate.key)},"changes":`); append(candidate.changes); append("}");
      continue;
    }
    const selection = frozenStudySelection(parse(candidate.selection, "Frozen material selection"));
    const identity = `${selection.expected_sha256}:${selection.allow_estimates === true}`;
    if (identities.has(identity)) throw Error("This frozen material and estimate policy is already included.");
    identities.add(identity);
    append(`,{"key":${JSON.stringify(candidate.key)},"changes":{"schema":"ftx-material-bindings/1",`);
    if (target.scope === "default") { append('"default":'); append(candidate.selection); }
    else {
      append('"pieces":{');
      target.ids.forEach((id, index) => { append(`${index ? "," : ""}${JSON.stringify(id)}:`); append(candidate.selection); });
      append("}");
    }
    append("}}");
  }
  append("]");
  if (design) {
    append(design.adaptive ? ',"design_adaptive":' : ',"design_grid":'); append(design.adaptive || design.grid);
    append(',"design_fit":{"fit_spec":'); append(design.fitSpec); append("}");
  }
  if (cutting) { append(',"cutting":'); append(rawCutting ? state.cutting : JSON.stringify(cutting)); }
  if (finiteStock) { append(',"finite_stock":'); append(state.finiteStock); }
  if (cuttingJob) { append(',"cutting_job":'); append(state.cuttingJob); }
  if (sewing) { append(',"sewing":'); append(state.sewing); }
  if (selection) { append(',"sewing_selection":'); append(state.sewingSelection); }
  if (purchasing) {
    append(',"purchasing":{"quotes":'); append(purchasing.quotes);
    append(',"market":'); append(purchasing.market); append("}");
  }
  if (own(state, "cutAndSewPurchasing")) append(`,"cut_and_sew_purchasing":${combinedPurchasing}`);
  if (massJson !== null) { append(',"assembly_mass":'); append(massJson); }
  append(`,"comparison":${JSON.stringify({ basis: state.basis, objectives: state.objectives,
    constraints, required_consumers: required, allow_mixed_protocols: false })}}`);
  return parts.join("");
}

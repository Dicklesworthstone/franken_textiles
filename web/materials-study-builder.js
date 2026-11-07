// Guided request authoring only. Rust still owns material admission, planning and comparison.
export const MAX_GUIDED_STUDY_CANDIDATES = 7;
const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const own = (value, key) => Object.hasOwn(value, key);

// Presence enables guided criteria; the native consumer admits the complete source policy.
export function hasTextileWaterPolicy(surface) {
  return surface?.surface_water != null || surface?.surface_water_alternatives != null ||
    surface?.coupled_water_alternatives != null;
}

// Mechanical mass requires the source-qualified coupling, not just a thermal water film.
export function hasTextileWaterMechanicsPolicy(surface) {
  return (surface?.surface_water != null && surface?.mechanical_coupling != null) ||
    surface?.coupled_water_alternatives != null;
}
const TEXTILE_INERTIAL_MASS_METRICS = new Set([
  "textile_water_final_inertial_mass_kg", "textile_water_max_sample_inertial_mass_kg",
]);

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
  textile_water_final_vapor_pressure_pa: "transient_surface", textile_water_min_sample_vapor_pressure_pa: "transient_surface",
  textile_water_max_sample_vapor_pressure_pa: "transient_surface", textile_water_remaining_vapor_mass_kg: "transient_surface",
  textile_water_isothermal_heat_removed_j: "transient_surface",
  textile_water_final_inertial_mass_kg: "transient_surface", textile_water_max_sample_inertial_mass_kg: "transient_surface",
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
function temperature(value, label) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < -273.15) {
    throw Error(`${label} must be finite and at least -273.15 °C.`);
  }
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

/** Author a prescribed environment; Rust still admits each selected material's domain and optical face. */
export function buildMaterialExteriorExchange(exterior) {
  const fields = ["air_c", "convection_w_m2_k", "surface_temperature_range_c", "convection_source",
    "convection_evidence", "convection_conditions"];
  object(exterior, [...fields, "allow_estimates", "radiation"], fields, "Qualified exterior exchange");
  temperature(exterior.air_c, "Exterior air temperature");
  nonnegative(exterior.convection_w_m2_k, "Prescribed convection coefficient");
  const range = exterior.surface_temperature_range_c;
  if (!Array.isArray(range) || range.length !== 2) throw Error("Declare both limits of the exterior surface-temperature range.");
  for (const value of range) temperature(value, "Exterior surface-temperature limit");
  if (range[0] > range[1] || !Number.isFinite(range[1] - range[0])) {
    throw Error("Exterior surface-temperature limits must be ordered with a finite span.");
  }
  const boundedName = (value, label) => {
    name(value, label);
    bytes(value, 4096, `${label} must not exceed 4096 UTF-8 bytes.`);
  };
  const sourceFields = ["reference", "accessed_on", "note"];
  const source = object(exterior.convection_source, sourceFields, sourceFields, "Convection source");
  for (const key of sourceFields) boundedName(source[key], `Convection source ${key}`);
  const date = /^(\d{4})-(\d{2})-(\d{2})$/.exec(source.accessed_on);
  const year = Number(date?.[1]), month = Number(date?.[2]), day = Number(date?.[3]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const monthDays = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (!date || year < 1 || month < 1 || month > 12 || day < 1 || day > monthDays[month - 1]) {
    throw Error("Convection source date must be a valid YYYY-MM-DD calendar date.");
  }
  boundedName(exterior.convection_conditions, "Convection applicability conditions");
  if (!["measured", "published", "estimate"].includes(exterior.convection_evidence)) {
    throw Error("Convection evidence must be measured, published or estimate.");
  }
  if (own(exterior, "allow_estimates") && typeof exterior.allow_estimates !== "boolean") {
    throw Error("Convection estimate consent must be an explicit boolean.");
  }
  if (exterior.convection_evidence === "estimate" && (!own(exterior, "allow_estimates") || exterior.allow_estimates !== true)) {
    throw Error("Estimated convection requires explicit estimate consent.");
  }
  const result = { ...exterior, surface_temperature_range_c: [...range], convection_source: { ...source } };
  if (own(exterior, "radiation") && exterior.radiation !== null) {
    const fields = ["ambient_c", "emissivity_property", "exposed_face", "automatic_material_face", "allow_opaque_gray_approximation"];
    const radiation = object(exterior.radiation, fields, ["ambient_c", "allow_opaque_gray_approximation"], "Ambient radiation");
    temperature(radiation.ambient_c, "Radiant enclosure temperature");
    if (radiation.allow_opaque_gray_approximation !== true) throw Error("Radiation requires explicit opaque-gray enclosure consent.");
    if (own(radiation, "automatic_material_face") && typeof radiation.automatic_material_face !== "boolean") {
      throw Error("Automatic material-face selection must be an explicit boolean.");
    }
    const automatic = own(radiation, "automatic_material_face") && radiation.automatic_material_face;
    for (const key of ["emissivity_property", "exposed_face"]) {
      if (automatic) {
        if (own(radiation, key) && radiation[key] !== "") throw Error("Automatic material-face selection cannot also prescribe an emissivity property or face.");
      } else {
        if (!own(radiation, key)) throw Error("Radiation requires an explicit emissivity property and exposed face, or automatic material-face selection.");
        boundedName(radiation[key], `Radiation ${key}`);
      }
    }
    result.radiation = { ...radiation };
  }
  return result;
}

function exteriorBoundary(boundary, exterior) {
  if (boundary.outside_resistance_m2_k_w !== 0 || boundary.outside_c !== exterior.air_c) {
    throw Error("Exterior exchange requires zero outside film resistance and outside_c equal to air_c.");
  }
}

/** One explicit boundary broadcasts over the current native material columns; no old model lock is inferred. */
export function buildMaterialSurfaceConditions(boundary, exterior = null) {
  const fields = ["inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w"];
  object(boundary, fields, fields, "Uniform sheet heat conditions");
  for (const key of ["inside_c", "outside_c"]) temperature(boundary[key], `Sheet heat ${key}`);
  for (const key of ["inside_resistance_m2_k_w", "outside_resistance_m2_k_w"]) nonnegative(boundary[key], `Sheet heat ${key}`);
  const conditions = { schema: "ftx-steady-surface-conditions/1", boundaries: [boundary] };
  if (exterior !== null) {
    const qualified = buildMaterialExteriorExchange(exterior);
    exteriorBoundary(boundary, qualified);
    conditions.exterior_exchange = [qualified];
  }
  return JSON.stringify(conditions);
}

/** Author shared coverage topology without requiring or inferring a steady heat boundary. */
export function buildMaterialCoverageTopology(state) {
  const fields = ["bodyTriangleIds", "instanceLayers", "settings"];
  object(state, fields, fields, "Guided body/layer topology");
  if (typeof state.bodyTriangleIds !== "string" || !state.bodyTriangleIds.trim()) {
    throw Error("Declare body triangle IDs or inclusive ranges, such as 0-15, 22, 31-33.");
  }
  let size = bytes(state.bodyTriangleIds);
  const ids = [], seen = new Set();
  const tokens = state.bodyTriangleIds.trim().split(/[,\s]+/, 4097);
  if (tokens.length > 4096) throw Error("Declare at most 4096 body triangle IDs or ranges.");
  for (const token of tokens) {
    const match = /^(\d+)(?:-(\d+))?$/.exec(token);
    if (!match) throw Error("Body triangle entries must be nonnegative integer IDs or inclusive ranges without spaces around the hyphen.");
    const first = Number(match[1]), last = Number(match[2] ?? match[1]), count = last - first + 1;
    if (!Number.isSafeInteger(first) || !Number.isSafeInteger(last) || first < 0 || last < first || count > 4096 - ids.length) {
      throw Error("Declare at most 4096 body triangle IDs, using increasing safe-integer ranges.");
    }
    for (let offset = 0; offset < count; offset++) {
      const id = first + offset;
      if (seen.has(id)) throw Error("Body triangle IDs and ranges must not overlap or repeat.");
      seen.add(id); ids.push(id);
    }
  }
  const retainName = (value, label) => {
    name(value, label);
    bytes(value, 4096, `${label} must not exceed 4096 UTF-8 bytes.`);
    size += bytes(JSON.stringify(value));
    if (size > MAX_REQUEST_BYTES) throw Error("Guided body/layer heat exceeds 16 MiB.");
    return value;
  };
  if (!Array.isArray(state.instanceLayers) || !state.instanceLayers.length || state.instanceLayers.length > 65536) {
    throw Error("Save 1–65536 exact physical-instance layer assignments.");
  }
  const layers = new Map();
  for (const row of state.instanceLayers) {
    object(row, ["instance", "layer"], ["instance", "layer"], "Coverage layer assignment");
    retainName(row.instance, "Physical instance name"); retainName(row.layer, "Physical layer name");
    if (layers.has(row.instance)) throw Error("Each physical instance needs exactly one layer assignment.");
    layers.set(row.instance, row.layer);
  }
  const settingFields = ["max_distance_m", "min_normal_cosine", "separation_tolerance_m", "cells_per_layer", "interfaces"];
  const settings = object(state.settings, settingFields, settingFields, "Coverage geometry settings");
  for (const key of ["max_distance_m", "min_normal_cosine", "separation_tolerance_m"]) {
    if (typeof settings[key] !== "number" || !Number.isFinite(settings[key]) || settings[key] <= 0) {
      throw Error(`Coverage ${key} must be finite and positive.`);
    }
  }
  if (settings.min_normal_cosine > 1 || settings.separation_tolerance_m >= settings.max_distance_m) {
    throw Error("Coverage normal cosine must not exceed 1; separation tolerance must be smaller than the search distance.");
  }
  if (!Number.isSafeInteger(settings.cells_per_layer) || settings.cells_per_layer < 1 || settings.cells_per_layer > 64) {
    throw Error("Coverage needs 1–64 through-thickness cells per layer.");
  }
  if (!Array.isArray(settings.interfaces) || settings.interfaces.length > 4096) throw Error("Coverage permits at most 4096 directed interfaces.");
  const names = new Set(layers.values()), pairs = new Set();
  for (const item of settings.interfaces) {
    const fields = ["inner", "outer", "resistance_m2_k_w", "basis"];
    object(item, fields, fields, "Coverage interface");
    for (const key of ["inner", "outer", "basis"]) retainName(item[key], `Interface ${key}`);
    nonnegative(item.resistance_m2_k_w, "Interface resistance");
    const key = JSON.stringify([item.inner, item.outer]);
    if (item.inner === item.outer || !names.has(item.inner) || !names.has(item.outer) || pairs.has(key)) {
      throw Error("Each directed interface must uniquely join two different saved layer names.");
    }
    pairs.add(key);
  }
  const request = JSON.stringify({ body_triangle_ids: ids, instance_layers: Object.fromEntries(layers), settings });
  bytes(request, MAX_REQUEST_BYTES, "Guided body/layer heat exceeds 16 MiB.");
  return request;
}

/** Author steady coverage inputs; the native projection determines actual overlap and layer order. */
export function buildMaterialCoverageRequest(state) {
  const fields = ["bodyTriangleIds", "instanceLayers", "settings", "boundary"];
  object(state, [...fields, "exterior", "bareBoundary"], fields, "Guided body/layer heat");
  const topology = JSON.parse(buildMaterialCoverageTopology({ bodyTriangleIds: state.bodyTriangleIds,
    instanceLayers: state.instanceLayers, settings: state.settings }));
  const exterior = own(state, "exterior") ? state.exterior : null;
  const { schema, ...conditions } = JSON.parse(buildMaterialSurfaceConditions(state.boundary, exterior));
  if (exterior != null) {
    if (!own(state, "bareBoundary")) throw Error("Covered exterior exchange requires explicit bare-body film conditions.");
    conditions.bare_boundaries = JSON.parse(buildMaterialSurfaceConditions(state.bareBoundary)).boundaries;
  } else if (own(state, "bareBoundary") && state.bareBoundary != null) {
    throw Error("Separate bare-body films require covered exterior exchange.");
  }
  const request = JSON.stringify({ ...topology, conditions });
  bytes(request, MAX_REQUEST_BYTES, "Guided body/layer heat exceeds 16 MiB.");
  return request;
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

function motionLiquidSources(sources) {
  const fields = ["surface_water", "surface_water_alternatives", "coupled_water_alternatives"];
  object(sources, [...fields, "mechanical_coupling"], [], "Original textile-water policies");
  const active = fields.filter(key => own(sources, key));
  if (active.length !== 1 || (own(sources, "mechanical_coupling") && active[0] !== "surface_water")) {
    throw Error("Choose one direct water policy, one uncoupled alternative collection or one paired collection; separate mechanics requires direct water.");
  }
  for (const [field, raw] of Object.entries(sources)) {
    if (field === "surface_water" || field === "mechanical_coupling") {
      sourceObject(raw, `Original ${field} policy`);
    } else {
      const entries = parse(raw, `Original ${field} collection`);
      if (!Array.isArray(entries) || !entries.length || entries.length > 64 ||
        entries.some(entry => !entry || typeof entry !== "object" || Array.isArray(entry))) {
        throw Error(`Original ${field} must be a collection of 1–64 complete qualifications.`);
      }
      if (field === "coupled_water_alternatives") for (const entry of entries) {
        object(entry, ["surface_water", "mechanical_coupling"], ["surface_water", "mechanical_coupling"], "Paired water qualification");
        for (const key of ["surface_water", "mechanical_coupling"]) {
          if (!entry[key] || typeof entry[key] !== "object" || Array.isArray(entry[key])) {
            throw Error("Each paired qualification needs complete water and mechanical objects.");
          }
        }
      }
    }
  }
  // Material locks, source evidence, physical domains and interval schedules remain native checks.
  // Parsed objects are never serialized back over the original qualification text.
}

/** Author prescribed endpoints around exact liquid policies; Rust owns physical admission and evolution. */
export function buildMaterialMotionRequest(state, liquid = undefined) {
  object(state, ["keyframes", "max_step_s", "max_trials", "max_pairs", "max_sample_vertices", "transient_surface", "transient_coverage"],
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
  const heatingFields = ["plastic_dissipation_heat", "relaxation_dissipation_heat", "bending_plastic_dissipation_heat"];
  for (const [consumer, coverage] of [["transient_surface", false], ["transient_coverage", true]]) {
    if (!own(state, consumer)) continue;
    const label = coverage ? "Guided moving coverage heat" : "Guided sheet heat";
    const fields = ["schema", "initial_temperature_c", "boundaries", "max_step_s", "max_cell_steps",
      ...(coverage ? ["body_triangle_ids", "instance_layers", "settings", "exposed_boundaries", "max_geometry_operations"] : ["cells_per_layer"])];
    const heat = object(state[consumer], [...fields, ...heatingFields, "initial_instance_temperatures_c", "exterior_exchange",
      ...(coverage ? ["bare_boundaries", "exposed_exterior_exchange"] : ["initial_instance_temperature_profiles_c"])], fields, label);
    const schema = coverage ? "ftx-motion-coverage-heat/1" : "ftx-motion-surface-heat/1";
    if (heat.schema !== schema) throw Error(`${label} needs ${schema}.`);
    for (const field of heatingFields) {
      if (own(heat, field) && heat[field] !== null && heat[field] !== "uniform_reference_thickness") {
        throw Error(`${label} ${field} needs uniform_reference_thickness, null or omission.`);
      }
    }
    // The engine qualifies each selected material law, conversion observation and actual temperature.
    // These opt-ins never author a fraction or replace material-source admission.
    if (coverage) {
      if (!Array.isArray(heat.body_triangle_ids) || !heat.body_triangle_ids.length || heat.body_triangle_ids.length > 4096 ||
        heat.body_triangle_ids.some(id => !Number.isSafeInteger(id) || id < 0)) {
        throw Error("Moving coverage needs 1–4096 distinct nonnegative safe-integer body triangle IDs.");
      }
      if (!heat.instance_layers || typeof heat.instance_layers !== "object" || Array.isArray(heat.instance_layers)) {
        throw Error("Moving coverage needs an object of exact physical-instance layer assignments.");
      }
      const entries = Object.entries(heat.instance_layers);
      if (!entries.length || entries.length > 65536) throw Error("Save 1–65536 exact physical-instance layer assignments.");
      // Both consumers use the same authored geometry contract. Original motion files bypass
      // this guided subset and retain advanced source-qualified interfaces, bridges and liquid policies.
      buildMaterialCoverageTopology({ bodyTriangleIds: heat.body_triangle_ids.join(","),
        instanceLayers: entries.map(([instance, layer]) => ({ instance, layer })), settings: heat.settings });
    }
    temperature(heat.initial_temperature_c, coverage ? "Initial coverage temperature" : "Initial sheet temperature");
    if (!coverage && own(heat, "initial_instance_temperature_profiles_c")) {
      if (own(heat, "initial_instance_temperatures_c")) {
        throw Error("Choose uniform instance temperatures or complete through-thickness profiles, not both.");
      }
      const profiles = heat.initial_instance_temperature_profiles_c;
      if (!profiles || typeof profiles !== "object" || Array.isArray(profiles)) {
        throw Error("Initial temperature profiles must be an object keyed by exact physical instance names.");
      }
      const entries = Object.entries(profiles);
      if (!entries.length || entries.length > 65536) throw Error("Initial temperature profiles need 1–65536 explicit instances.");
      let size = 2;
      for (const [instance, values] of entries) {
        name(instance, "Initial-profile instance");
        if (!Array.isArray(values) || !Number.isSafeInteger(heat.cells_per_layer) || heat.cells_per_layer < 1
          || heat.cells_per_layer > 64 || values.length !== heat.cells_per_layer) {
          throw Error("Each initial temperature profile needs exactly cells_per_layer values, from inside to outside.");
        }
        for (const [cell, value] of values.entries()) {
          temperature(value, `Initial cell ${cell + 1} for ${instance}`);
        }
        size += bytes(JSON.stringify(instance)) + bytes(JSON.stringify(values)) + 2;
        if (size > MAX_REQUEST_BYTES) throw Error("Guided motion request exceeds 16 MiB.");
      }
      // The native retained mesh requires complete instance coverage and qualifies every cell.
    }
    if (own(heat, "initial_instance_temperatures_c")) {
      const values = heat.initial_instance_temperatures_c;
      if (!values || typeof values !== "object" || Array.isArray(values)) {
        throw Error("Initial instance temperatures must be an object of exact physical instance names and temperatures.");
      }
      const entries = Object.entries(values);
      if (!entries.length || entries.length > 65536) throw Error("Initial instance temperatures need 1–65536 explicit entries.");
      let size = 2;
      for (const [instance, value] of entries) {
        name(instance, "Initial-temperature instance"); temperature(value, `Initial temperature for ${instance}`);
        size += bytes(JSON.stringify(instance)) + bytes(JSON.stringify(value)) + 2;
        if (size > MAX_REQUEST_BYTES) throw Error("Guided motion request exceeds 16 MiB.");
      }
      // Native retained-instance ownership and each material's temperature domain decide admission.
      // The scalar remains an explicit fallback; neither source-piece IDs nor material IDs are expanded.
    }
    for (const [key, maximum] of [["max_cell_steps", 10000000],
      ...(coverage ? [["max_geometry_operations", 20000000]] : [["cells_per_layer", 64]])]) {
      if (!Number.isSafeInteger(heat[key]) || heat[key] < 1 || heat[key] > maximum) {
        throw Error(`${label} ${key} must be an integer from 1 to ${maximum}.`);
      }
    }
    if (typeof heat.max_step_s !== "number" || !Number.isFinite(heat.max_step_s) || heat.max_step_s < 1e-9 || heat.max_step_s > 1) {
      throw Error("Maximum thermal substep must be finite and from 1e-9 to 1 second.");
    }
    const filmFields = ["inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w"];
    const filmSchedules = ["boundaries", ...(coverage ? ["exposed_boundaries"] : [])];
    if (coverage && own(heat, "bare_boundaries") && heat.bare_boundaries !== null) filmSchedules.push("bare_boundaries");
    for (const field of filmSchedules) {
      if (!Array.isArray(heat[field]) || heat[field].length !== state.keyframes.length) {
        throw Error(`${label} needs one saved ${field} entry for each motion endpoint interval.`);
      }
      for (const film of heat[field]) {
        if (field !== "boundaries" && film === null) continue;
        object(film, filmFields, filmFields, `Saved ${label} ${field}`);
        temperature(film.inside_c, "Inside reservoir temperature"); temperature(film.outside_c, "Outside reservoir temperature");
        nonnegative(film.inside_resistance_m2_k_w, "Inside film resistance");
        nonnegative(film.outside_resistance_m2_k_w, "Outside film resistance");
      }
    }
    for (const field of coverage ? ["exterior_exchange", "exposed_exterior_exchange"] : ["exterior_exchange"]) {
      if (!own(heat, field) || heat[field] === null) continue;
      if (!Array.isArray(heat[field]) || heat[field].length !== state.keyframes.length) {
        throw Error(`${label} needs one saved ${field} entry for each motion endpoint interval.`);
      }
      for (const [index, value] of heat[field].entries()) {
        if (coverage && value === null) continue;
        const exterior = buildMaterialExteriorExchange(value);
        const film = field === "exposed_exterior_exchange" ? heat.exposed_boundaries[index] : heat.boundaries[index];
        if (film === null) throw Error("Exposed exterior exchange requires explicit exposed film conditions in that interval.");
        exteriorBoundary(film, exterior);
      }
    }
    if (coverage) {
      const exterior = own(heat, "exterior_exchange") ? heat.exterior_exchange : null;
      const bare = own(heat, "bare_boundaries") ? heat.bare_boundaries : null;
      for (let i = 0; i < state.keyframes.length; i++) {
        if ((exterior?.[i] != null) !== (bare?.[i] != null)) {
          throw Error("Each covered exterior interval requires explicit bare-body films; film-only intervals must leave bare_boundaries null.");
        }
      }
    }
  }
  let request;
  if (liquid === undefined) request = JSON.stringify({ schema: "ftx-drape-motion/1", ...state });
  else {
    if (!own(state, "transient_surface")) throw Error("Original textile-water policies require authored independent-sheet heat.");
    motionLiquidSources(liquid);
    const heat = JSON.stringify(state.transient_surface).slice(0, -1) +
      Object.entries(liquid).map(([key, raw]) => `,${JSON.stringify(key)}:${raw}`).join("") + "}";
    request = "{" + Object.entries({ schema: "ftx-drape-motion/1", ...state }).map(([key, value]) =>
      `${JSON.stringify(key)}:${key === "transient_surface" ? heat : JSON.stringify(value)}`).join(",") + "}";
  }
  bytes(request, MAX_REQUEST_BYTES, "Guided motion request exceeds 16 MiB.");
  return request;
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
    "motion", "motionLiquid", "maxMotionTrials", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage"],
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
      if (own(state, "motionLiquid")) throw Error("Original motion requests retain their complete policies; choose authored motion to compose separate liquid sources.");
      motionPolicy(sourceObject(state.motion, "Prescribed motion"));
      motion = state.motion;
    } else motion = buildMaterialMotionRequest(state.motion, state.motionLiquid);
    if (!own(state, "maxMotionTrials")) {
      throw Error("Prescribed motion and its explicit integer scenario motion-trial budget must be supplied together.");
    }
    if (!Number.isSafeInteger(state.maxMotionTrials) || state.maxMotionTrials < 1 || state.maxMotionTrials > 32768) {
      throw Error("Prescribed motion requires an explicit integer scenario motion-trial budget from 1 to 32768.");
    }
  } else if (own(state, "maxMotionTrials")) {
    throw Error("A scenario motion-trial budget requires a prescribed motion request; both must be supplied together.");
  }
  if (own(state, "motionLiquid") && motion === undefined) throw Error("Original textile-water policies require authored motion and independent-sheet heat.");
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
      textileWaterMechanics: hasTextileWaterMechanicsPolicy(scenario.motion?.transient_surface),
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

// The selected-body adapter reports these canonical tape quantities in millimetres.
export const GUIDED_FIT_BODY_MEASUREMENTS = Object.freeze([
  "stature", "chest", "waist", "hip", "waist_height", "crotch_height", "thigh", "knee", "calf", "ankle",
  "upper_arm", "wrist", "arm_length", "knee_height", "back_neck_to_waist", "neck_base", "head",
  "shoulder_length", "across_back", "total_crotch_length", "outseam",
]);

/** Author explicit canonical targets; actual geometry, body readings and fit admission stay in Rust. */
export function buildGuidedFitSpec(points) {
  if (!Array.isArray(points) || !points.length || points.length > 32) throw Error("Save 1–32 explicit fit targets.");
  const ids = new Set();
  const retained = Array.from(points, point => {
    const common = ["id", "kind", "body", "target_mm"], optional = ["label", "body_scale", "body_offset_mm"];
    const fields = { girth: ["instances", "landmark", "dy"], width: ["instance", "at_vertex", "landmark", "dy"],
      vertical: ["instance", "from", "to"], path: ["refs", "scale"] };
    if (!own(fields, point?.kind)) throw Error("Choose a girth, width, vertical or path fit target.");
    object(point, [...common, ...optional, ...fields[point.kind]], common, "Fit target");
    name(point.id, "Fit target name");
    if (ids.has(point.id)) throw Error("Fit target names must be unique.");
    ids.add(point.id);
    if (!GUIDED_FIT_BODY_MEASUREMENTS.includes(point.body)) throw Error("Choose a supported body measurement in millimetres.");
    if (own(point, "label")) name(point.label, "Fit target label");
    const target = point.target_mm;
    if (!Array.isArray(target) || target.length !== 3 || Array.from(target).some(value => typeof value !== "number" || !Number.isFinite(value)) ||
      target[0] > target[1] || target[1] > target[2]) throw Error("Ease targets need finite minimum ≤ nominal ≤ maximum values in millimetres.");
    for (const key of ["body_scale", "body_offset_mm", "dy", "scale"]) if (own(point, key) &&
      (typeof point[key] !== "number" || !Number.isFinite(point[key]))) throw Error("Fit target " + key + " must be finite.");
    if (point.kind === "girth" || point.kind === "path") {
      const key = point.kind === "girth" ? "instances" : "refs", selectors = point[key];
      if (!Array.isArray(selectors) || !selectors.length || selectors.length > 64) throw Error("A fit target needs 1–64 explicit instance or path references.");
      for (const value of selectors) name(value, "Fit target reference");
      if (point.kind === "girth") name(point.landmark, "Measuring-line landmark");
    } else {
      name(point.instance, "Physical instance");
      if (point.kind === "vertical") {
        name(point.from, "Starting vertex"); name(point.to, "Ending vertex");
      } else {
        if (own(point, "at_vertex") === own(point, "landmark")) throw Error("A width target needs exactly one vertex or landmark for its measuring line.");
        name(point.at_vertex ?? point.landmark, "Measuring-line vertex or landmark");
      }
    }
    return { ...point, target_mm: [...target], ...(point.instances ? { instances: [...point.instances] } : {}),
      ...(point.refs ? { refs: [...point.refs] } : {}) };
  });
  const raw = JSON.stringify({ schema: "fabric-fit/1", points: retained }, null, 2) + "\n";
  bytes(raw, 256 * 1024, "Authored fit contract exceeds 256 KiB.");
  return raw;
}

/** Validate explicitly authored additional starts against the same complete variable domains. */
export function buildGuidedDesignRestartPoints(points, variables) {
  if (!Array.isArray(points) || points.length > 31) throw Error("Choose at most 31 additional adaptive starting points.");
  if (!Array.isArray(variables) || !variables.length || variables.length > 3) throw Error("Additional starts need 1–3 declared variables.");
  const keys = [...uniqueNames(variables.map(variable => variable?.key), "Adaptive variable names", 3)];
  for (const variable of variables) adaptiveDomain(variable);
  // JSON uses the same identity for -0 and +0, matching the native coordinate cache.
  const seen = new Set([JSON.stringify(variables.map(variable => variable.initialMm))]);
  return Array.from(points, point => {
    object(point, keys, keys, "Additional adaptive start");
    const values = variables.map(variable => {
      const value = point[variable.key];
      if (typeof value !== "number" || !Number.isFinite(value) || value < variable.boundsMm[0] || value > variable.boundsMm[1]) {
        throw Error(`Additional start for ${variable.key} must be finite and inside its declared bounds.`);
      }
      return value;
    });
    const identity = JSON.stringify(values);
    if (seen.has(identity)) throw Error("Additional adaptive starts must be distinct from one another and the initial point.");
    seen.add(identity);
    return Object.fromEntries(keys.map((key, index) => [key, values[index]]));
  });
}

function adaptiveGuidance(design, candidates, comparison, matrix) {
  if (!["fit_band_violation_mm", "consumer_comparison", "consumer_pareto", "consumer_worst_case", "consumer_matrix_pareto"].includes(design.objective)) {
    throw Error("Choose fit-band, nominated-case, worst-case or Pareto adaptive guidance.");
  }
  if (own(design, "constraintRestoration")) {
    if (design.constraintRestoration !== "componentwise" || design.objective === "fit_band_violation_mm") {
      throw Error("Constraint restoration requires componentwise consumer guidance.");
    }
    const policies = matrix ? matrix.cases.map(item => item.comparison ?? comparison) : [comparison];
    if (!policies.some(policy => Array.isArray(policy?.constraints) && policy.constraints.length)) {
      throw Error("Constraint restoration needs at least one explicit comparison limit.");
    }
  }
  if (design.objective === "fit_band_violation_mm") {
    if (own(design, "materialKey") || own(design, "materialKeys") || own(design, "caseKey")) throw Error("Fit-band discovery does not accept guidance material keys or a caseKey.");
    return;
  }
  const severalMaterials = own(design, "materialKeys");
  if (severalMaterials && (own(design, "materialKey") ||
    !["consumer_pareto", "consumer_matrix_pareto"].includes(design.objective))) {
    throw Error("Multiple guidance materials require Pareto guidance and cannot also nominate materialKey.");
  }
  const materials = severalMaterials ? [...uniqueNames(design.materialKeys, "Adaptive guidance materialKeys", 16)] :
    [name(design.materialKey, "Adaptive guidance materialKey")];
  if (severalMaterials && materials.length < 2) throw Error("Choose 2–16 distinct materials to guide discovery.");
  if (materials.some(key => key !== "baseline" && !candidates.some(candidate => candidate?.key === key))) {
    throw Error("Adaptive materialKey/materialKeys must identify the unchanged baseline or existing material choices.");
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
          policies.reduce((count, policy) => count + policy.objectives.length, 0) < (severalMaterials ? 1 : 2)) {
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
  if (design.objective === "consumer_comparison" ? count !== 1 : count < (severalMaterials ? 1 : 2) || count > 16) {
    if (severalMaterials) throw Error("Multiple-material Pareto guidance needs 1–16 comparison objectives per material.");
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
  const allowed = adaptive ? [...required, "materialKey", "materialKeys", "caseKey", "restartPointsMm", "constraintRestoration"] : multiple ? [...fields, "mode", "maxCandidates"] : fields;
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
  const restartPoints = adaptive && own(design, "restartPointsMm") ? buildGuidedDesignRestartPoints(design.restartPointsMm, authored) : [];
  const keys = new Set();
  let totalPoints = 1, totalSites = after?.length ?? 0;
  const variables = Array.from(authored, value => {
    const fields = adaptive ? ["key", "boundsMm", "initialMm", "initialStepMm", "minimumStepMm", "sites"] : ["key", "values", "sites"];
    const variable = object(value, fields, fields, "Linked variable");
    name(variable.key, "Linked variable key");
    if (keys.has(variable.key)) throw Error("Linked variable keys must be unique.");
    keys.add(variable.key);
    const values = adaptive ? adaptiveDomain(variable) : linkedValues(variable.values);
    if (adaptive) values.push(...restartPoints.map(point => point[variable.key]));
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
    ...(own(design, "constraintRestoration") ? { constraint_restoration: design.constraintRestoration } : {}),
    ...(own(design, "materialKey") ? { material_key: design.materialKey } : {}),
    ...(own(design, "materialKeys") ? { material_keys: [...design.materialKeys] } : {}),
    ...(own(design, "caseKey") ? { case_key: design.caseKey } : {}), variables,
    ...(restartPoints.length ? { restart_points_mm: restartPoints } : {}),
    ...(after === undefined ? {} : { after }),
    max_evaluations: design.maxEvaluations, max_designs: design.maxDesigns } :
    { variables, ...(after === undefined ? {} : { after }), max_designs: design.maxDesigns, fit_screening: { max_candidates: maxCandidates } });
  bytes(search, 256 * 1024, `Linked design ${mode} exceeds 256 KiB of authored domain data.`);
  return { [adaptive ? "adaptive" : "grid"]: search, fitSpec: design.fitSpec, retainedDesigns: retained + 1,
    probes: adaptive && design.objective !== "fit_band_violation_mm" ? design.maxEvaluations * (design.materialKeys?.length ?? 1) : 0 };
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

function hasTextileWaterMechanics(value) {
  return hasTextileWater(value) && own(value, "textileWaterMechanics") && value.textileWaterMechanics === true;
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
      "motionVaporHistory", "motionWetFilm", "transientSurface", "transientCoverage", "textileWater", "textileWaterMechanics", "comparison"], ["key", "surface"], "Named scenario case");
    name(item.key, "Scenario case key");
    bytes(item.key, 128, "Scenario case keys must not exceed 128 UTF-8 bytes.");
    if (keys.has(item.key)) throw Error("Scenario case keys must be unique.");
    keys.add(item.key);
    if (typeof item.surface !== "boolean") throw Error("Each named case needs an explicit surface-request boolean.");
    for (const key of ["motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
      "transientSurface", "transientCoverage", "textileWater", "textileWaterMechanics"]) {
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
 * textileWaterMechanics additionally describes direct or paired mechanical qualification for wet mass criteria.
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
      "motionVaporHistory", "motionWetFilm", "transientSurface", "transientCoverage", "textileWater", "textileWaterMechanics", "matrix"], ["drape", "surface"], "Scenario comparison context");
    if (scenario.drape !== true || typeof scenario.surface !== "boolean") {
      throw Error("Scenario comparison needs explicit drape: true and a boolean surface request.");
    }
    for (const key of ["motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
      "transientSurface", "transientCoverage", "textileWater", "textileWaterMechanics"]) {
      if (own(scenario, key) && typeof scenario[key] !== "boolean") throw Error(`Scenario ${key} availability must be a boolean when supplied.`);
    }
    if (own(scenario, "matrix")) {
      matrix = matrixContext(scenario.matrix);
      for (const key of ["surface", "motion", "materialStrain", "contactLoads", "coverage", "vaporSurface", "vaporCoverage", "motionVaporHistory", "motionWetFilm",
        "transientSurface", "transientCoverage", "textileWater", "textileWaterMechanics"]) {
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
    { basis: state.basis, objectives: state.objectives, constraints: state.constraints }, matrix, state.seamIds) : null;
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
  const textileWaterMechanics = matrix ? matrix.cases.some(hasTextileWaterMechanics) : hasTextileWaterMechanics(scenario);
  const metric = (key, basis) => {
    if (typeof key !== "string" || !own(METRIC_CONSUMERS, key)) throw Error(`Unsupported guided comparison metric: ${key}.`);
    requireConsumer(METRIC_CONSUMERS[key]);
    if (key.startsWith("textile_water_") && !textileWater) {
      throw Error("Textile-water criteria require motion and an original transient-surface water policy in the same case.");
    }
    if (TEXTILE_INERTIAL_MASS_METRICS.has(key) && !textileWaterMechanics) {
      throw Error("Wet inertial-mass criteria require motion, textile water and its direct or paired mechanical coupling in the same case.");
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
    const needsTextileWaterMechanics = [...state.objectives, ...constraints].some(item => TEXTILE_INERTIAL_MASS_METRICS.has(item.metric));
    for (const item of matrix.cases) {
      if (item.comparison != null) continue;
      const missing = Object.entries(scenarioConsumers(item)).find(([consumer, available]) => needed.has(consumer) && !available);
      if (missing) throw Error(`Named case ${item.key} inherits a comparison requiring ${missing[0]} inputs it does not request.`);
      if (needsTextileWater && !hasTextileWater(item)) {
        throw Error(`Named case ${item.key} inherits a comparison requiring textile-water inputs it does not request.`);
      }
      if (needsTextileWaterMechanics && !hasTextileWaterMechanics(item)) {
        throw Error(`Named case ${item.key} inherits a comparison requiring textile-water mechanical coupling it does not request.`);
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

// Saved workbench inputs are a local replay envelope, not an admission or evaluation report.
export const MAX_WORKBENCH_STUDY_BYTES = 128 * 1024 * 1024;
const MAX_WORKBENCH_SOURCE_BYTES = 64 * 1024 * 1024;

function workbenchSource(raw, label, schemas) {
  if (typeof raw !== "string") throw Error(`${label} must retain its original JSON text.`);
  let value;
  try { value = JSON.parse(raw); }
  catch { throw Error(`${label} must be valid JSON.`); }
  if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${label} must be a JSON object.`);
  if (schemas && !schemas.includes(value.schema)) throw Error(`${label} has an unsupported schema.`);
  return value;
}

function workbenchStudyBundle(value) {
  object(value, ["schema", "request", "body_model"], ["schema", "request"], "Saved study");
  if (value.schema !== "ftx-workbench-study/1") throw Error("Choose an ftx-workbench-study/1 saved study.");
  const request = object(value.request, ["type", "pattern", "studyRequest", "onBody", "scenarioRequest",
    "measurements", "measurementSources", "measurementConsent", "measurementPrimary", "measurementAcceptedFit",
    "measurementArmDeg", "measurementPose", "outfit"], ["type", "pattern", "studyRequest"], "Saved study request");
  if (!["study", "scenario"].includes(request.type)) throw Error("Saved inputs must describe a manufacturing or scenario study.");
  if (own(request, "onBody") && (request.type !== "study" || typeof request.onBody !== "boolean")) {
    throw Error("Only manufacturing studies declare onBody, as a boolean.");
  }
  if (own(request, "outfit")) object(request.outfit, ["request_json", "composition_json"],
    ["request_json", "composition_json"], "Saved outfit");
  let total = 0;
  for (const raw of [...Object.values(request), ...Object.values(request.outfit || {})]) {
    if (typeof raw === "string") total += bytes(raw, MAX_WORKBENCH_SOURCE_BYTES, "Combined saved study inputs exceed 64 MiB.");
    if (total > MAX_WORKBENCH_SOURCE_BYTES) throw Error("Combined saved study inputs exceed 64 MiB.");
  }
  workbenchSource(request.pattern, "Original pattern", ["fabric-pattern/1"]);
  workbenchSource(request.studyRequest, "Original study request", ["ftx-material-study/1", "ftx-design-study/1"]);
  if (request.type === "scenario") {
    workbenchSource(request.scenarioRequest, "Original scenario request", ["ftx-drape-scenario/1", "ftx-drape-scenario-matrix/1"]);
  } else if (own(request, "scenarioRequest")) throw Error("Manufacturing studies cannot contain an unused scenario request.");
  const onBody = request.type === "scenario" || request.onBody === true;
  if (onBody) {
    workbenchSource(request.measurements, "Original body input");
    const model = object(value.body_model, ["sha256", "byte_length"], ["sha256", "byte_length"], "Saved body model identity");
    if (typeof model.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(model.sha256) || !Number.isSafeInteger(model.byte_length)
      || model.byte_length < 1 || model.byte_length > 256 * 1024 * 1024) {
      throw Error("Saved body model identity needs a lowercase SHA-256 and a byte length from 1 to 256 MiB.");
    }
  } else if (own(value, "body_model") || Object.keys(request).some(key => key.startsWith("measurement"))) {
    throw Error("Body-independent manufacturing studies cannot contain unused body inputs.");
  }
  if (own(request, "measurementSources")) {
    if (request.measurementConsent !== true) throw Error("Associate additional measurements with this same wearer explicitly.");
    if (typeof request.measurementSources !== "string") throw Error("Additional measurements must retain their original JSON text strings.");
    let sources;
    try { sources = JSON.parse(request.measurementSources); }
    catch { throw Error("Additional measurement sources must be valid JSON."); }
    if (!Array.isArray(sources) || !sources.length || sources.length > 8) throw Error("Choose 1–8 additional measurement sources.");
    let size = 0;
    for (const raw of sources) {
      if (typeof raw !== "string") throw Error("Additional measurements must retain their original JSON text strings.");
      size += bytes(raw, MAX_REQUEST_BYTES, "Additional measurements exceed 16 MiB.");
      if (size > MAX_REQUEST_BYTES) throw Error("Additional measurements exceed 16 MiB.");
      workbenchSource(raw, "Additional measurement source");
    }
    if (own(request, "measurementPrimary")) workbenchSource(request.measurementPrimary, "Original primary measurements");
    if (own(request, "measurementAcceptedFit")) {
      workbenchSource(request.measurementAcceptedFit, "Original accepted body fit", ["ftx-body-fit/1"]);
      bytes(request.measurementAcceptedFit, MAX_REQUEST_BYTES, "Accepted body-fit report exceeds 16 MiB.");
      if (!own(request, "measurementPrimary") || !Number.isFinite(request.measurementArmDeg)
        || request.measurementArmDeg < 0 || request.measurementArmDeg > 180) {
        throw Error("Supplemental accepted-fit replay requires original primary measurements and a valid arm angle.");
      }
    } else if (own(request, "measurementArmDeg")) throw Error("Supplemental arm metadata requires an accepted body-fit source.");
    if (own(request, "measurementPose")) {
      const pose = object(request.measurementPose, ["arm_abduction_deg", "legs", "leg_fraction"],
        ["arm_abduction_deg", "legs", "leg_fraction"], "Supplemental preview pose");
      if (!own(request, "measurementPrimary") || own(request, "measurementAcceptedFit")
        || !Number.isFinite(pose.arm_abduction_deg) || pose.arm_abduction_deg < 0 || pose.arm_abduction_deg > 180
        || !["standing", "seated", "squat"].includes(pose.legs) || !Number.isFinite(pose.leg_fraction)
        || pose.leg_fraction < 0 || pose.leg_fraction > 1) {
        throw Error("Supplemental preview pose needs original primary measurements, valid arm and leg settings, and no accepted fit.");
      }
    }
  } else if (["measurementConsent", "measurementPrimary", "measurementAcceptedFit", "measurementArmDeg", "measurementPose"].some(key => own(request, key))) {
    throw Error("Supplemental replay metadata requires original additional measurement sources.");
  }
  if (own(request, "outfit")) {
    workbenchSource(request.outfit.request_json, "Original outfit request", ["ftx-outfit-drape/1"]);
    workbenchSource(request.outfit.composition_json, "Original outfit composition", ["ftx-outfit-composition/1"]);
  }
  const frozenRequest = { ...request };
  if (request.measurementPose) frozenRequest.measurementPose = Object.freeze({ ...request.measurementPose });
  if (request.outfit) frozenRequest.outfit = Object.freeze({ ...request.outfit });
  return Object.freeze({ schema: value.schema, request: Object.freeze(frozenRequest),
    ...(onBody ? { body_model: Object.freeze({ ...value.body_model }) } : {}) });
}

export function buildWorkbenchStudyBundle(request, bodyModel) {
  const saved = workbenchStudyBundle({ schema: "ftx-workbench-study/1", request,
    ...(bodyModel !== undefined ? { body_model: bodyModel } : {}) });
  const raw = JSON.stringify(saved);
  bytes(raw, MAX_WORKBENCH_STUDY_BYTES, "Saved study exceeds 128 MiB.");
  return raw;
}

export function parseWorkbenchStudyBundle(raw) {
  if (typeof raw !== "string") throw Error("Saved study must be original JSON text.");
  bytes(raw, MAX_WORKBENCH_STUDY_BYTES, "Saved study exceeds 128 MiB.");
  let saved;
  try { saved = JSON.parse(raw); }
  catch { throw Error("Saved study must be valid JSON."); }
  return workbenchStudyBundle(saved);
}

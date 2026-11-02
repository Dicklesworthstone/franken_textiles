// Guided request authoring only. Rust still owns material admission, planning and comparison.
export const MAX_GUIDED_STUDY_CANDIDATES = 7;
const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const own = (value, key) => Object.hasOwn(value, key);
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
};

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

/** Author one fresh static drape; source files remain verbatim and Rust owns numerical admission. */
export function buildMaterialScenarioRequest(state) {
  object(state, ["fitSpec", "allowNoFit", "options", "maxMeshPoints", "maxVertexFrames", "surface"],
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
  const parts = ['{"schema":"ftx-drape-scenario/1","options":', JSON.stringify(options), ',"fit_spec":', fitSpec,
    ',"max_mesh_points":', String(state.maxMeshPoints), ',"max_vertex_frames":', String(state.maxVertexFrames),
    ...(own(state, "surface") ? [',"surface":', state.surface] : []), '}'];
  let size = 0;
  for (const part of parts) {
    size += bytes(part);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided scenario request exceeds 16 MiB.");
  }
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

function linkedDesign(value, pieces, materialCount) {
  const fields = ["pieceIds", "axis", "atMm", "multiplier", "values", "maxDesigns", "fitSpec"];
  const design = object(value, fields, fields, "Linked design");
  const selected = uniqueNames(design.pieceIds, "Altered source pieces", 16);
  if ([...selected].some(id => !pieces.has(id))) throw Error("Alteration names a missing source piece.");
  if (!["x", "y"].includes(design.axis) || typeof design.atMm !== "number" || !Number.isFinite(design.atMm) ||
    typeof design.multiplier !== "number" || !Number.isFinite(design.multiplier) || design.multiplier === 0) {
    throw Error("Linked alterations need an x/y axis, finite cut coordinate and finite nonzero multiplier.");
  }
  if (!Array.isArray(design.values) || !design.values.length || design.values.length > 64 ||
    Array.from(design.values).some((value, index) => typeof value !== "number" || !Number.isFinite(value) ||
      (index > 0 && design.values[index - 1] >= value))) {
    throw Error("Linked design needs 1–64 finite strictly increasing millimetre values.");
  }
  for (const value of design.values) {
    const displacement = value * design.multiplier;
    if (!Number.isFinite(displacement) || (value !== 0 && displacement === 0)) {
      throw Error("Linked design displacement overflows or underflows; no alteration is silently dropped.");
    }
  }
  if (!Number.isSafeInteger(design.maxDesigns) || design.maxDesigns < 1 || design.maxDesigns > 31) {
    throw Error("The passing-design allowance must be an integer from 1 to 31.");
  }
  if ((Math.min(design.values.length, design.maxDesigns) + 1) * materialCount > 32) {
    throw Error("Linked design exceeds 32 expanded design/material alternatives including both baselines; reduce its passing-design allowance or material shortlist.");
  }
  if (typeof design.fitSpec !== "string") throw Error("Design fit contract must retain its original JSON text.");
  bytes(design.fitSpec, 256 * 1024, "Design fit contract exceeds 256 KiB of UTF-8 source.");
  const fit = sourceObject(design.fitSpec, "Design fit contract");
  if ((own(fit, "schema") && fit.schema !== "fabric-fit/1") || !Array.isArray(fit.points) || !fit.points.length || fit.points.length > 32) {
    throw Error("Design fit needs 1–32 explicit points and fabric-fit/1 when a schema is supplied.");
  }
  const grid = JSON.stringify({ variables: [{ key: "linked_adjustment", values_mm: design.values,
    sites: design.pieceIds.map(piece => ({ piece, axis: design.axis, at_mm: design.atMm, multiplier: design.multiplier })) }],
    max_designs: design.maxDesigns, fit_screening: { max_candidates: design.values.length } });
  bytes(grid, 256 * 1024, "Linked design grid exceeds 256 KiB of authored domain data.");
  return { grid, fitSpec: design.fitSpec };
}

/**
 * Author an unchanged baseline and 1–7 frozen choices, optionally with a linked fit-screened grid.
 * `pieceIds` are the current pattern's source IDs. `target` is default or named pieces.
 * `cutting` uses authored numeric roll options or an exact original policy file. Optional
 * `finiteStock` and `cuttingJob` retain original inventory/job JSON instead of roll cutting.
 * sewing/selection and purchasing inputs always retain their original JSON texts.
 * `assemblyMass` is a canonical inventory
 * request (original JSON text for sourced extras, or an authored object with no extras).
 * Objectives, inclusive constraints and required consumers are explicit; no completeness or
 * protocol consent is inferred. Original source text is embedded verbatim, including locked numbers.
 * Optional scenario context describes the separately dispatched drape/heat request, is never
 * serialized into the study, and does not enable scenario consumers in manufacturing-only calls.
 * Optional `design` authors one shared slash-spread variable and retains the original fit contract;
 * its selected-body dispatch and canonical geometry/fit admission remain with the caller and Rust.
 */
export function buildMaterialStudyRequest(state, scenario) {
  if (scenario !== undefined) {
    object(scenario, ["drape", "surface"], ["drape", "surface"], "Scenario comparison context");
    if (scenario.drape !== true || typeof scenario.surface !== "boolean") {
      throw Error("Scenario comparison needs explicit drape: true and a boolean surface request.");
    }
  }
  object(state, ["candidates", "target", "pieceIds", "cutting", "finiteStock", "cuttingJob", "assemblyMass", "objectives", "basis",
    "sewing", "sewingSelection", "purchasing", "cutAndSewPurchasing", "constraints", "requiredConsumers", "design"],
    ["candidates", "target", "pieceIds", "objectives", "basis"], "Guided study");
  const pieces = uniqueNames(state.pieceIds, "Pattern source pieces", 4096);
  const target = object(state.target, ["scope", "ids"], ["scope"], "Material target");
  if (target.scope === "pieces") {
    if (!own(target, "ids")) throw Error("Choose source pieces for the material target.");
    const selected = uniqueNames(target.ids, "Selected source pieces", 4096);
    if ([...selected].some(id => !pieces.has(id))) throw Error("Material target names a missing source piece.");
  } else if (target.scope !== "default" || own(target, "ids")) throw Error("Choose a default or explicit piece target.");
  if (!Array.isArray(state.candidates) || !state.candidates.length || state.candidates.length > MAX_GUIDED_STUDY_CANDIDATES) {
    throw Error("Add 1–7 material choices; the unchanged baseline is included separately.");
  }
  if (!["nominal", "reported_bounds"].includes(state.basis)) throw Error("Choose nominal or reported-bound comparison.");
  const design = own(state, "design") ? linkedDesign(state.design, pieces, state.candidates.length + 1) : null;

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
    drape: !!scenario, surface: scenario?.surface === true,
  };
  const requireConsumer = consumer => {
    if (typeof consumer !== "string" || !own(consumers, consumer)) throw Error(`Unsupported guided consumer: ${consumer}.`);
    if (!consumers[consumer]) throw Error(`Comparison requires requested ${consumer} inputs.`);
  };
  const metric = (key, basis) => {
    if (typeof key !== "string" || !own(METRIC_CONSUMERS, key)) throw Error(`Unsupported guided comparison metric: ${key}.`);
    requireConsumer(METRIC_CONSUMERS[key]);
    if (key === "assembly_mass_kg" && !mass.inventory_complete) {
      throw Error("Assembly-mass comparison requires an explicit complete inventory declaration.");
    }
    if (basis === "reported_bounds" && key !== "assembly_mass_kg") {
      throw Error("Guided reported-bound comparison supports assembly mass only; use an explicit nominal basis for other guided metrics and guards.");
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

  const parts = []; let size = 0;
  const append = value => {
    size += bytes(value);
    if (size > MAX_REQUEST_BYTES) throw Error("Guided study request exceeds 16 MiB.");
    parts.push(value);
  };
  append(`{"schema":"${design ? "ftx-design-study/1" : "ftx-material-study/1"}","variants":[{"key":"baseline","changes":{"schema":"ftx-material-bindings/1"}}`);
  const keys = new Set(["baseline"]), identities = new Set();
  for (const candidate of state.candidates) {
    object(candidate, ["key", "selection"], ["key", "selection"], "Material choice");
    name(candidate.key, "Material choice name");
    if (keys.has(candidate.key)) throw Error("Material choice names must be unique; baseline is reserved.");
    keys.add(candidate.key);
    const selection = object(parse(candidate.selection, "Frozen material selection"),
      ["id", "record", "overrides", "sources", "allow_estimates", "expected_sha256"],
      ["id", "record", "expected_sha256"], "Frozen material selection");
    name(selection.id, "Material ID");
    if (!selection.record || typeof selection.record !== "object" || Array.isArray(selection.record)
      || !own(selection.record, "id") || selection.record.id !== selection.id
      || typeof selection.expected_sha256 !== "string" || !/^[0-9a-f]{64}$/.test(selection.expected_sha256)
      || (own(selection, "allow_estimates") && typeof selection.allow_estimates !== "boolean")) {
      throw Error("Inspect the material first to retain its matching record, source lock and estimate policy.");
    }
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
    append(',"design_grid":'); append(design.grid);
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

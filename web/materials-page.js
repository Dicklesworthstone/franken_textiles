// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const boundOperations = ["exports", "fold", "heat", "vaporSurface", "coverage", "cut", "stock", "price", "sew"];
const operations = [...boundOperations, "screen", "study", "scenario", "pressure", "treatment", "vapor", "calibrate",
  "stripAcquisition", "stripExperiment", "stripHistory", "stripFit", "outfit", "thermalPoint", "thermalHistory", "garmentcode"];
const selectionOperations = ["inspect", "pressure", "calibrate", "stripFit"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let preparedBy = null;
let preparedTreatment = null;
let preparedImport = null, adoptedImport = false;
let sourceText = null, outfit = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
let roundRead = 0, roundToken = new URL(window.location.href).searchParams.get("roundImport");
const roundFiles = new Map();
let interfaceChange = null, interfaceRead = 0, interfaceReady = true;
let calibratedPair = null;
const pieceChanges = new Map(), urls = new Map();
const combinationGroups = new Map();
const outfitTargets = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = ({ screen: "Source-qualified screening",
    study: "Fixed-pattern manufacturing alternatives", scenario: "Fresh drape and heat alternatives",
    pressure: "Pressure-conditioned material", treatment: "Laundering and dimensional treatment", vapor: "Potential evaporative transfer", calibrate: "Identified material behavior",
    thermalPoint: "Material at the stated temperature", thermalHistory: "Dry heating and cooling history",
    stripAcquisition: "Complete clamped-strip acquisitions", stripExperiment: "Clamped-strip experiment",
    stripHistory: "Strip loading and recovery", vaporSurface: "Vapor transfer on this drape",
    stripFit: "Clamped-strip identification", outfit: "Imported joint outfit", garmentcode: "Imported GarmentCode pattern — review required" })[op] || op;
  const summary = document.createElement("pre"), links = document.createElement("div"); links.className = "links";
  const comparison = document.createElement("div"); comparison.className = "comparison";
  section.append(title, summary, comparison, links); $("outputs").append(section);
  sections.set(op, { section, summary, comparison, links, study: null, adoptions: [] });
}
const text = value => JSON.stringify(value, null, 2);
const has = id => roundFiles.has(id) || !!$(id).files.length;
const studyBodyMode = () => $("studyBodySource").value || "measurements";
function studyBodyReady() {
  const mode = studyBodyMode();
  if (mode === "accepted") return has("bodyFitFile") && $("bodyFitConsent").checked;
  if (mode === "request") return has("scenarioBody");
  return mode === "measurements" && has("measurements");
}
function bodyDependentStudies(id) {
  const mode = studyBodyMode();
  const used = id === "body" || (id === "scenarioBody" && mode === "request") ||
    (id === "measurements" && (mode === "measurements" || mode === "accepted")) ||
    (["bodyFitFile", "bodyFitConsent", "bodyArmDeg"].includes(id) && mode === "accepted");
  return used ? ["scenario", ...($("studyOnBody").checked ? ["study"] : [])] : [];
}
function controls() {
  for (const el of commands) el.disabled = !ready || !!pending;
  for (const section of sections.values()) {
    for (const button of section.adoptions) button.disabled = !ready || !!pending;
  }
  $("bind").disabled ||= !source || !interfaceReady;
  $("assign").disabled = !!pending || !source || !inspected;
  $("reset").disabled = !!pending || !source;
  $("addCombination").disabled = !!pending || !source || !inspected;
  $("clearCombinations").disabled = !!pending || !combinationGroups.size;
  $("clearInterfaces").disabled = interfaceChange === null && interfaceReady;
  for (const op of boundOperations) $(op).disabled ||= !bound;
  $("screen").disabled ||= !has("screenRequest");
  $("pressure").disabled ||= !has("compressionRequest");
  $("treatment").disabled ||= !has("treatmentRequest");
  $("useTreatment").disabled = !!pending || !preparedTreatment;
  $("garmentcode").disabled ||= !has("garmentcodeSource");
  $("useGarmentcode").disabled = !!pending || !preparedImport;
  $("clearGarmentcodeAnnotations").disabled = !has("garmentcodeAnnotations");
  $("garmentcodeArcs").disabled = has("garmentcodeAnnotations");
  for (const id of ["garmentcodeArcSagitta", "garmentcodeArcLength"]) {
    $(id).disabled = has("garmentcodeAnnotations") || !$("garmentcodeArcs").checked;
  }
  $("vapor").disabled ||= !has("vaporRequest");
  $("thermalPoint").disabled ||= !$("selection").value.trim() || !$("thermalPointTemperature").value.trim() ||
    !Number.isFinite(Number($("thermalPointTemperature").value));
  $("thermalHistory").disabled ||= !has("thermalRequest");
  $("calibrate").disabled ||= !has("calibrationRequest");
  $("stageInterface").disabled ||= !source || !interfaceReady || !calibratedPair;
  $("replaceCalibratedInterface").disabled = !!pending || !calibratedPair;
  for (const op of ["stripAcquisition", "stripExperiment", "stripHistory", "stripFit"]) $(op).disabled ||= !has(`${op}Request`);
  $("study").disabled ||= !source || !has("studyRequest");
  if ($("studyOnBody").checked) $("study").disabled ||= !has("body") || !studyBodyReady();
  $("scenario").disabled ||= !source || !has("studyRequest") || !has("scenarioRequest") || !has("body") || !studyBodyReady();
  if ($("combinationMode").checked && !combinationGroups.size) {
    $("study").disabled = true; $("scenario").disabled = true;
  }
  $("exports").disabled ||= !has("measurements") || !has("fit");
  $("fold").disabled ||= !has("measurements") || !has("fit") || !has("body");
  $("fold").disabled ||= has("bodyFitFile") && !$("bodyFitConsent").checked;
  $("clearBodyFit").disabled = !has("bodyFitFile");
  $("bodyArmDeg").disabled = !has("bodyFitFile");
  $("selfContact").disabled = !!outfit;
  $("cut").disabled ||= !has("cutOptions");
  $("stock").disabled ||= !has("stockRequest");
  $("price").disabled ||= !has("cutOptions") || !has("quotes") || !has("market");
  $("sew").disabled ||= !has("sewOptions");
  $("heat").disabled ||= !drape || !has("heatConditions");
  $("vaporSurface").disabled ||= !drape || !has("vaporSurfaceConditions");
  $("coverage").disabled ||= !drape || !has("map") || !has("coverageConditions");
  $("clearCoverageBridges").disabled = !has("coverageBridges");
  $("cancel").disabled = !pending;
}
function clear(keys) {
  if (keys.includes("bind") && !keys.includes("treatment")) keys = [...keys, "treatment"];
  for (const key of keys) {
    for (const url of urls.get(key) || []) URL.revokeObjectURL(url);
    urls.delete(key);
    if (sections.has(key)) {
      const s = sections.get(key); s.section.hidden = true; s.links.replaceChildren();
      s.comparison.replaceChildren(); s.summary.textContent = ""; s.study = null; s.adoptions = [];
    }
    if (key === "fold") { drape = null; $("viewer").hidden = true; $("viewer").removeAttribute("src"); }
    if (key === "cut") { $("marker").hidden = true; $("marker").removeAttribute("src"); }
    if (key === "stock") { $("stockMarker").hidden = true; $("stockMarker").removeAttribute("src"); }
    if (key === "bind") { bound = null; $("boundSummary").textContent = "No current bound pattern."; $("bindFiles").replaceChildren(); }
    if (key === "treatment") preparedTreatment = null;
    if (key === "garmentcode") preparedImport = null;
    if (key === "calibrate") {
      calibratedPair = null;
      $("calibratedInterfaceSummary").textContent = "No current calibrated contact pair.";
      $("replaceCalibratedInterface").checked = false;
    }
  }
}
function error(message) { $("error").textContent = message; $("error").hidden = false; }
function invalidateThermalPoint() {
  if (pending?.op === "thermalPoint") start();
  clear(["thermalPoint"]); controls();
}
function start() {
  if (worker) worker.terminate();
  const generation = ++epoch;
  ready = false; pending = null;
  $("status").textContent = "Loading local material engine…"; controls();
  try { worker = new Worker(new URL("./materials-worker.js", import.meta.url), { type: "module" }); }
  catch (e) { error(String(e.message || e)); return; }
  worker.onmessage = ({ data: msg }) => {
    if (generation !== epoch) return;
    if (msg.type === "ready") { ready = true; $("status").textContent = "Ready. Inspect a material or load a pattern."; controls(); return; }
    if (msg.type === "loadError") { ready = false; error(`Cannot load web/pkg: ${msg.message}. Build the browser bundle first.`); controls(); return; }
    if (msg.request !== pending?.id) return;
    if (msg.type === "progress") { $("progress").textContent = msg.text; return; }
    const op = pending.op; pending = null;
    if (msg.type === "error") {
      error(msg.message); $("status").textContent = `${op} refused; no current result published.`;
      if (msg.fatal) { ready = false; error(`${msg.message}. Reload this page to restart the engine.`); }
    } else if (msg.type === "result") {
      show(op, msg); $("status").textContent = `${op} complete. Results describe only the retained inputs.`;
    }
    $("progress").textContent = ""; controls();
  };
  worker.onerror = e => {
    if (generation !== epoch) return;
    pending = null; ready = false; clear(["bind", ...operations]);
    error(e.message || "The worker failed; reload the page."); controls();
  };
}
function invalidate(keys, dependentOnly = false) {
  if (pending && (!dependentOnly || keys.includes(pending.op))) start(); // terminate affected Rust work; never wait for its stale success
  clear(keys); $("error").hidden = true; $("progress").textContent = ""; controls();
}
function link(key, parent, f) {
  const url = URL.createObjectURL(new Blob([f.bytes || f.text], { type: f.mime }));
  if (!urls.has(key)) urls.set(key, []); urls.get(key).push(url);
  const a = document.createElement("a"); a.href = url; a.download = f.name; a.textContent = f.name; parent.append(a);
  return url;
}
// These are displays of retained production outcomes, not a second scorer or physics engine.
const available = outcome => outcome?.status === "available" ? outcome.result : null;
const state = outcome => outcome?.status === "available" ? "Available" :
  outcome?.status === "not_requested" ? "Not requested" : outcome?.status === "refused" ?
  `Refused: ${outcome.refusal?.message || "See report"}` : "Unavailable";
const metric = value => typeof value === "number" && Number.isFinite(value) ? String(value) : "Unknown";
const vaporMetric = (result, name) => metric(result?.[name]) +
  (result?.reported_bounds?.[name] ? `; reported range [${result.reported_bounds[name].map(metric).join(", ")}]` : "");
function vaporBoundsNote(parent, result, label = "Vapor source bounds") {
  if (!result || !Object.hasOwn(result, "reported_bounds")) return;
  const note = document.createElement("p");
  note.textContent = `${label}: ` + (result.reported_bounds ?
    "complete reported resistance endpoints propagated through the fixed geometry, pressures, films, interfaces and latent-heat conversion. " +
    "These conditional ranges are not confidence intervals or uncertain-geometry, moisture or empirical garment predictions." :
    `unavailable${result.bounds_unavailable?.length ? ` — ${result.bounds_unavailable.join("; ")}` : ""}. ` +
    "Nominal results remain visible; missing bounds are not replaced with an exact range.");
  parent.append(note);
}
function table(parent, caption, headings) {
  const t = document.createElement("table"), c = document.createElement("caption"), head = document.createElement("tr");
  c.textContent = caption; t.append(c);
  for (const label of headings) { const th = document.createElement("th"); th.scope = "col"; th.textContent = label; head.append(th); }
  t.append(head); parent.append(t); return t;
}
function cells(t, values) {
  const row = document.createElement("tr");
  for (const value of values) { const td = document.createElement("td"); td.textContent = value; row.append(td); }
  t.append(row);
}
function detail(parent, title, value) {
  const d = document.createElement("details"), s = document.createElement("summary"), pre = document.createElement("pre");
  s.textContent = title; d.append(s, pre); parent.append(d);
  // Large material snapshots are formatted only when inspected. Downloaded reports stay raw.
  let shown = false;
  d.addEventListener("toggle", () => { if (d.open && !shown) { pre.textContent = text(value); shown = true; } });
}
function procurementResult(parent, r) {
  if (r.schema === "ftx-cut-procurement/3") return splitProcurementResult(parent, r);
  const grouped = Array.isArray(r.material_orders);
  for (const row of (grouped ? r.material_orders : r.roll_prices) || []) {
    const sample = row.sample, scope = sample.scope;
    const label = grouped ? `Material order ${row.material_order_index + 1}` : `Roll ${row.roll_index + 1}`;
    const note = document.createElement("p");
    note.textContent = `${label}: ${metric(row.required_stock_area_m2)} m² of stock. ` +
      (grouped ? `Markers ${row.roll_indices.map(i => i + 1).join(", ")} require ${row.marker_lengths_m.map(metric).join(", ")} m respectively; ` +
        `total ${metric(row.required_length_m)} m, longest continuous segment ${metric(row.required_continuous_length_m)} m. ` +
        "Minimum quantity, order increments and available stock apply once to the combined material order. " :
        (row.required_length_m == null ? "" : `The marker requires one continuous ${metric(row.required_length_m)} m cut. `)) +
      `Evaluated at ${scope.as_of_at || `${scope.as_of} (date only)`} UTC. ` +
      `Market: ${scope.currency}; ${scope.region}; ${scope.quality}; ${scope.terms}. ` +
      "The report retains the latest observed offer and its exclusion; an older price is not substituted. " +
      "A source continuous-cut limit is an upper limit, not a guarantee of continuous inventory.";
    parent.append(note);
    const entries = [...sample.offers.map(offer => ({ offer, quote: offer.quote })),
      ...(sample.excluded_offers || []).map(excluded => ({ excluded, quote: excluded.quote }))];
    const hasPriceBreaks = entries.some(({ quote, offer }) => quote.price_breaks?.length || offer?.applied_price_break);
    const t = table(parent, `${label} — latest supplier observations`,
      ["Offer", "Observed (UTC)", "Valid through (UTC)", "Admission", "Order quantity", `Merchandise cost (${scope.currency})`, "Source continuous-cut limit",
        ...(hasPriceBreaks ? ["Applied source price"] : [])]);
    for (const { quote: q, offer, excluded } of entries) {
      const expiry = [q.valid_through_at, q.valid_through ? `${q.valid_through} (date limit)` : null].filter(Boolean).join("; ");
      const quantity = offer?.order_quantity ?? excluded?.required_order_quantity;
      const continuity = q.maximum_continuous_quantity == null ? "Not reported" :
        `${metric(q.maximum_continuous_quantity)} ${q.unit}` +
        (offer?.within_observed_continuous_limit === true ? "; within observed limit" : "");
      const tier = offer?.applied_price_break;
      const appliedPrice = excluded ? "Not applied: excluded" : tier ?
        `From ${metric(tier.minimum_quantity)} ${q.unit}: ${metric(tier.price)} ${scope.currency} per ${metric(q.quantity)} ${q.unit}` +
          (tier.sku == null ? "" : `; SKU ${tier.sku}`) : "Base quote";
      cells(t, [q.offer_id, q.observed_at || `${q.observed_on} (date only)`, expiry || "Not reported",
        excluded ? `Excluded: ${excluded.reason.replaceAll("_", " ")}` : "Eligible",
        typeof quantity === "number" ? `${metric(quantity)} ${q.unit}` : "Unknown", metric(offer?.order_cost), continuity,
        ...(hasPriceBreaks ? [appliedPrice] : [])]);
    }
    if (!entries.length) cells(t, ["No observation in the requested market at this time", "Unknown", "Unknown", "No eligible offer", "Unknown", "Unknown", "Not reported"]);
    detail(parent, `${label}: selected order, source evidence and stock exclusions`, row);
  }
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage, currency: r.currency,
    complete_continuous_length_limit_coverage: r.complete_continuous_length_limit_coverage,
    least_observed_independent_order_total: r.least_observed_independent_order_total, limits: r.limits };
}
function splitProcurementResult(parent, r, label = "Whole-marker supplier allocation") {
  const summary = table(parent, label, ["Complete offer coverage", "Selected complete cost", "Proven least cost",
    "Search complete", "Budget exhausted", "Optimality proven", "Search nodes", "Quote evaluations"]);
  const cost = value => typeof value === "number" ? `${metric(value)} ${r.currency}` : "Unknown";
  cells(summary, [r.complete_offer_coverage ? "Yes" : "No complete allocation found",
    cost(r.selected_observed_order_total), cost(r.least_observed_independent_order_total),
    String(r.search_complete), String(r.budget_exhausted), String(r.order_allocation_optimality_proven),
    metric(r.search_nodes), metric(r.quote_evaluations)]);
  const note = document.createElement("p");
  note.textContent = "Each marker stays whole and is assigned once. Minimum quantities, increments and observed capacity apply once to each selected supplier order. " +
    "A complete feasible allocation remains usable when the work limit prevents proof of the least cost; no partial allocation is priced as a complete job. " +
    "Only source-locked offers for the exact material revision are considered. Requested cuts and dated stock observations do not reserve stock or guarantee fulfillment.";
  parent.append(note);
  for (const row of r.material_orders || []) {
    const allocation = row.allocation, selected = allocation?.selected_orders;
    const title = `${label}: material order ${row.material_order_index + 1}`;
    const t = table(parent, title, ["Supplier / offer", "Assigned markers", "Required stock (m²)", "Rounded order quantity",
      `Selected cost (${r.currency})`, "Observed available quantity", "Longest marker (m)", "Continuous-cut limit", "Order SKU / price tier"]);
    for (const order of selected || []) {
      const offer = order.offer, q = offer.quote;
      const tier = offer.applied_price_break;
      const orderSku = (tier?.sku ?? q.sku ?? "Not reported") +
        (tier ? `; tier from ${metric(tier.minimum_quantity)} ${q.unit}` : "");
      const continuity = q.maximum_continuous_quantity == null ? "Not reported" :
        `${metric(q.maximum_continuous_quantity)} ${q.unit}` +
        (offer.within_observed_continuous_limit === true ? "; within observed limit" : "");
      cells(t, [`${q.supplier || "Supplier not named"} / ${q.offer_id}`, order.roll_indices.map(i => i + 1).join(", "),
        metric(order.required_stock_area_m2), `${metric(offer.order_quantity)} ${q.unit}`, metric(offer.order_cost),
        q.available_quantity == null ? "Not reported" : `${metric(q.available_quantity)} ${q.unit}`,
        metric(order.required_continuous_length_m), continuity, orderSku]);
    }
    if (!selected?.length) cells(t, [allocation?.search_complete ? "No feasible allocation among admitted offers" :
      "No complete allocation found within the work limit", "Unassigned", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown", "Unknown"]);
    const status = document.createElement("p");
    status.textContent = `Markers ${(row.roll_indices || []).map(i => i + 1).join(", ")}; ` +
      `total required length ${metric(row.required_length_m)} m. ` +
      (allocation?.optimality_proven ? "Least cost proved for these whole markers and admitted offers. " :
        "Least cost has not been proved. ") +
      (allocation?.budget_exhausted ? "Search reached its declared work limit. " : "") +
      `Candidate offers: ${(allocation?.candidate_offer_ids || []).join(", ") || "None"}.`;
    parent.append(status);
    detail(parent, `${title}: selected orders, market scopes, source evidence and search result`, row);
  }
  detail(parent, `${label}: dated market and retained quote book`, { market: r.market, quotes: r.quotes });
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage,
    complete_continuous_length_limit_coverage: r.complete_continuous_length_limit_coverage, currency: r.currency,
    selected_observed_order_total: r.selected_observed_order_total,
    least_observed_independent_order_total: r.least_observed_independent_order_total,
    order_allocation_optimality_proven: r.order_allocation_optimality_proven,
    search_complete: r.search_complete, budget_exhausted: r.budget_exhausted,
    search_nodes: r.search_nodes, quote_evaluations: r.quote_evaluations, limits: r.limits };
}
function screening(parent, r) {
  const properties = [...new Set(r.alternatives.flatMap(row => Object.keys(row.readings)))].sort();
  const t = table(parent, "Scalar-property screening only — not garment-level ranking", ["Alternative", "Admission", "Dominated by", ...properties]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(x => x.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported", ...properties.map(key => {
        const q = row.readings[key];
        return q ? `${metric(q.value)} ${q.unit}; ${q.bounds ? `[${q.bounds.map(metric).join(", ")}] reported bounds` : "bounds not reported"}` : "Unknown";
      })]);
    detail(parent, `${row.key}: observations, protocols and frozen source (display only)`, {
      material_id: row.material_id, readings: row.readings, exclusions: row.exclusions, selection: row.selection });
  }
}
function metricUsesBounds(policy, name) {
  return (policy?.basis === "reported_bounds" && policy.objectives.some(o => o.metric === name)) ||
    (policy?.constraints || []).some(c => c.metric === name && (c.basis ?? policy.basis) === "reported_bounds");
}
function objectiveComparison(parent, r) {
  const motionQualified = r.motion_required === true;
  const reportedBounds = r.policy.basis === "reported_bounds";
  const anyBounds = reportedBounds || r.policy.constraints.some(c => c.basis === "reported_bounds");
  const note = document.createElement("p");
  note.textContent = `Computed ${r.policy.basis} frontier: ${r.frontier.length ? r.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
    (r.policy.allow_mixed_protocols ? "Mixed source protocols explicitly admitted. " : "Reported input protocols must match. ") +
    "Each constraint uses its explicit basis or inherits the objective basis. Nominal constraints retain nominal admission. " +
    (anyBounds ? "Requested reported-range decisions use complete source-reported ranges: mass at fixed authored geometry and inventory, " +
      "constant steady dry heat at fixed geometry, reservoirs, films and interfaces, " +
      "and vapor potential at fixed geometry, boundaries and retained sampling. " +
      "Final cumulative finite-water ranges, when requested, additionally hold the declared initial liquid and supply fixed. " +
      "Conditional on these inputs and evaluated alternatives; no general scenario uncertainty, confidence level, empirical fit or global optimum claim." :
      "Conditional on these inputs and evaluated alternatives; no propagated uncertainty, empirical fit or global optimum claim.");
  parent.append(note);
  const scope = document.createElement("p");
  scope.textContent = motionQualified ?
    "Motion-qualified comparison: a complete admitted trajectory is required. Each declared motion heat objective also requires its complete thermal history. " +
      "Initial static objectives retain their initial-frame meaning; signed energy uses the requested sheet or body-coverage domain." :
    "This comparison does not require motion completion. Static drape or heat objectives, when requested, describe the initial state. " +
      "Any separately requested motion or transient heat must be inspected in its own result.";
  parent.append(scope);
  const objectives = r.policy.objectives;
  const constraints = r.policy.constraints.filter(c => !objectives.some(o => o.metric === c.metric));
  const metrics = [...objectives, ...constraints];
  const wetFilmLabels = {
    motion_wet_film_evaporated_mass_kg: "Modeled covered evaporation",
    motion_wet_film_remaining_liquid_mass_kg: "Remaining body liquid",
    motion_wet_film_unmet_demand_mass_kg: "Unmet covered vapor demand",
    motion_wet_film_supplied_mass_kg: "Declared supplied body water",
    motion_wet_film_body_reservoir_latent_energy_j: "Prescribed-body latent energy"
  };
  const threadLabels = {
    thread_required_length_m: "Required thread length",
    thread_required_mass_kg: "Consumed thread mass",
    thread_observed_order_cost: "Complete thread order cost",
    cut_and_sew_selected_order_cost: "Complete fabric + thread order cost"
  };
  const cuttingLabels = {};
  for (const [prefix, scope] of [["cutting", "Roll"], ["finite_stock", "Finite stock"], ["cutting_job", "Complete cutting job"]]) {
    cuttingLabels[`${prefix}_modeled_path_seconds`] = `${scope} modeled knife time`;
    cuttingLabels[`${prefix}_cut_length_m`] = `${scope} cut length`;
    cuttingLabels[`${prefix}_raised_travel_length_m`] = `${scope} raised travel`;
  }
  const contactLabels = {
    body_contact_resultant_n: "Initial body-contact resultant",
    max_body_contact_vertex_force_n: "Initial maximum nodal body-contact force",
    motion_peak_body_contact_resultant_n: "Motion peak body-contact resultant",
    motion_max_body_contact_vertex_force_n: "Motion maximum nodal body-contact force"
  };
  const textileWaterLabels = {
    textile_water_evaporated_mass_kg: "Evaporated textile water",
    textile_water_remaining_mass_kg: "Remaining textile liquid",
    textile_water_supplied_mass_kg: "Supplied textile liquid",
    textile_water_condensed_mass_kg: "Condensed textile water",
    textile_water_latent_energy_j: "Textile evaporation latent energy",
    textile_water_condensation_latent_energy_j: "Textile condensation latent release"
  };
  const strainLabels = {};
  for (const [prefix, scope] of [["", "Initial"], ["motion_", "Motion"]]) {
    for (const [axis, label] of [["warp", "warp E11"], ["weft", "weft E22"], ["engineering_shear", "shear 2E12"]]) {
      strainLabels[`${prefix}min_${axis}_green_strain`] = `${scope} minimum ${label}`;
      strainLabels[`${prefix}max_${axis}_green_strain`] = `${scope} maximum ${label}`;
    }
  }
  if (metrics.some(o => Object.hasOwn(strainLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["material_strain", "motion_material_strain"].includes(name))) {
    const strain = document.createElement("p");
    strain.textContent = "Material-direction objectives use signed, dimensionless Green strains in the retained warp and weft frame. " +
      "The requested instances define the spatial scope; motion extrema include the initial state and every accepted sample. " +
      "Inclusive constraints are declared design requirements. They do not establish fabric rupture limits, continuous-time bounds, wearer comfort or measured fit.";
    parent.append(strain);
  }
  if (metrics.some(o => Object.hasOwn(contactLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["contact_loads", "motion_contact_loads"].includes(name))) {
    const contact = document.createElement("p");
    contact.textContent = "Body-contact objectives use normal barrier forces in newtons over the selected garment vertices. " +
      "Shared seam vertices count once, and opposite forces can cancel in a resultant. Nodal maxima depend on mesh resolution. " +
      "Motion extrema cover the initial state and every accepted sample; these declared limits do not establish contact pressure, friction loads, comfort or measured fit.";
    parent.append(contact);
  }
  if (metrics.some(o => Object.hasOwn(textileWaterLabels, o.metric))) {
    const textileWater = document.createElement("p");
    textileWater.textContent = "Textile water objectives use the complete admitted coupled temperature history. " +
      "The Rust comparison checks exact material and geometry ownership, canonical vapor paths, initial and supplied water inventory, incoming sensible heat and the combined sensible/latent energy balance. " +
      "These nominal quantities account for textile-owned liquid; body-reservoir water has separate objectives. " +
      "Source uncertainty, physical garment validation and a globally optimal design are not established.";
    parent.append(textileWater);
  }
  if (metrics.some(o => Object.hasOwn(threadLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["sewing_consumables", "sewing_purchasing", "cut_and_sew_purchasing"].includes(name))) {
    const thread = document.createElement("p");
    thread.textContent = "Thread comparison uses the selected sewing plan's complete, source-qualified channels and exact yarn orders. " +
      "Thread purchasing retains its own currency and dated market. A requested complete fabric + thread cost additionally requires one coherent market and quote history with distinct physical stock. " +
      "Missing channels or offers leave the corresponding complete metric unknown. Recipe selection by time does not prove a thread-cost optimum.";
    parent.append(thread);
  }
  if (metrics.some(o => Object.hasOwn(cuttingLabels, o.metric)) ||
      r.policy.required_consumers?.some(name => ["cutting_program", "finite_stock_program", "cutting_job_program"].includes(name))) {
    const cutting = document.createElement("p");
    cutting.textContent = "Cutting objectives use complete selected knife traversals qualified for the exact textile and machine. " +
      "Roll and finite-stock objectives retain their separate physical scope; only a complete cutting job combines its disjoint routes. " +
      "Modeled time covers declared constant-rate cutting, raised travel and lift/lower phases. It excludes loading, acceleration and inspection. " +
      "An incomplete or refused process leaves these nominal metrics unknown while independent geometry and purchasing remain available. " +
      "A complete selected traversal need not be shortest; these objectives do not establish production cycle time, physical cut quality or machine authorization.";
    parent.append(cutting);
  }
  const waterMetrics = metrics.filter(o => Object.hasOwn(wetFilmLabels, o.metric));
  if (waterMetrics.length || r.policy.required_consumers?.includes("motion_wet_film")) {
    const water = document.createElement("p");
    water.textContent = "Finite body water comparison requires complete covered motion and the exact declared film policy. " +
      "Modeled evaporation is limited by each triangle's initial liquid and prescribed supply; water is not pooled between triangles. " +
      (waterMetrics.some(o => metricUsesBounds(r.policy, o.metric)) ?
        (waterMetrics.some(o => o.metric !== "motion_wet_film_supplied_mass_kg") ?
          "Final cumulative source ranges replay low- and high-demand Ret endpoints with geometry, boundaries, initial liquid, supply and integration fixed. " +
          "Remaining liquid reverses the demand endpoint order; evaporation, unmet demand and latent energy follow low to high demand. " +
          "Missing Ret bounds exclude these bounded water metrics while retaining nominal readings. " : "") +
        "Declared supplied water, when requested, has a fixed singleton range and needs no Ret bounds. " +
        "Accepted-sample water ledgers and replay remain nominal, without per-interval water bounds. " +
        "These conditional ranges do not establish general water-state uncertainty or confidence intervals. " :
        "These nominal results do not propagate water-state uncertainty. ") +
      "Prescribed-body latent energy does not include textile-temperature feedback or establish wearer cooling. " +
      "Neither basis describes bare-body transfer or physiological sweating.";
    parent.append(water);
  }
  const t = table(parent, "Declared computed objectives — authoritative Rust comparison", ["Alternative", "Admission", "Dominated by",
    ...metrics.map(o => `${wetFilmLabels[o.metric] || textileWaterLabels[o.metric] || strainLabels[o.metric] || contactLabels[o.metric] || threadLabels[o.metric] || cuttingLabels[o.metric] || o.metric} (${o.direction || "constraint"})`)]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(e => e.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
      ...metrics.map(o => {
        const q = row.readings[o.metric];
        if (!q) return "Unknown";
        const range = q.reported_bounds ? `; reported range [${q.reported_bounds.map(metric).join(", ")}] ${q.unit}` :
          metricUsesBounds(r.policy, o.metric) ? "; reported range unknown" : "";
        return `${metric(q.value)} ${q.unit}${range}`;
      })]);
  }
  for (const pair of r.incomparable) {
    const p = document.createElement("p"); p.textContent = `${pair.alternatives.join(" / ")}: ${pair.reason}`; parent.append(p);
  }
  if (r.policy.constraints.length) {
    const limits = table(parent, "Declared constraints — basis and inclusive limits",
      ["Metric", "Basis", "Minimum", "Maximum"]);
    for (const c of r.policy.constraints) cells(limits, [c.metric,
      `${c.basis ?? r.policy.basis}${c.basis ? " (explicit)" : " (inherited)"}`,
      c.minimum == null ? "None" : metric(c.minimum), c.maximum == null ? "None" : metric(c.maximum)]);
  }
  detail(parent, "Computed comparison policy, context and exclusions (display only)", r);
}
function assemblyMassComparison(parent, rows) {
  if (!rows.some(row => row.assembly_mass)) return;
  const note = document.createElement("p");
  note.textContent = "One declared pre-finishing assembly, not purchased stock or simulated cloth. All physical cut pieces and declared extras are counted. " +
    "Reported mass bounds use fixed authored geometry and inventory; they are not confidence intervals or an allowance for missing components.";
  parent.append(note);
  const t = table(parent, "Declared assembly mass — complete or unknown", ["Alternative", "Mass consumer", "All cut sheets (kg)",
    "Declared assembly (kg)", "Reported assembly bounds (kg)", "Extra inventory"]);
  for (const row of rows) {
    const report = available(row.assembly_mass), mass = report?.assembly_mass;
    cells(t, [row.key, report ? (mass?.kg == null ? "Report available; total unknown" : "Complete declared inventory") :
      row.assembly_mass ? state(row.assembly_mass) : "Not requested",
      metric(report?.sheet_mass?.kg), metric(mass?.kg),
      mass?.reported_bounds_kg ? `[${mass.reported_bounds_kg.map(metric).join(", ")}]` : "Not reported",
      report?.request?.inventory_complete === true ? "Declared complete" :
        report?.request?.inventory_complete === false ? "Incomplete" : "Unknown"]);
    detail(parent, `${row.key}: cut-piece mass, retained extras and source inventory (display only)`,
      report || { refusal: row.assembly_mass?.refusal });
  }
}
function attachedMassComparison(parent, rows, simulations) {
  if (!rows.some(row => available(simulations.get(row.key))?.drape?.attached_mass)) return;
  const note = document.createElement("p");
  note.textContent = "Source-qualified accessory groups contribute to gravity and inertia at their declared physical attachment points. " +
    "Cloth thermal storage continues to use the characterized sheet materials and reference areas.";
  parent.append(note);
  const t = table(parent, "Attached accessory mass — gravity and inertia", ["Alternative", "Drape", "Simulated cloth (kg)",
    "Attached groups (kg)", "Total inertial mass (kg)", "Mass balance residual (kg)"]);
  for (const row of rows) {
    const outcome = simulations.get(row.key), report = available(outcome)?.drape?.attached_mass;
    cells(t, [row.key, state(outcome), metric(report?.cloth_mass_kg), metric(report?.attached_mass_kg),
      metric(report?.total_inertial_mass_kg), metric(report?.mass_balance_residual_kg)]);
    detail(parent, `${row.key}: attached mass sources and welded physical targets (display only)`,
      report || { refusal: outcome?.refusal });
  }
}
function materialStrainComparison(parent, key, output, dependency, requested, moving = false) {
  if (!requested && (!output || output.status === "not_requested")) return;
  const result = available(output), label = moving ? "accepted-motion" : "initial";
  const status = document.createElement("p");
  status.textContent = `${key}: ${label} material-direction strain — ${output ? state(output) : `Unavailable: ${state(dependency)}`}. ` +
    (result ? `${metric(result.triangle_samples)} triangle evaluations; sampled duration ${metric(result.duration_s)} s.` : "");
  parent.append(status);
  if (result) {
    const t = table(parent, `${key}: ${label} material-direction strain`,
      ["Scope", "Direction", "Minimum (1)", "Minimum location", "Maximum (1)", "Maximum location"]);
    const witness = w => w ? `Triangle ${metric(w.source_triangle)}, sample ${metric(w.sample_index)} at ${metric(w.time_s)} s` : "Unknown";
    for (const [scope, values] of [["Selected scope", result], ...Object.entries(result.instances || {})]) {
      ["Warp E11", "Weft E22", "Engineering shear 2E12"].forEach((axis, i) => {
        cells(t, [scope, axis, metric(values.green_strain_range?.[i]?.[0]), witness(values.minimum_witness?.[i]),
          metric(values.green_strain_range?.[i]?.[1]), witness(values.maximum_witness?.[i])]);
      });
    }
    const note = document.createElement("p");
    note.textContent = "Raw material Green strain [E11, E22, 2E12] is dimensionless and retains its sign; it is not engineering extension percent. " +
      "Values use the production material frame before numerical compression or gather softening. Witnesses retain the earliest sampled extreme and its source triangle. " +
      "Characterization domains describe supported material observations; declared comparison limits do not establish rupture, safe strain, continuous-time bounds or physical garment fit.";
    parent.append(note);
  }
  detail(parent, `${key}: ${label} strain scope, sources and sampled witnesses (display only)`, result || {refusal: output?.refusal || dependency?.refusal});
}
function contactLoadsComparison(parent, key, output, dependency, requested, moving = false) {
  if (!requested && (!output || output.status === "not_requested")) return;
  const result = available(output), label = moving ? "accepted-motion" : "initial";
  const status = document.createElement("p");
  status.textContent = `${key}: ${label} body-contact loads — ${output ? state(output) : `Unavailable: ${state(dependency)}`}. ` +
    (result ? `${metric(result.vertex_samples)} vertex evaluations; ${metric(result.pair_evaluations)} contact-pair evaluations.` : "");
  parent.append(status);
  if (result) {
    const frames = result.frames || [];
    const witness = index => {
      const frame = frames.find(f => f.sample_index === index);
      return frame ? `Sample ${metric(frame.sample_index)} at ${metric(frame.time_s)} s` : "Unknown";
    };
    const summary = table(parent, `${key}: ${label} body-contact loads`,
      ["Peak resultant (N)", "Resultant witness", "Maximum nodal force (N)", "Nodal-force witness"]);
    cells(summary, [metric(result.peak_body_resultant_n), witness(result.peak_body_resultant_sample),
      metric(result.max_body_vertex_force_n), witness(result.max_body_vertex_force_sample)]);
    const samples = table(parent, `${key}: ${label} sampled body-contact forces`,
      ["Sample", "Time (s)", "Resultant x (N)", "Resultant y (N)", "Resultant z (N)", "Resultant magnitude (N)", "Maximum nodal force (N)", "Source vertex"]);
    for (const frame of frames) {
      const body = frame.body;
      cells(samples, [metric(frame.sample_index), metric(frame.time_s),
        ...[0, 1, 2].map(i => metric(body?.resultant_n?.[i])), metric(body?.resultant_magnitude_n),
        metric(body?.max_vertex_force_n), metric(body?.max_force_vertex)]);
    }
    const scope = document.createElement("p");
    scope.textContent = `Selected garment instances: ${Object.keys(result.source?.instances || {}).join(", ") || "Unknown"}. ` +
      "The shared vertices of sewn pieces are counted once. Resultants can hide opposing local loads; compare nodal maxima at the same mesh resolution. " +
      "These normal barrier forces exclude tangential friction and do not establish pressure, safe wear or continuous-time load bounds.";
    parent.append(scope);
  }
  detail(parent, `${key}: ${label} contact scope, materials and sampled force witnesses (display only)`,
    result || { refusal: output?.refusal || dependency?.refusal });
}
function transientHeatComparison(parent, key, result, dependency, policy) {
  const consumers = [["transient_surface", "Independent sheets"], ["transient_coverage", "Body coverage + exposed cloth"]]
    .filter(([name]) => policy?.[name] || result?.[name]);
  if (!consumers.length) return;
  const t = table(parent, `${key}: transient heat — complete time history or refusal`, ["Thermal domain", "Status", "Duration (s)",
    "Thermal substeps", "Final temperature range (°C)", "Net boundary energy (J)", "Textile sensible change (J)",
    "Energy residual (J)", "Signed remap residual (J)"]);
  for (const [name, label] of consumers) {
    const output = result?.[name], h = available(output), range = h?.trace?.at(-1)?.temperature_range_c;
    cells(t, [label, output ? state(output) : `Unavailable: ${state(dependency)}`, metric(h?.duration_s), metric(h?.thermal_steps),
      range ? `[${range.map(metric).join(", ")}]` : "Unknown", metric(h?.net_boundary_energy_j),
      metric(h?.physical_stored_change_j ?? h?.stored_change_j), metric(h?.balance_error_j),
      name === "transient_surface" ? "Not applicable" : metric(h?.signed_remap_residual_j)]);
    detail(parent, `${key}: ${label} temperature history, source and energy accounting (display only)`, h || { refusal: output?.refusal || dependency?.refusal });
    if (h?.thermal_bridges) {
      const bridges = h.thermal_bridges;
      const energy = table(parent, `${key}: seam and closure heat during motion`,
        ["Baseline inside energy (J)", "Signed bridge excess (J)", "Total inside energy (J)",
          "Baseline outside energy (J)", "Total outside energy (J)"]);
      cells(energy, [metric(bridges.baseline_inside_energy_j), metric(bridges.bridge_energy_j),
        metric(bridges.total_inside_energy_j), metric(bridges.baseline_outside_energy_j), metric(bridges.total_outside_energy_j)]);
      const scope = document.createElement("p");
      scope.textContent = "These seam and closure paths use an independently qualified zero-storage exchange law. " +
        "Their actual length and body ownership are checked at each accepted motion endpoint, with live textile temperatures and the full interval inside the source domain. " +
        "The same signed excess energy is added to the inside and outside boundary ledgers; baseline textile temperature, sensible storage and net boundary energy remain unchanged. " +
        "The ordinary inside-energy comparison objective uses the total. This model does not infer seam heat capacity or a time-dependent law from a steady measurement. " +
        "A later source or geometry failure refuses the complete affected heat history.";
      parent.append(scope);
      detail(parent, `${key}: moving bridge qualifications, geometry and signed energy history (display only)`, bridges);
    }
    if (h?.surface_water) {
      const water = h.surface_water;
      const condensing = water.schema === "ftx-textile-surface-water-result/3";
      const supplied = condensing || water.schema === "ftx-textile-surface-water-result/2";
      const mass = table(parent, `${key}: textile surface liquid — conserved ${condensing ? "initial, supplied and condensed" : supplied ? "initial and supplied" : "initial"} inventory`,
        ["Initial liquid (kg)", ...(supplied ? ["Supplied liquid (kg)"] : []),
          ...(condensing ? ["Condensed liquid (kg)"] : []), "Evaporated liquid (kg)", "Remaining liquid (kg)", "Mass residual (kg)", "Dryout events"]);
      cells(mass, [metric(water.initial_liquid_kg), ...(supplied ? [metric(water.supplied_liquid_kg)] : []),
        ...(condensing ? [metric(water.condensed_liquid_kg)] : []), metric(water.evaporated_liquid_kg), metric(water.remaining_liquid_kg),
        metric(water.mass_balance_error_kg), metric(water.dryout_events)]);
      const energy = table(parent, `${key}: textile evaporation and sensible energy`,
        ["Latent energy (J)", "Exported vapor sensible energy (J)", "Liquid sensible change (J)",
          ...(supplied ? ["Incoming liquid sensible energy (J)"] : []),
          ...(condensing ? ["Condensation latent release (J)", "Incoming vapor sensible energy (J)"] : []), "Combined energy residual (J)"]);
      cells(energy, [metric(water.latent_energy_j), metric(water.exported_vapor_sensible_energy_j),
        metric(water.liquid_stored_change_j), ...(supplied ? [metric(water.supplied_liquid_sensible_energy_j)] : []),
        ...(condensing ? [metric(water.condensation_latent_energy_j), metric(water.incoming_vapor_sensible_energy_j)] : []), metric(water.balance_error_j)]);
      const scope = document.createElement("p");
      scope.textContent = "This explicitly sourced free liquid shares the inner textile cell temperature. Evaporation cools that cell, " +
        "and the temperature feeds the declared vapor-pressure table. " +
        (supplied ? "Each mechanical interval has independently sourced delivery rates and incoming temperatures per instance. Supply uses reference material area, can support evaporation on a dry patch and can rewet it. " :
          "Dryout ends evaporation; the remaining interval uses the dry heat model. ") +
        "The combined balance includes dry-textile and liquid sensible storage, exported vapor sensible energy and latent energy, " +
        "against boundary energy plus qualified material heat" + (supplied ? " and incoming liquid sensible energy. Loading beyond the qualified cap refuses the interval. " : ". The initial-only model has zero replenishment. ") +
        (condensing ? "Independently qualified reverse vapor transfer can condense on dry or wet cloth and later re-evaporate. Condensation latent release and incoming vapor sensible enthalpy are separate energy inputs; the effective enthalpy convention remains explicit in the source policy. " : "") +
        (h.mechanical_coupling?.exchange ? "Qualified supplied liquid, condensed vapor and evaporation update cloth inertia and gravity after every accepted interval. " :
          h.mechanical_coupling ? "Qualified evaporation updates cloth inertia and gravity after every accepted interval; co-moving vapor removes its own momentum without accelerating the remaining cloth. " :
          "The liquid adds no mechanical water mass. ") +
        (["ftx-coupled-liquid-mechanics-result/2", "ftx-coupled-liquid-mechanics-result/3"].includes(h.mechanical_coupling?.schema) ?
          "Incoming water also transfers its explicitly prescribed world-frame momentum. Inelastic mixing loss is reported separately from sensible and latent heat. " : "") +
        "It does not infer absorbed moisture, wicking, body sweat or wet material properties. Comparison admission checks the complete liquid and vapor ledger, " +
        "including exact material bindings and canonical vapor paths, before temperature, energy or textile-water objectives can qualify.";
      parent.append(scope);
      detail(parent, `${key}: textile liquid source, exact patches and canonical vapor paths (display only)`, water);
      if (h.mechanical_coupling) {
        const m = h.mechanical_coupling, exchange = m.exchange;
        const prescribed = ["ftx-coupled-liquid-mechanics-result/2", "ftx-coupled-liquid-mechanics-result/3"].includes(m.schema);
        const inertia = table(parent, `${key}: ${exchange ? "liquid exchange" : "evaporation"} changes moving cloth mass`,
          ["Dry cloth + accessories (kg)", "Initial water (kg)", "Remaining water (kg)", "Current inertial mass (kg)", "Mass residual (kg)"]);
        cells(inertia, [metric(m.substrate_and_attached_mass_kg), metric(m.initial_liquid_mass_kg),
          metric(m.remaining_liquid_mass_kg), metric(m.total_inertial_mass_kg), metric(m.mass_balance_residual_kg)]);
        const vector = value => Array.isArray(value) ? value.map(metric).join(", ") : "Unknown";
        if (exchange) {
          const transfer = table(parent, `${key}: ${prescribed ? "prescribed incoming" : "co-moving"} liquid transfer — separate from thermal energy`,
            ["Transfer", "Mass (kg)", "Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)"]);
          cells(transfer, ["Initial admission", metric(m.initial_liquid_mass_kg), vector(exchange.initial_momentum_kg_m_s),
            metric(exchange.initial_kinetic_energy_j), metric(exchange.initial_gravitational_energy_j)]);
          cells(transfer, ["Incoming liquid and condensed vapor", metric(exchange.incoming_mass_kg), vector(exchange.incoming_momentum_kg_m_s),
            metric(exchange.incoming_kinetic_energy_j), metric(exchange.incoming_gravitational_energy_j)]);
          cells(transfer, ["Outgoing vapor", metric(exchange.outgoing_mass_kg), vector(m.exported_momentum_kg_m_s),
            metric(m.exported_kinetic_energy_j), metric(m.exported_gravitational_energy_j)]);
          const residual = table(parent, `${key}: liquid exchange momentum and energy residuals`,
            ["Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)",
              ...(prescribed ? ["Inelastic mixing loss (J)"] : [])]);
          cells(residual, [vector(m.transfer_momentum_residual_kg_m_s), metric(m.transfer_kinetic_residual_j),
            metric(m.transfer_gravitational_residual_j), ...(prescribed ? [metric(exchange.mixing_dissipation_j)] : [])]);
        } else {
          const transfer = table(parent, `${key}: mechanical energy carried away by vapor`,
            ["Momentum x, y, z (kg m/s)", "Kinetic energy (J)", "Gravitational energy (J)", "Kinetic transfer residual (J)"]);
          cells(transfer, [vector(m.exported_momentum_kg_m_s), metric(m.exported_kinetic_energy_j),
            metric(m.exported_gravitational_energy_j), metric(m.transfer_kinetic_residual_j)]);
        }
        const couplingScope = document.createElement("p");
        couplingScope.textContent = "Initial liquid is added co-moving after the dry drape; it is not a wet equilibrium. Each mechanical step uses its starting water mass, then accepted heat and liquid exchange update the next step. " +
          "Refine the mechanical time step to assess this first-order coupling. The thermal latent/sensible ledger remains separate from transferred bulk mechanical energy. " +
          (exchange ? (prescribed ?
            "Incoming liquid and condensed vapor use the prescribed world-frame velocities; outgoing vapor leaves at the mixed cloth velocity. Inelastic mixing loss is separate from thermal heat. " :
            "Incoming liquid, condensed vapor and outgoing vapor share the local endpoint cloth velocity under the independent source qualification. ") +
            "Equal incoming and outgoing mass retain both transfer ledgers. Signed gravitational energies keep the declared world frame. These residuals check mass transfer, not the complete motion energy balance. " :
            "Supply and condensation are unsupported in this evaporation-only mode. ") +
          "A coupled heat or source-domain failure refuses the motion. Drainage, resolved fluid impact and inferred wet material properties remain unsupported.";
        parent.append(couplingScope);
        detail(parent, `${key}: ${exchange ? "liquid exchange" : "evaporation"} mechanics qualification and complete transfer ledger (display only)`, m);
      }
    }
    const channels = [
      ["plastic_dissipation_heat", "Plastic yield", "plastic_dissipation_j", "initial_plastic_dissipation_j"],
      ["relaxation_dissipation_heat", "Maxwell dashpots", "viscous_dissipation_j", "initial_viscous_dissipation_j"],
      ["bending_plastic_dissipation_heat", "Bending yield", "plastic_dissipation_j", "initial_plastic_dissipation_j"]
    ].filter(([field]) => h?.[field]);
    if (channels.length) {
      const domain = name === "transient_surface" ? "independent-sheet heat" : "coverage and exposed-cloth heat";
      const work = table(parent, `${key}: material work supplied to ${domain}`, ["Physical source", "Work since motion start (J)",
        "Supplied material heat (J)", "Unconverted work (J)", "Initial work excluded (J)"]);
      for (const [field, label, total, initial] of channels) {
        const source = h[field];
        cells(work, [label, metric(source[total]), metric(source.supplied_heat_j),
          metric(source.unconverted_dissipation_j), metric(source[initial])]);
        detail(parent, `${key}: ${label} conversion observations for ${domain} (display only)`, {
          distribution: source.distribution, temporal_distribution: source.temporal_distribution,
          material_fractions: source.material_fractions ?? null
        });
      }
      const scope = document.createElement("p");
      scope.textContent = (h.surface_water ?
        "The energy residual includes dry-textile and liquid sensible storage, exported vapor sensible energy and evaporative latent energy, minus net boundary energy, qualified source heat and declared incoming liquid/vapor sensible energy and condensation latent release. " :
        "The energy residual is physical stored change minus net boundary energy minus the sum of qualified source heat. ") +
        "Each source needs its own material conversion evidence. Work already accumulated in the initial drape, reversible storage and numerical loss are excluded. " +
        (channels.every(([field]) => h[field].distribution === "uniform_reference_thickness" && h[field].temporal_distribution === "uniform_over_accepted_interval") ?
          "Converted heat is distributed uniformly through each sheet's reference thickness and over its accepted motion interval. " :
          "Inspect the retained report for the declared heat distribution. ") +
        (name === "transient_coverage" ? "Source heat follows covered and exposed reference material; bare body receives none. " : "") +
        (channels.some(([field]) => field === "bending_plastic_dissipation_heat") ?
          "Bending work uses each source triangle's reference area; overlapping reconstruction stencils do not multiply deposited heat. " : "") +
        (h.mechanical_coupling ? "Temperature controls qualified vapor exchange, which updates mechanical mass; the material laws remain frozen within their qualified domains. " : "Temperatures do not feed back into mechanics. ") +
        "The temperature-history report retains the material fractions and their source evidence.";
      parent.append(scope);
    }
  }
  const note = document.createElement("p");
  note.textContent = (consumers.some(([name]) => available(result?.[name])?.mechanical_coupling) ?
    "Only the explicitly qualified liquid domain feeds mechanical mass; the thermal domains retain separate initial temperatures, interval films and energy totals. Do not add their energy totals. " :
    "These are separate one-way thermal models with declared initial temperatures and interval films. Do not add their energy totals. ") +
    "Remapping transfers stored heat without elapsed time; its numerical residual is not environmental heat. Bare body paths store no cloth energy. " +
    "Geometry uses accepted motion samples; smaller thermal substeps do not refine the body motion. Small energy residuals are not empirical accuracy.";
  parent.append(note);
}
function motionSamples(parent, key, motion) {
  const d = document.createElement("details"), title = document.createElement("summary");
  title.textContent = `${key}: inspect retained motion samples`; d.append(title); parent.append(d);
  let initialized = false;
  d.addEventListener("toggle", () => {
    if (!d.open || initialized) return;
    initialized = true;
    try {
      const accepted = motion.samples, hasInitial = motion.initial_sample != null;
      const samples = Array.isArray(accepted) && accepted.length ? (hasInitial ? [motion.initial_sample, ...accepted] : accepted) : null;
      const counts = [samples?.[0]?.body_positions_m?.length, samples?.[0]?.positions_m?.length];
      const tracks = ["body_positions_m", "positions_m"], topology = [motion.body_triangles, motion.triangles];
      if (!samples || counts.some(n => !n) || topology.some((triangles, i) =>
        !Array.isArray(triangles) || !triangles.length || triangles.some(t => !Array.isArray(t) || t.length !== 3 ||
          t.some(v => !Number.isInteger(v) || v < 0 || v >= counts[i])))) {
        throw Error("Retained body/cloth geometry is incomplete; inspect the downloaded report.");
      }
      const low = [Infinity, Infinity, Infinity], high = [-Infinity, -Infinity, -Infinity];
      let previous = hasInitial ? -1 : 0;
      for (const sample of samples) {
        if (!Number.isFinite(sample.time_s) || sample.time_s <= previous || (hasInitial && previous === -1 && sample.time_s !== 0)) {
          throw Error("Retained physical sample times are invalid; no intermediate motion is inferred.");
        }
        previous = sample.time_s;
        for (const [i, track] of tracks.entries()) {
          if (!Array.isArray(sample[track]) || sample[track].length !== counts[i]) {
            throw Error("Retained samples changed topology; inspect the downloaded report.");
          }
          for (const p of sample[track]) {
            if (!Array.isArray(p) || p.length !== 3 || p.some(v => !Number.isFinite(v))) {
              throw Error("Retained sample coordinates are invalid; inspect the downloaded report.");
            }
            for (let axis = 0; axis < 3; ++axis) { low[axis] = Math.min(low[axis], p[axis]); high[axis] = Math.max(high[axis], p[axis]); }
          }
        }
      }
      const span = Math.max(...high.map((v, i) => v - low[i]), 1e-6);
      if (!Number.isFinite(span)) throw Error("Retained sample coordinates exceed the display range.");
      const center = high.map((v, i) => low[i] + (v - low[i]) / 2), scale = 372 / span;
      const note = document.createElement("p"); note.className = "note";
      note.textContent = "Select a retained state at its exact physical time. " + (hasInitial ?
        "The first state precedes motion; later states are accepted solver steps. " :
        "This report retains accepted solver steps; its initial geometry was not retained. ") +
        "Grey body and teal cloth use one fixed metre scale across both views and every sample. Wireframe projections do not resolve depth.";
      const label = document.createElement("label"), slider = document.createElement("input"), stamp = document.createElement("output");
      label.textContent = "Retained sample"; slider.type = "range"; slider.min = "0"; slider.max = String(samples.length - 1);
      slider.step = "1"; slider.value = "0"; slider.ariaLabel = `${key}: retained motion sample`; label.append(slider);
      const views = document.createElement("div"); views.className = "motion-views";
      const contexts = ["Front · X / Z", "Side · Y / Z"].map(name => {
        const figure = document.createElement("figure"), caption = document.createElement("figcaption"), canvas = document.createElement("canvas");
        caption.textContent = name; canvas.width = 420; canvas.height = 420;
        canvas.ariaLabel = `${key}: ${name} at the selected physical sample`; canvas.textContent = canvas.ariaLabel;
        const context = canvas.getContext("2d");
        if (!context) throw Error("Canvas preview unavailable; retained sample geometry remains in the downloaded report.");
        figure.append(caption, canvas); views.append(figure); return context;
      });
      function render() {
        const index = Number(slider.value);
        if (!Number.isInteger(index) || index < 0 || index >= samples.length) return;
        const sample = samples[index];
        stamp.textContent = `${hasInitial && index === 0 ? "Initial state" :
          `Accepted step ${index + 1 - Number(hasInitial)} of ${accepted.length}`} · t = ${metric(sample.time_s)} s`;
        for (const [axis, ctx] of contexts.entries()) {
          ctx.clearRect(0, 0, 420, 420);
          for (const [i, track] of tracks.entries()) {
            ctx.strokeStyle = i === 0 ? "#a0adb2" : "#145b68"; ctx.lineWidth = i === 0 ? 0.5 : 1;
            ctx.beginPath();
            for (const triangle of topology[i]) {
              for (const [j, vertex] of triangle.entries()) {
                const p = sample[track][vertex], x = 210 + (p[axis] - center[axis]) * scale, y = 210 - (p[2] - center[2]) * scale;
                if (j === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
              }
              ctx.closePath();
            }
            ctx.stroke();
          }
        }
      }
      slider.addEventListener("input", render); d.append(note, label, stamp, views); render();
    } catch (e) {
      const message = document.createElement("p"); message.textContent = e.message; d.append(message);
    }
  });
}
function wetFilmResult(parent, key, history) {
  const film = history.wet_film;
  if (!film) return;
  const totals = table(parent, `${key}: finite body water and latent energy`, ["Quantity", "Value", "Unit"]);
  for (const [label, field, unit] of [
    ["Initial body-side liquid", "starting_liquid_mass_kg", "kg"],
    ["Prescribed liquid supplied", "supplied_mass_kg", "kg"],
    ["Covered-path potential demand", "potential_evaporation_mass_kg", "kg"],
    ["Modeled evaporation from available liquid", "evaporated_mass_kg", "kg"],
    ["Unmet covered-path demand", "unmet_demand_mass_kg", "kg"],
    ["Remaining body-side liquid", "remaining_liquid_mass_kg", "kg"],
    ["Latent energy supplied by prescribed body reservoir", "body_reservoir_latent_energy_j", "J"],
    ["Liquid mass balance residual", "mass_balance_error_kg", "kg"]
  ]) cells(totals, [label, metric(film.totals?.[field]), unit]);
  const note = document.createElement("p");
  note.textContent = "Liquid belongs to the same body triangles throughout motion. Each interval uses its declared average covered-path demand and constant liquid supply; " +
    "evaporation is limited by the liquid available to each triangle. Uncovered transfer remains unknown. " +
    "The prescribed body reservoir supplies the reported latent energy. Textile temperatures, garment mass and wet-material properties are unchanged by this body-film model. " +
    "The source and model assumptions are retained below; this is not a sweat-production or wearer-comfort prediction.";
  parent.append(note);
  const samples = table(parent, `${key}: body water at accepted motion samples`, ["Time (s)", "Supplied liquid (kg)",
    "Modeled evaporation (kg)", "Unmet demand (kg)", "Remaining liquid (kg)", "Body latent energy (J)", "Mass balance residual (kg)"]);
  for (const sample of history.samples || []) {
    const water = sample.wet_film?.totals;
    cells(samples, [metric(sample.time_s), metric(water?.supplied_mass_kg), metric(water?.evaporated_mass_kg),
      metric(water?.unmet_demand_mass_kg), metric(water?.remaining_liquid_mass_kg),
      metric(water?.body_reservoir_latent_energy_j), metric(water?.mass_balance_error_kg)]);
  }
  detail(parent, `${key}: body water source, model assumptions and per-triangle ledger`, film);
}
function vaporHistoryResult(parent, key, history, statusLabel, refusal) {
  const summary = table(parent, `${key}: moving layered vapor potential`, ["History", "Duration (s)",
    "Accepted samples", "Potential energy (J)", "Potential water mass (kg)", "Integration rule"]);
  cells(summary, [statusLabel, metric(history?.duration_s),
    metric(history?.samples?.length), vaporMetric(history?.totals, "potential_energy_j"),
    vaporMetric(history?.totals, "potential_mass_kg"), history?.integration || "Unknown"]);
  const note = document.createElement("p");
  const integration = history?.integration === "right_endpoint" ?
    "The right-endpoint rule holds each accepted sample's potential flow over its preceding interval; " :
    history?.integration === "composite_trapezoid" ?
      "The composite trapezoidal rule averages each interval's retained start and end flows; " :
      "The integration rule is retained in the report; ";
  note.textContent = "Each accepted body/cloth sample recomputes the covered layer paths under the fixed declared vapor boundaries. " +
    integration + "refining motion samples can change the integral. " +
    "Uncovered body transfer remains unknown, and complete-domain comparisons require coverage at every sample. " +
    (history?.wet_film ?
      "Potential transfer and the finite body-water ledger below retain separate meanings; the latter limits modeled evaporation by the supplied liquid." :
      "These potentials do not model water supply, actual evaporation, moisture storage or cooling supplied to dry heat.");
  parent.append(note);
  vaporBoundsNote(parent, history?.totals, `${key}: moving vapor source bounds`);
  detail(parent, `${key}: moving vapor sources, sample coverage and exact replay (display only)`,
    history || { refusal });
  if (!history) return;
  const samples = table(parent, `${key}: vapor coverage at accepted motion samples`, ["Time (s)", "Interval (s)",
    "Covered body (m²)", "Uncovered body (m²)", "Unassigned reference cloth (m²)",
    "Potential power (W)", "Potential water flow (kg/s)", "Interval energy (J)", "Interval water mass (kg)"]);
  for (const sample of [history.initial, ...(history.samples || [])].filter(Boolean)) {
    const coverage = sample.result?.coverage, initial = sample.time_s === 0;
    cells(samples, [metric(sample.time_s), initial ? "Initial only" : metric(sample.dt_s),
      metric(coverage?.covered_area_m2), metric(coverage?.uncovered_area_m2), metric(coverage?.unassigned_cloth_reference_area_m2),
      vaporMetric(coverage, "covered_potential_heat_flow_w"), vaporMetric(coverage, "covered_potential_mass_flow_kg_s"),
      initial ? "Initial only" : vaporMetric(sample, "potential_energy_j"),
      initial ? "Initial only" : vaporMetric(sample, "potential_mass_kg")]);
  }
  wetFilmResult(parent, key, history);
}
function motionVaporComparison(op, parent, key, index, motion, dependency, requested, filePrefix = "") {
  const output = motion?.vapor_coverage_history;
  if (!requested?.vapor_coverage_history && !output) return;
  const history = available(output);
  vaporHistoryResult(parent, key, history, output ? state(output) : `Unavailable: ${state(dependency)}`,
    output?.refusal || dependency?.refusal);
  if (typeof history?.replay_request_json === "string") {
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    link(op, actions, { name: `${filePrefix}alternative-${index + 1}-motion-vapor-coverage-replay.json`,
      text: history.replay_request_json, mime: "application/json" });
  }
}
function steadyHeatBounds(parent, label, result) {
  const steady = result?.steady;
  if (!steady || (!Object.hasOwn(steady, "reported_bounds") && !steady.bounds_unavailable?.length)) return;
  const bounds = steady.reported_bounds?.power_w;
  const t = table(parent, `${label}: steady source-reported heat range`,
    ["Signed nominal power (W)", "Signed reported power range (W)", "Range availability"]);
  cells(t, [metric(steady.power_w), bounds ? `[${bounds.map(metric).join(", ")}]` : "Unknown",
    bounds ? "Complete selected source endpoints" :
      (steady.bounds_unavailable || []).map(reason => reason.message || reason).join("; ") || "No complete source range"]);
  const note = document.createElement("p");
  note.textContent = "These conditional steady ranges retain the exact evaluated geometry, reservoir temperatures, films and fixed interfaces. " +
    "Each consumed constant material law needs complete reported resistance endpoints or thickness and conductivity endpoints. " +
    "Nominal power remains available when ranges are unsupported or incomplete. The ranges do not describe geometric uncertainty, transient heat or measured garment warmth.";
  parent.append(note);
}
function thermalBridges(parent, label, result) {
  const bridges = result?.thermal_bridges;
  if (!bridges) return;
  const totals = table(parent, `${label}: qualified seam and closure heat`,
    ["Baseline power (W)", "Seam/closure excess (W)", "Combined power (W)"]);
  cells(totals, [metric(bridges.baseline_power_w), metric(bridges.bridge_power_w), metric(bridges.total_power_w)]);
  const paths = table(parent, `${label}: current qualified paths`, ["Path", "Instance", "Current length (m)", "Excess power (W)"]);
  for (const path of bridges.paths || []) cells(paths, [path.id, path.instance, metric(path.length_m), metric(path.power_w)]);
  const note = document.createElement("p");
  note.textContent = "Combined power includes each source-qualified seam or closure excess once. " +
    "The unbridged stack remains visible as the baseline. These signed steady rates do not supply transient heat or establish measured garment warmth.";
  parent.append(note);
  detail(parent, `${label}: path qualifications, projected ownership and replay inputs (display only)`, {
    surface_model_sha256: bridges.surface_model_sha256, bridges,
    qualifications: result.request?.thermal_bridges });
}
function coverageResult(parent, result) {
  steadyHeatBounds(parent, "Steady heat", result);
  thermalBridges(parent, "Coverage", result);
  return { steady: result.steady,
    ...(Object.hasOwn(result, "thermal_bridges") ? { thermal_bridges: result.thermal_bridges } : {}),
    comfort_claim: result.comfort_claim, empirical_validation_claim: result.empirical_validation_claim };
}
function motionComparison(op, parent, rows, simulations, requested, filePrefix = "") {
  if (!requested && ![...simulations.values()].some(o => available(o)?.motion)) return;
  const note = document.createElement("p");
  note.textContent = "Motion continues the initial sewn cloth; it does not re-drape each pose. Motion enters the computed frontier through explicitly declared motion objectives or required consumers. " +
    "Endpoint steady snapshots are separate from explicitly requested transient histories. Successful motion is not a settled-garment or empirical fit claim.";
  parent.append(note);
  const t = table(parent, "Prescribed motion — complete trajectory or refusal", ["Alternative", "Motion", "Initial drape settled",
    "Duration (s)", "Accepted substeps", "Attempted substeps", "Peak absolute panel-edge strain (%)"]);
  for (const [index, row] of rows.entries()) {
    const evaluation = simulations.get(row.key), initial = available(evaluation), result = available(initial?.motion);
    cells(t, [row.key, initial ? state(initial.motion) : state(evaluation),
      initial?.drape?.settled === true ? "Yes — also review initial flags" : initial?.drape?.settled === false ? "NO — review initial state" : "Unknown",
      metric(result?.duration_s), metric(result?.samples?.length), metric(result?.trials), metric(result?.metrics?.max_absolute_edge_strain_pct)]);
    materialStrainComparison(parent, row.key, result?.material_strain, initial?.motion || evaluation,
      initial?.material_strain && initial.material_strain.status !== "not_requested", true);
    contactLoadsComparison(parent, row.key, result?.contact_loads, initial?.motion || evaluation,
      initial?.contact_loads && initial.contact_loads.status !== "not_requested", true);
    transientHeatComparison(parent, row.key, result, initial?.motion || evaluation, requested);
    motionVaporComparison(op, parent, row.key, index, result, initial?.motion || evaluation, requested, filePrefix);
    if (!result) continue;
    const endpoints = table(parent, `${row.key}: endpoint steady snapshots — not transient heat`, ["Time (s)", "Arm angle (degrees)",
      "Surface heat", "Signed surface power (W)", "Body coverage", "Signed coverage power (W)"]);
    for (const endpoint of result.keyframes) {
      cells(endpoints, [metric(endpoint.time_s), metric(endpoint.arm_abduction_deg), state(endpoint.surface),
        metric(available(endpoint.surface)?.steady?.power_w), state(endpoint.coverage), metric(available(endpoint.coverage)?.steady?.power_w)]);
      steadyHeatBounds(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s independent sheets`, available(endpoint.surface));
      steadyHeatBounds(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s body coverage`, available(endpoint.coverage));
      thermalBridges(parent, `${row.key}: endpoint ${metric(endpoint.time_s)} s`, available(endpoint.coverage));
    }
    if (result.keyframes.some(e => e.vapor_surface && e.vapor_surface.status !== "not_requested")) {
      const vapor = table(parent, `${row.key}: endpoint vapor potentials — independent sheets`,
        ["Time (s)", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const endpoint of result.keyframes) {
        const transfer = available(endpoint.vapor_surface);
        cells(vapor, [metric(endpoint.time_s), state(endpoint.vapor_surface), metric(transfer?.area_m2),
          vaporMetric(transfer, "potential_heat_flow_w"), vaporMetric(transfer, "potential_mass_flow_kg_s")]);
      }
    }
    if (result.keyframes.some(e => e.vapor_coverage && e.vapor_coverage.status !== "not_requested")) {
      const note = document.createElement("p");
      note.textContent = "Layered vapor transfer follows the cloth and body at each endpoint. These are steady potential rates for covered regions; " +
        "endpoint rates are separate from the optional history integral. Neither result models sweat supply or cooling supplied to the dry heat simulation.";
      parent.append(note);
      const vapor = table(parent, `${row.key}: endpoint vapor potentials — covered layers`,
        ["Time (s)", "Vapor coverage", "Covered body (m²)", "Uncovered body (m²)",
          "Potential power (W)", "Potential water flow (kg/s)"]);
      for (const [j, endpoint] of result.keyframes.entries()) {
        const transfer = available(endpoint.vapor_coverage), coverage = transfer?.coverage;
        cells(vapor, [metric(endpoint.time_s), state(endpoint.vapor_coverage), metric(coverage?.covered_area_m2),
          metric(coverage?.uncovered_area_m2), vaporMetric(coverage, "covered_potential_heat_flow_w"),
          vaporMetric(coverage, "covered_potential_mass_flow_kg_s")]);
        detail(parent, `${row.key}: endpoint ${j + 1} vapor layers, sources and boundary conditions (display only)`,
          transfer || { refusal: endpoint.vapor_coverage?.refusal });
        if (typeof transfer?.replay_request_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, { name: `${filePrefix}alternative-${index + 1}-motion-endpoint-${j + 1}-vapor-coverage-replay.json`,
            text: transfer.replay_request_json, mime: "application/json" });
        }
      }
    }
    detail(parent, `${row.key}: motion identities, material history and endpoint refusals (display only)`, {
      path_sha256: result.path_sha256, initial_positions_sha256: result.initial_positions_sha256,
      initial_body_collision_sha256: result.initial_body_collision_sha256, initial_drape_gating: result.initial_drape_gating,
      retained_vertex_samples: result.retained_vertex_samples, pairs_tested: result.pairs_tested,
      membrane_history: result.membrane_history, bending_history: result.bending_history,
      membrane_plasticity_history: result.membrane_plasticity_history,
      endpoints: result.keyframes.map(e => ({ time_s: e.time_s, positions_sha256: e.positions_sha256,
        body_collision_sha256: e.body_collision_sha256, surface_refusal: e.surface?.refusal, coverage_refusal: e.coverage?.refusal,
        vapor_refusal: e.vapor_surface?.refusal, vapor_coverage_refusal: e.vapor_coverage?.refusal })),
      scope: result.scope, thermal_basis: result.thermal_basis, vapor_basis: result.vapor_basis });
    motionSamples(parent, row.key, result);
  }
}
function comparison(op, r, parent = sections.get(op).comparison, { skipManufacturing = false, filePrefix = "", casePolicies = {} } = {}) {
  if (op === "screen") {
    screening(parent, r);
    return { schema: r.schema, basis: r.basis, allow_mixed_protocols: r.allow_mixed_protocols,
      scalar_frontier: r.frontier, request_sha256: r.request_sha256, notes: r.notes };
  }
  if (["ftx-material-scenario-matrix-study/1", "ftx-design-scenario-matrix-study/1"].includes(r.schema)) {
    return scenarioMatrixComparison(op, parent, r);
  }
  if (r.comparison) objectiveComparison(parent, r.comparison);
  const study = r.manufacturing || r;
  if (!skipManufacturing) manufacturingComparison(op, parent, study, r.comparison, casePolicies);
  if (r.manufacturing) {
    const simulations = new Map(r.simulations.map(row => [row.key, row.evaluation]));
    const sim = table(parent, "Initial static-drape results — review drape flags before interpreting heat", ["Alternative", "Drape",
      "Gating clear", "Cloth mass (kg)", "Surface heat", "Signed surface power (W)", "Body coverage", "Signed coverage power (W)"]);
    for (const row of study.alternatives) {
      const outcome = simulations.get(row.key), result = available(outcome), m = result?.metrics || {};
      cells(sim, [row.key, state(outcome), m.simulation_gating_clear === true ? "Yes" :
        m.simulation_gating_clear === false ? "NO — review flags" : "Unknown", metric(m.cloth_mass_kg),
        state(result?.surface), metric(m.independent_surface_power_w), state(result?.coverage), metric(m.body_coverage_power_w)]);
      detail(parent, `${row.key}: simulation flags, identities and thermal refusals`, {
        refusal: outcome?.refusal, settled: result?.drape?.settled, flags: result?.drape?.flags,
        scenario_sha256: result?.scenario_sha256, body_geometry_sha256: result?.body_geometry_sha256,
        placements_sha256: result?.placements_sha256, positions_sha256: result?.positions_sha256,
        surface_refusal: result?.surface?.refusal, coverage_refusal: result?.coverage?.refusal, note: result?.note });
      steadyHeatBounds(parent, `${row.key}: initial independent sheets`, available(result?.surface));
      steadyHeatBounds(parent, `${row.key}: initial body coverage`, available(result?.coverage));
      thermalBridges(parent, `${row.key}: initial coverage`, available(result?.coverage));
      materialStrainComparison(parent, row.key, result?.material_strain, outcome, r.scenario?.scenario?.material_strain);
      contactLoadsComparison(parent, row.key, result?.contact_loads, outcome, r.scenario?.scenario?.contact_loads);
    }
    vaporCoverageComparison(op, parent, study.alternatives, simulations, r.scenario?.scenario?.vapor_coverage, filePrefix);
    if ([...simulations.values()].some(o => {
      const vapor = available(o)?.vapor_surface;
      return vapor && vapor.status !== "not_requested";
    })) {
      const vapor = table(parent, "Initial drape vapor potentials — independent sheets",
        ["Alternative", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const row of study.alternatives) {
        const outcome = simulations.get(row.key), initial = available(outcome), transfer = available(initial?.vapor_surface);
        cells(vapor, [row.key, initial ? state(initial.vapor_surface) : state(outcome), metric(transfer?.area_m2),
          vaporMetric(transfer, "potential_heat_flow_w"), vaporMetric(transfer, "potential_mass_flow_kg_s")]);
        vaporBoundsNote(parent, transfer, `${row.key}: independent-sheet vapor bounds`);
        detail(parent, `${row.key}: vapor conditions, source columns and refusal (display only)`,
          transfer || { refusal: initial?.vapor_surface?.refusal || outcome?.refusal });
      }
    }
    attachedMassComparison(parent, study.alternatives, simulations);
    motionComparison(op, parent, study.alternatives, simulations, r.scenario?.scenario?.motion, filePrefix);
    detail(parent, "Retained body fit, scenario options and environment (display only)", r.scenario);
  }
  return { schema: r.schema, source_pattern_sha256: study.source_pattern_sha256, request_sha256: r.request_sha256,
    geometry_changed: study.geometry_changed, physical_validation_claim: study.physical_validation_claim,
    hardware_execution_authorized: study.hardware_execution_authorized, notes: study.notes,
    computed_frontier: r.comparison?.frontier ?? null,
    comparison_basis: r.comparison ? (r.comparison.policy.basis === "reported_bounds" ?
      "Complete source-reported ranges evaluated by Rust at fixed declared inputs; unresolved overlap and unknown ranges remain explicit." :
      "Declared nominal objectives evaluated by Rust; conditional Pareto set, not an empirical fit/warmth claim or global optimum.") :
      "Actual consumer results; no computed objectives requested. Unknowns are not zero, and no ranking is inferred by this page." };
}
function scenarioMatrixComparison(op, parent, r) {
  const note = document.createElement("p");
  note.textContent = "Named scenarios share the same original pattern, declared design and material alternatives, and body source. " +
    "Manufacturing is evaluated once; each case starts a fresh drape with its own conditions and optional motion. " +
    "Case decisions and physical units remain separate. No browser score or average replaces the native comparison.";
  parent.append(note);
  if (r.comparison) {
    const result = r.comparison, frontier = document.createElement("p");
    frontier.textContent = `Computed cross-case frontier: ${result.frontier.length ? result.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
      "Admission requires every case's declared gates and limits. Review each case's retained readings and exclusions below.";
    parent.append(frontier);
    const t = table(parent, "Named scenario decisions — authoritative Rust comparison",
      ["Alternative", "Across cases", "Dominated by", "Case", "Case admission", "Retained readings"]);
    for (const row of result.alternatives) {
      for (const key of result.case_order) {
        const candidate = row.cases[key], policy = result.policies[key];
        const quantities = [...(policy?.objectives || []), ...(policy?.constraints || [])]
          .filter((q, i, all) => all.findIndex(other => other.metric === q.metric) === i);
        const readings = quantities.map(({ metric: name }) => {
          const q = candidate?.readings?.[name];
          if (!q) return `${name}: Unknown`;
          const bounds = q.reported_bounds ? `; reported range [${q.reported_bounds.map(metric).join(", ")}] ${q.unit}` :
            metricUsesBounds(policy, name) ? "; reported range unknown" : "";
          return `${name}: ${metric(q.value)} ${q.unit}${bounds}`;
        });
        cells(t, [row.key, row.eligible ? "Eligible" : "Excluded", row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
          key, candidate ? (candidate.eligible ? "Eligible" : `Excluded: ${candidate.exclusions.map(e => e.message).join("; ")}`) : "Unknown",
          readings.length ? readings.join(" | ") : "No declared quantities"]);
      }
    }
    for (const pair of result.incomparable) {
      const p = document.createElement("p");
      p.textContent = `${pair.alternatives.join(" / ")}: ${pair.reason} (cases: ${pair.cases.join(", ")}).`;
      parent.append(p);
    }
    detail(parent, "Cross-case policies, identities and decisions (display only)", result);
  } else {
    const unavailable = document.createElement("p");
    unavailable.textContent = "No cross-case comparison is available. Comparison policies are required for every case; " +
      "available per-case comparisons and independent outcomes remain below.";
    parent.append(unavailable);
  }
  for (const message of r.notes || []) {
    const note = document.createElement("p"); note.textContent = message; parent.append(note);
  }
  const shared = document.createElement("section"), heading = document.createElement("h4");
  heading.textContent = "Shared manufacturing — evaluated once"; shared.append(heading); parent.append(shared);
  const casePolicies = r.comparison?.policies || Object.fromEntries(r.cases
    .filter(item => item.comparison?.policy).map(item => [item.key, item.comparison.policy]));
  comparison(op, r.manufacturing, shared, { casePolicies });
  for (const [index, item] of r.cases.entries()) {
    const group = document.createElement("section"), heading = document.createElement("h4");
    heading.textContent = `Case ${index + 1}: ${item.key}`; group.append(heading); parent.append(group);
    comparison(op, { ...item, schema: r.schema === "ftx-design-scenario-matrix-study/1" ?
      "ftx-design-scenario-study/1" : "ftx-material-scenario-study/1", manufacturing: r.manufacturing }, group,
      { skipManufacturing: true, filePrefix: `case-${index + 1}-` });
    detail(group, "Case request identity (display only)", { key: item.key, request_sha256: item.request_sha256 });
  }
  return { schema: r.schema, request_sha256: r.request_sha256, source_pattern_sha256: r.manufacturing.source_pattern_sha256,
    cases: r.cases.map(item => item.key), max_evaluations: r.max_evaluations,
    planned_evaluations: r.planned_evaluations, evaluations: r.evaluations,
    computed_cross_case_frontier: r.comparison?.frontier ?? null,
    physical_validation_claim: r.manufacturing.physical_validation_claim,
    scope: "Fixed-input named cases, with their native outcomes, refusals and policies retained independently." };
}
function designTreatments(op, parent, design, prefix, label) {
  if (!Array.isArray(design?.treatments) || !design.treatments.length) return;
  const note = document.createElement("p");
  note.textContent = "These are exact intermediate treatment stages. Review final geometry, fit and material binding admission before using a design. " +
    "Completed stages can remain available for inspection if a later operation refuses.";
  parent.append(note);
  for (const stage of design.treatments) {
    const step = stage.operation_index + 1, report = stage.report;
    const heading = document.createElement("h4");
    heading.textContent = `${label}: operation ${step} — ${report.mode === "compensate" ? "Cutting compensation" : "Material treatment"}`;
    parent.append(heading);
    treatmentResult(parent, report);
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    for (const [suffix, field] of [["before-pattern", "source_pattern_json"], ["after-pattern", "pattern_json"], ["request", "frozen_request_json"]]) {
      if (typeof report[field] === "string") link(op, actions, { name: `${prefix}-treatment-${step}-${suffix}.json`,
        text: report[field], mime: "application/json" });
    }
  }
}
function designComparison(op, parent, study, casePolicies = {}) {
  if (!study.alternatives.some(row => row.design)) return;
  const note = document.createElement("p");
  note.textContent = "Each pattern design starts from the original source, then applies its explicit alterations and material choices. " +
    "The baseline retains the source geometry. Cutting, sewing and requested simulations use each candidate's actual pattern. " +
    "Review alteration validation and consumer refusals before interpreting the computed comparison.";
  parent.append(note);
  const grid = study.design_search;
  if (grid) {
    if (grid.schema === "ftx-design-adaptive-search/1") {
      const guidance = grid.consumer_guidance;
      const pareto = guidance?.pareto_frontier;
      const matrixPareto = pareto && guidance.case_aggregation === "pareto";
      const policies = matrixPareto ? (guidance.case_order || []).map(key =>
        ({ key, policy: guidance.case_policies?.[key] })) : [];
      const objectives = pareto ? guidance.objectives || [] : guidance?.objective ? [guidance.objective] : [];
      const policyFor = key => policies.find(item => item.key === key)?.policy || casePolicies[key];
      const basisText = basis => basis === "reported_bounds" ? "Source-reported ranges" :
        basis === "nominal" ? "Nominal values" : "Unknown";
      const bounded = guidance?.basis === "reported_bounds";
      const worstCase = guidance?.case_aggregation === "worst_case";
      const limitingCases = (nominal, endpoints) => nominal ?
        `; nominal limiting case: ${nominal}` + (endpoints ?
          `; lower endpoint case: ${endpoints[0]}; upper endpoint case: ${endpoints[1]}` : "") : "";
      const readingText = (reading, needsBounds = bounded) => {
        if (!reading) return "Unknown";
        const range = reading.reported_bounds ?
          `; reported range [${reading.reported_bounds.map(metric).join(", ")}] ${reading.unit || ""}` :
          needsBounds ? "; reported range unknown" : "";
        return `${metric(reading.value)} ${reading.unit || ""}${range}`;
      };
      if (guidance) {
        const search = table(parent, matrixPareto ? "Multi-case Pareto adaptive alteration search" :
          pareto ? "Pareto-guided adaptive alteration search" : "Consumer-guided adaptive alteration search", ["Fit evaluations", "Consumer probes",
          "Planned callback slots (upper bound)", "Selected geometries", "Stop reason",
          ...(pareto ? ["Evaluated nondominated trials", "Retained trials", "Omitted by capacity"] :
            [bounded ? "Retained trial" : "Best trial", bounded ? "Retained objective" : "Best objective"])]);
        const reading = guidance.best_reading;
        cells(search, [metric(grid.evaluations), metric(guidance.evaluations), metric(guidance.reserved_evaluations),
          metric(grid.selected_designs), grid.stop_reason?.replaceAll("_", " ") ?? "Unknown",
          ...(pareto ? [(pareto.evaluated_trial_keys || []).join(", ") || "None admitted",
            (pareto.retained_trial_keys || []).join(", ") || "None selected",
            (pareto.unretained_trial_keys || []).join(", ") || "None"] :
            [guidance.best_trial_key ?? "Unknown", readingText(reading) +
              (worstCase && reading ? limitingCases(guidance.best_case_key, guidance.best_reported_bound_case_keys) : "")])]);
        const objective = document.createElement("p");
        objective.textContent = `Guidance material: ${guidance.material_key ?? "Unknown"}. ` +
          (guidance.case_key ? `Guidance case: ${guidance.case_key}. Every declared case must admit a selected trial: ${(guidance.case_order || []).join(", ")}. ` : "") +
          (worstCase ? `Case aggregation: worst case. Every declared case must admit a selected trial: ${(guidance.case_order || []).join(", ")}. ` +
            (bounded ? "Reported lower and upper endpoints each retain their own limiting case; they may differ from the nominal limiting case. " : "") : "") +
          (matrixPareto ? `Guidance covers every named case: ${(guidance.case_order || []).join(", ")}. ` +
            "Every declared case must admit a selected trial. Each case keeps its own objectives, directions, comparison basis and source protocols. " +
            "Dominance requires no worse outcomes in every case and a strict improvement in at least one case. " :
            pareto ? `Declared objectives: ${objectives.map(item => `${item.metric} (${item.direction})`).join("; ") || "Unknown"}. ` :
              `Declared objective: ${guidance.objective?.metric ?? "Unknown"} (${guidance.objective?.direction ?? "Unknown"}). `) +
          (pareto ? `Retention capacity: ${metric(pareto.retention_capacity)} geometries. ` +
            "Polling also explores incomparable admitted designs, including those omitted from the output cache. " +
            "The displayed frontier covers evaluated trials only; the retained subset is not a complete frontier or a single winner. " +
            "No weighted score is introduced. " : "") +
          (matrixPareto ? "Objective bases are declared separately for each case below. Missing required ranges cannot qualify; nominal readings do not supply missing endpoints. " :
            bounded ? "Objective basis: source-reported ranges. Bounded constraints admit the entire reported range; missing required ranges cannot qualify. " +
              (pareto ? "Range comparisons retain every declared objective and direction; incomparable ranges do not acquire a total ranking. " :
                "Search advances only when one range is strictly preferred. Overlapping, touching or equal ranges remain unresolved; retained trials do not form a total ranking. ") :
              "Objective basis: nominal values. ") +
          "Each constraint uses its explicit basis or inherits its case policy; nominal guards do not acquire uncertainty bounds. " +
          "Rust evaluates the nominated material with the ordinary manufacturing and requested scenario consumers. " +
          "Existing source admission, required consumers, protocol rules and inclusive constraints govern each probe. " +
          "Callback slots are an upper bound on attempted work, not a preapproved simulation allowance; cumulative consumer budgets still apply.";
        parent.append(objective);
        if (matrixPareto) {
          const cases = table(parent, "Adaptive guidance policies by operating case",
            ["Case", "Objective basis", "Objectives and directions", "Inclusive constraints", "Required consumers", "Protocol comparison"]);
          for (const { key, policy } of policies) {
            cells(cases, [key, basisText(policy.basis),
              policy.objectives.map(item => `${item.metric} (${item.direction})`).join("; "),
              (policy.constraints || []).map(item => `${item.metric}: minimum ${item.minimum == null ? "Unbounded" : metric(item.minimum)}; ` +
                `maximum ${item.maximum == null ? "Unbounded" : metric(item.maximum)} (${basisText(item.basis || policy.basis)})`).join("; ") || "None",
              (policy.required_consumers || []).join(", ") || "Objective consumers",
              policy.allow_mixed_protocols ? "Mixed protocols explicitly admitted" : "Matching case protocols required"]);
            detail(parent, `${key}: complete adaptive guidance policy (display only)`, policy);
          }
        }
      } else {
        const search = table(parent, "Fit-directed adaptive alteration search", ["Fit evaluations", "Passed flat gates",
          "Selected for consumers", "Stop reason", "Best maximum violation (mm)", "Best total violation (mm)"]);
        cells(search, [metric(grid.evaluations), metric(grid.qualified_designs), metric(grid.selected_designs),
          grid.stop_reason?.replaceAll("_", " ") ?? "Unknown", metric(grid.best_max_fit_band_violation_mm),
          metric(grid.best_total_fit_band_violation_mm)]);
      }
      const scope = document.createElement("p");
      scope.textContent = (guidance ?
        "Flat geometry and selected-body fit gates precede every consumer probe. The search keeps better admitted geometries while polling the declared domain; " +
        "all trials remain inspectable, including displaced candidates and failed probes. Selected geometries are then crossed with the authored material choices. " :
        "The adaptive search uses unrounded distances outside your declared fit bands to choose the next alteration values. ") +
        "Its bounds and step sizes define where it can search; the stopping reason does not prove that unvisited values are infeasible " +
        "or that a global optimum was found. Selected designs use the ordinary manufacturing and scenario consumers, and the original baseline remains visible. " +
        "Computed target compliance does not establish physical garment fit.";
      parent.append(scope);
      const trials = table(parent, "Adaptive alteration trials", ["Pattern design", "Variables (mm)",
        "Maximum violation (mm)", "Total violation (mm)", "Flat admission",
        ...(guidance ? ["Consumer admission", pareto ? "Declared objectives" : "Declared objective",
          ...(pareto ? ["Evaluated frontier"] : [])] : []), "Downstream selection", "Reason"]);
      for (const [i, row] of (grid.rows || []).entries()) {
        const design = row.design || {}, fit = available(design.fit);
        const passed = design.validation?.passed === true && fit?.passed === true;
        const candidate = row.consumer_comparison;
        const reading = worstCase && bounded ? row.consumer_worst_case?.reading :
          candidate?.readings?.[guidance?.objective?.metric];
        const caseDecisions = row.consumer_cases || [];
        const attempted = matrixPareto ? caseDecisions.length > 0 : !!candidate;
        const admitted = !row.refusal && (matrixPareto ?
          policies.length > 0 && policies.every(policy => caseDecisions.some(item =>
            item.key === policy.key && item.comparison?.eligible === true)) :
          candidate?.eligible === true && caseDecisions.every(item => item.comparison?.eligible === true));
        const reasons = [row.refusal?.message, row.consumer_refusal?.message,
          ...(candidate?.exclusions || []).map(e => e.message),
          ...(matrixPareto ? caseDecisions.flatMap(item => [
            ...(item.comparison?.exclusions || []).map(e => `${item.key}: ${e.message}`),
            item.refusal?.message ? `${item.key}: ${item.refusal.message}` : null]) : [])]
          .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
        cells(trials, [design.key ?? "Unknown",
          design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => `${key}: ${metric(value)}`).join("; ") : "Unknown",
          metric(row.max_fit_band_violation_mm), metric(row.total_fit_band_violation_mm),
          passed ? "Passed declared flat gates" : "Refused",
          ...(guidance ? [attempted ? (admitted ? "Eligible" : "Excluded") : "Not attempted",
            matrixPareto ? policies.map(({ key, policy }) => {
              const decision = caseDecisions.find(item => item.key === key);
              return `${key} [${basisText(policy.basis)}]: ` + policy.objectives.map(item =>
                `${item.metric} (${item.direction}): ${readingText(decision?.comparison?.readings?.[item.metric], policy.basis === "reported_bounds")}`).join("; ");
            }).join(" | ") || "Unknown" :
              pareto ? objectives.map(item => `${item.metric}: ${readingText(candidate?.readings?.[item.metric])}`).join("; ") || "Unknown" :
                readingText(reading) + (worstCase && reading ?
                limitingCases(row.consumer_case_key, row.consumer_worst_case?.reported_bound_case_keys) : ""),
            ...(pareto ? [admitted ? ((pareto.evaluated_trial_keys || []).includes(design.key) ?
              "Nondominated among evaluated trials" : "Dominated among evaluated trials") : "Not admitted"] : [])] : []),
          row.selected === true ? "Selected" : "Not selected",
          reasons || (passed ? "Passed declared targets" : "Inspect retained validation")]);
        if (row.consumer_cases?.length) {
          const cases = table(parent, `${design.key ?? "Unknown design"}: operating-case admission`,
            ["Case", "Admission", "Readings", "Exclusions or consumer refusal"]);
          for (const item of row.consumer_cases) cells(cases, [item.key,
            item.comparison?.eligible === true ? "Eligible under declared policy" : "Excluded",
            Object.entries(item.comparison?.readings || {}).map(([key, value]) =>
              `${key}: ${readingText(value, policyFor(item.key) ? metricUsesBounds(policyFor(item.key), key) :
                bounded && objectives.some(item => item.metric === key))}`).join("; ") || "Unavailable",
            [...(item.comparison?.exclusions || []).map(reason => reason.message), item.refusal?.message]
              .filter(Boolean).join("; ") || "None"]);
        }
        if (typeof design.altered_pattern_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, {name: `adaptive-design-${i + 1}-pattern.json`, text: design.altered_pattern_json, mime: "application/json"});
        }
        designTreatments(op, parent, design, `adaptive-design-${i + 1}`, design.key ?? "Unknown design");
        detail(parent, `${design.key ?? "Unknown design"}: adaptive trial, unrounded fit and body identity (display only)`, row);
      }
    } else if (grid.fit_screening) {
      const screening = grid.fit_screening;
      const search = table(parent, "Fit screening before manufacturing and simulation",
        ["Declared grid points", "Flat screened", "Passed flat gates", "Rejected", "Selected for consumers",
          "Passing designs not selected", "Unvisited grid points"]);
      cells(search, [metric(grid.total_grid_points), metric(screening.screened_designs), metric(screening.qualified_designs),
        metric(screening.rejected_designs), metric(screening.selected_designs), metric(screening.qualified_designs_omitted),
        metric(screening.unvisited_designs)]);
      const scope = document.createElement("p");
      scope.textContent = (screening.complete_fit_screening === true ?
        "Every declared grid point received flat geometry and fit screening. " :
        "The flat-screening allowance left unvisited grid points; their feasibility remains unknown. ") +
        (screening.all_qualified_designs_selected === true ?
          "Every passing screened design was selected for the requested consumers. " :
          "Some passing screened designs were not selected for manufacturing or simulation; their downstream results remain unknown. ") +
        "The original baseline remains visible. Flat fit targets do not establish physical garment fit or a continuous or global optimum.";
      parent.append(scope);
      const screened = table(parent, "Screened garment designs",
        ["Pattern design", "Variables (mm)", "Flat admission", "Downstream selection", "Reason"]);
      for (const [i, row] of (screening.rows || []).entries()) {
        const design = row.design || {}, fit = available(design.fit);
        const passed = design.validation?.passed === true && fit?.passed === true;
        cells(screened, [design.key ?? "Unknown",
          design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => `${key}: ${metric(value)}`).join("; ") : "Unknown",
          passed ? "Passed declared flat gates" : "Refused",
          row.selected === true ? "Selected" : "Not selected",
          row.refusal?.message || (passed ? "Passed declared targets" : "Inspect retained validation")]);
        if (typeof design.altered_pattern_json === "string") {
          const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
          link(op, actions, {name: `screened-design-${i + 1}-pattern.json`, text: design.altered_pattern_json, mime: "application/json"});
        }
        designTreatments(op, parent, design, `screened-design-${i + 1}`, design.key ?? "Unknown design");
        detail(parent, `${design.key ?? "Unknown design"}: screened operations, body identity and fit evidence (display only)`, row);
      }
    } else {
      const search = table(parent, "Declared alteration search",
        ["Generated designs", "Declared grid points", "Omitted designs", "Enumeration"]);
      cells(search, [metric(grid.generated_designs), metric(grid.total_grid_points), metric(grid.omitted_designs),
        grid.complete_grid_enumeration === true ? "Complete declared grid" :
          grid.budget_exhausted === true ? "Stopped at design budget" : "Incomplete"]);
      const scope = document.createElement("p");
      scope.textContent = grid.complete_grid_enumeration === true ?
        "Every declared grid point was generated. Candidate validation and consumer results still govern eligibility; this is not a continuous or global optimum." :
        "The frontier is conditional on visited designs. Omitted grid points remain unknown and may contain better feasible designs.";
      parent.append(scope);
    }
    detail(parent, grid.schema === "ftx-design-adaptive-search/1" ?
      "Exact adaptive bounds, linked sites, steps and search allowance (display only)" :
      "Exact alteration grid, linked sites and search allowance (display only)", grid);
  }
  const t = table(parent, "Pattern designs and material choices",
    ["Alternative", "Pattern design", "Material choice", "Alteration operations", "Geometry"]);
  const fitTable = study.alternatives.some(row => row.design?.fit || row.design?.variables_mm) ?
    table(parent, "Design variables and declared fit targets",
      ["Alternative", "Variables (mm)", "Body measurement basis", "Fit admission"]) : null;
  for (const [i, row] of study.alternatives.entries()) {
    const design = row.design, fit = available(design?.fit);
    cells(t, [row.key, design?.key ?? "Unknown", design?.material_key ?? "Unknown",
      Array.isArray(design?.operations) ? String(design.operations.length) : "Unknown",
      design?.geometry_changed === true ? "Changed" : design?.geometry_changed === false ? "Unchanged" : "Unavailable"]);
    if (!design) continue;
    if (fitTable) cells(fitTable, [row.key,
      design.variables_mm ? Object.entries(design.variables_mm).map(([key, value]) => key + ": " + metric(value)).join("; ") :
        design.key === "baseline" ? "Original baseline" : "Not parameterized",
      fit?.body_identity?.provenance || (design.fit ? "Unavailable" : "Not requested"),
      !design.fit ? "Not requested" : !fit ? state(design.fit) :
        fit.passed === true ? "Passed declared targets" : "Targets not met"]);
    if (fit) {
      const points = table(parent, row.key + ": canonical flat fit measurements",
        ["Point", "Body (mm)", "Finished (mm)", "Ease (mm)", "Target min / nominal / max (mm)", "Status", "Residual (mm)"]);
      for (const point of Array.isArray(fit.validation?.points) ? fit.validation.points : []) {
        cells(points, [point.label ?? point.id ?? "Unknown", metric(point.body_mm), metric(point.finished_mm),
          metric(point.ease_mm), Array.isArray(point.target_mm) ? point.target_mm.map(value => metric(value)).join(" / ") : "Unknown",
          point.status ?? "Unknown", metric(point.residual_mm)]);
      }
      detail(parent, row.key + ": selected body identity and complete flat fit validation (display only)", fit);
    }
    if (typeof design.altered_pattern_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-design-pattern.json`,
        text: design.altered_pattern_json, mime: "application/json" });
    }
    designTreatments(op, parent, design, `alternative-${i + 1}`, row.key);
    detail(parent, `${row.key}: exact alteration inputs, geometry identities and validation (display only)`, {
      key: design.key, material_key: design.material_key, operations: design.operations, variables_mm: design.variables_mm,
      source_pattern_sha256: design.source_pattern_sha256, altered_pattern_sha256: design.altered_pattern_sha256,
      geometry_changed: design.geometry_changed, validation: design.validation, fit: design.fit, treatments: design.treatments,
      refusal: row.binding?.refusal });
  }
}
function useStudyPattern(op, study, row) {
  if (!ready || pending || sections.get(op).study !== study) return;
  const raw = available(row.binding)?.pattern_json;
  if (typeof raw !== "string") return;
  try {
    const data = parsePattern(raw), retainedOutfit = outfit;
    const garments = retainedOutfit ? JSON.parse(retainedOutfit.composition_json).garments : [];
    resetPatternInput();
    ++roundRead; roundToken = null; window.opener = null;
    roundFiles.delete("patternFile"); $("patternFile").value = "";
    installPattern(raw, `Study candidate ${row.key}`, garments, data);
    outfit = retainedOutfit;
    if (outfit) $("selfContact").checked = true;
    if (roundFiles.size) $("roundImportText").textContent =
      "A study candidate is now the current pattern. Received measurement, fit and body snapshots remain selected. Review them before recomputing this candidate.";
    $("status").textContent = "Candidate pattern loaded with its exact material selections. Review the current body, fit and process inputs, then Freeze selections and bind to recompute consumers. Study alterations now start from this candidate.";
    controls();
  } catch (e) { error(String(e.message || e)); controls(); }
}
function manufacturingComparison(op, parent, study, computed, casePolicies = {}) {
  sections.get(op).study = study;
  designComparison(op, parent, study, casePolicies);
  if (study.alternatives.some(row => typeof available(row.binding)?.pattern_json === "string")) {
    const note = document.createElement("p");
    note.textContent = "Use a candidate pattern to continue with its exact evaluated geometry and material assignments. " +
      "Review its refusals and current body, fit and process inputs before binding and recomputing consumers. Selection does not carry forward a fit verdict or choose a comparison winner.";
    parent.append(note);
  }
  if (study.alternatives.some(row => row.choices)) {
    const choices = table(parent, "Material combinations evaluated by the study", ["Alternative", "Group choices"]);
    for (const row of study.alternatives) cells(choices, [row.key,
      Object.entries(row.choices || {}).map(([group, choice]) => `${group}: ${choice}`).join("; ")]);
  }
  const t = table(parent, "Independent manufacturing outcomes", ["Alternative", "Binding", "Cutting", "Sewing", "Purchasing",
    "Stock area (m²)", "Cut area (m²)", "Complete stock mass (kg)", "Proven least order cost", "Stitch intervals", "Feed-only seconds"]);
  for (const [i, row] of study.alternatives.entries()) {
    const m = row.metrics || {}, purchase = available(row.purchasing);
    const cost = metric(m.least_observed_independent_order_total);
    cells(t, [row.key, state(row.binding), state(row.cutting), state(row.sewing),
      purchase?.complete_offer_coverage === false ? "Incomplete offer coverage" : state(row.purchasing),
      metric(m.stock_area_m2), metric(m.external_cut_area_m2), metric(m.stock_mass_kg),
      cost === "Unknown" ? cost : `${cost} ${purchase?.currency || "(currency unavailable)"}`,
      metric(m.stitch_intervals), metric(m.feed_seconds)]);
    const binding = available(row.binding);
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    if (typeof binding?.pattern_json === "string") {
      link(op, actions, { name: `alternative-${i + 1}-pattern.json`, text: binding.pattern_json, mime: "application/json" });
      const button = document.createElement("button"); button.type = "button";
      button.textContent = `Use ${row.key} pattern`;
      button.onclick = () => useStudyPattern(op, study, row);
      sections.get(op).adoptions.push(button); actions.append(button);
    }
    detail(parent, `${row.key}: frozen assignments and source observations (display only)`, {
      pattern_sha256: binding?.pattern_sha256, choices: row.choices, frozen_changes: row.frozen_changes, pieces: binding?.pieces,
      refusals: { binding: row.binding.refusal, cutting: row.cutting.refusal, finite_stock: row.finite_stock?.refusal,
        cutting_job: row.cutting_job?.refusal, sewing: row.sewing.refusal, purchasing: row.purchasing.refusal } });
    if (purchase?.schema === "ftx-cut-procurement/3") {
      splitProcurementResult(parent, purchase, `${row.key}: whole-marker supplier allocation`);
    }
    sewingWorkholding(parent, available(row.sewing), row.key);
    sewingHandling(parent, available(row.sewing), row.key);
    sewingParallel(parent, available(row.sewing), row.key);
    sewingRouting(parent, available(row.sewing), row.key);
    sewingAccess(parent, available(row.sewing), row.key);
    knifeProgramResult(parent, available(row.cutting)?.program, row.key, available(row.cutting)?.program_refusal);
  }
  if (study.alternatives.some(row => row.finite_stock && row.finite_stock.status !== "not_requested")) {
    const stock = table(parent, "Finite hides and remnants — used physical inventory", ["Alternative", "Finite stock",
      "Used items", "Used gross area (m²)", "Allocated cut area (m²)", "Complete used-stock mass (kg)"]);
    for (const row of study.alternatives) {
      const m = row.metrics || {};
      cells(stock, [row.key, state(row.finite_stock), metric(m.used_stock_count), metric(m.used_stock_gross_area_m2),
        metric(m.finite_stock_external_cut_area_m2), metric(m.used_stock_mass_kg)]);
      const plan = available(row.finite_stock);
      knifeProgramResult(parent, plan?.program, `${row.key}: finite stock`, plan?.program_refusal);
      if (plan) detail(parent, `${row.key}: physical stock, defects and allocated pieces (display only)`, plan);
    }
  }
  if (study.alternatives.some(row => row.sewing_selection && row.sewing_selection.status !== "not_requested")) {
    const routeSearch = study.alternatives.some(row => {
      const result = available(row.sewing_selection);
      return result?.request?.schema === "ftx-sewing-selection/6" || result?.selected_route != null;
    });
    const selection = table(parent, routeSearch ? "Material-specific sewing recipe and route search" : "Material-specific sewing recipe search",
      ["Alternative", "Status", ...(routeSearch ? ["Chosen route"] : []), "Chosen recipes",
      "Trials evaluated", "Search complete", "Budget exhausted", "Listed optimum proved", "Unresolved trials"]);
    for (const row of study.alternatives) {
      const plan = available(row.sewing_selection);
      const choices = plan?.selected_choices ? Object.entries(plan.selected_choices)
        .map(([operation, recipe]) => `${operation}: ${recipe}`).join("; ") :
        plan ? "No listed recipe selected" : "Unavailable";
      cells(selection, [row.key, state(row.sewing_selection),
        ...(routeSearch ? [plan?.selected_route ?? (plan ? "No route selected" : "Unavailable")] : []),
        choices || "Original recipes", metric(plan?.trials?.length),
        plan ? String(plan.search_complete) : "Unknown", plan ? String(plan.budget_exhausted) : "Unknown",
        plan ? String(plan.optimality_proven) : "Unknown", metric(plan?.unresolved_trials)]);
      detail(parent, `${row.key}: sewing recipe choices, work limits and refusals (display only)`,
        plan || { refusal: row.sewing_selection?.refusal });
    }
    const costSearches = study.alternatives.filter(row => {
      const result = available(row.sewing_selection);
      return result?.request?.objective === "minimize_thread_observed_order_cost" ||
        result?.objective_currency != null;
    });
    if (costSearches.length) {
      const costs = table(parent, "Thread-cost-directed recipe search", ["Alternative",
        "Selected complete thread order cost", "Currency", "Listed cost optimum proved", "Unresolved trials"]);
      for (const row of costSearches) {
        const result = available(row.sewing_selection);
        cells(costs, [row.key, metric(result.selected_thread_order_cost), result.objective_currency ?? "Unknown",
          String(result.optimality_proven), metric(result.unresolved_trials)]);
      }
      const routedCosts = costSearches.filter(row => {
        const result = available(row.sewing_selection);
        return result.request?.schema === "ftx-sewing-selection/6" || result.selected_route != null;
      });
      if (routedCosts.length) {
        const trials = table(parent, "Complete thread order costs by recipe and route",
          ["Alternative", "Route", "Recipes", "Complete order cost", "Currency", "Cost availability", "Deadline admission", "Reason"]);
        for (const row of routedCosts) {
          const result = available(row.sewing_selection);
          for (const trial of result.trials || []) {
            const deadline = trial.completion_deadline;
            const cost = trial.refusal ? "Plan refused" :
              trial.objective_unavailable || trial.thread_order_cost == null ? "Unresolved" : "Complete order";
            const admission = deadline ?
              ({ feasible: "Meets deadline", excluded: "Misses deadline (proved)", unresolved: "Unresolved" }[deadline.status] || "Unknown") :
              result.request?.completion_deadline_milliseconds != null ? "Unknown" : "Not requested";
            const reasons = [trial.refusal?.message, trial.objective_unavailable?.message, deadline?.reason]
              .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
            cells(trials, [row.key, trial.route_choice ?? "Unknown",
              Object.entries(trial.choices || {}).map(([operation, recipe]) => `${operation}: ${recipe}`).join("; ") || "Original recipes",
              metric(trial.thread_order_cost), result.objective_currency ?? "Unknown", cost, admission,
              reasons || "Inspect retained source-qualified order"]);
          }
        }
        const scope = document.createElement("p");
        scope.textContent = "Route costs use each authored fixed assignment's actual qualified recipe and complete thread order. " +
          "Eligible-cell assignment searches are not admitted for this cost objective. " +
          "A complete fixed-route order does not require proof of the shortest makespan. " +
          "Any declared deadline is checked independently; a cheap excluded or unresolved trial does not become the selected route.";
        parent.append(scope);
      }
      const scope = document.createElement("p");
      scope.textContent = "Recipe selection uses each complete job's source-qualified thread orders, including supplier minimums, " +
        "increments and applicable price tiers. Trials with incomplete consumption or prices remain unresolved. " +
        "A retained order can be usable before the search is complete; the proof applies only to the declared recipes and dated observations. " +
        "Cutting costs, setup time and undeclared supplier bundles are outside this objective.";
      parent.append(scope);
    }
    const deadlineSearches = study.alternatives.filter(row => {
      const result = available(row.sewing_selection);
      return result?.request?.completion_deadline_milliseconds != null ||
        result?.trials?.some(trial => trial.completion_deadline);
    });
    if (deadlineSearches.length) {
      const deadlines = table(parent, "Deadline-constrained sewing recipe search", ["Alternative", "Recipe and route",
        "Inclusive deadline (s)", "Complete schedule (s)", "Completion lower bound (s)", "Deadline admission",
        "Minimum completion proved", "Reason"]);
      const seconds = (milliseconds, fallback) => {
        if (!Number.isSafeInteger(milliseconds) || milliseconds < 0) return metric(fallback);
        // Preserve the exact integer clock even where adjacent milliseconds have identical f64 seconds.
        const digits = String(milliseconds).padStart(4, "0");
        const fraction = digits.slice(-3).replace(/0+$/, "");
        return digits.slice(0, -3) + (fraction ? `.${fraction}` : "");
      };
      for (const row of deadlineSearches) {
        const result = available(row.sewing_selection);
        for (const trial of result.trials || []) {
          const deadline = trial.completion_deadline;
          const recipe = Object.entries(trial.choices || {}).map(([operation, choice]) => `${operation}: ${choice}`).join("; ");
          const admission = trial.refusal ? "Plan refused" :
            ({ feasible: "Meets deadline", excluded: "Misses deadline (proved)", unresolved: "Unresolved" }[deadline?.status] || "Unknown");
          const reasons = [deadline?.reason, trial.refusal?.message, trial.objective_unavailable?.message]
            .filter((message, index, all) => message && all.indexOf(message) === index).join("; ");
          cells(deadlines, [row.key,
            [trial.route_choice, recipe || "Original recipes"].filter(Boolean).join("; "),
            seconds(deadline?.limit_milliseconds ?? result.request?.completion_deadline_milliseconds),
            seconds(deadline?.selected_makespan_milliseconds, deadline?.selected_completion_seconds),
            seconds(deadline?.lower_bound_makespan_milliseconds, deadline?.lower_bound_completion_seconds),
            admission, deadline?.timing_optimality_proven == null ? "Unknown" : String(deadline.timing_optimality_proven),
            reasons || "Inspect retained completion timing"]);
        }
      }
      const scope = document.createElement("p");
      scope.textContent = "Only complete schedules meeting the inclusive deadline compete for recipe selection. " +
        "A feasible schedule can qualify before its minimum time is proved. A late incumbent remains unresolved unless a valid lower bound " +
        "or proved minimum rules out the deadline. Feed and setup alone do not establish complete production time.";
      parent.append(scope);
    }
  }
  if (study.alternatives.some(row => row.metrics?.sewing_setup_seconds != null ||
      row.metrics?.sewing_feed_and_setup_seconds != null) ||
      [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
        .some(item => ["sewing_setup_seconds", "sewing_feed_and_setup_seconds"].includes(item.metric))) {
    const timing = table(parent, "Source-qualified sewing durations", ["Alternative", "Sewing",
      "Feed-only seconds", "Setup seconds", "Feed + setup seconds"]);
    for (const row of study.alternatives) {
      const m = row.metrics || {};
      cells(timing, [row.key, state(row.sewing), metric(m.feed_seconds),
        metric(m.sewing_setup_seconds), metric(m.sewing_feed_and_setup_seconds)]);
      const sewing = available(row.sewing);
      const source = sewing?.production?.parallel || sewing?.production?.routing ? null :
        (sewing?.production?.ordering || sewing?.ordering)?.timing;
      if (source) detail(parent, `${row.key}: setup timing evidence and selected schedule (display only)`, source);
    }
    const note = document.createElement("p");
    note.textContent = "Setup and feed + setup durations come from the selected qualified sewing plan and its declared timing evidence. " +
      "Missing timing stays unknown. These durations exclude undeclared handling, workholding and other machine-cycle work. " +
      "A feasible bounded-search result does not prove an optimal schedule.";
    parent.append(note);
  }
  sewingConsumablesComparison(parent, study.alternatives, computed);
  cuttingJobComparison(op, parent, study.alternatives);
  cutAndSewPurchasingComparison(parent, study.alternatives, computed);
  assemblyMassComparison(parent, study.alternatives);
  vaporComparison(op, parent, study.alternatives);
  if (study.screening) screening(parent, study.screening);
}
function sewingConsumablesComparison(parent, alternatives, computed) {
  const metrics = ["thread_required_length_m", "thread_required_mass_kg", "thread_observed_order_cost"];
  const requested = [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
    .some(item => metrics.includes(item.metric)) ||
    computed?.policy?.required_consumers?.some(name => ["sewing_consumables", "sewing_purchasing"].includes(name));
  if (!requested && !alternatives.some(row => available(row.sewing)?.consumables ||
      metrics.some(name => row.metrics?.[name] != null))) return;
  const totals = table(parent, "Thread for the selected sewing plan — complete or unknown",
    ["Alternative", "Production copies", "Thread quantities", "Required length (m)", "Thread purchasing", "Thread currency", "Complete order cost"]);
  for (const row of alternatives) {
    const plan = available(row.sewing), report = plan?.consumables, purchase = report?.purchasing;
    const complete = report?.complete_consumption_coverage === true;
    const quantity = report ? (complete ? "Complete channel coverage" : "Incomplete channel coverage") :
      plan ? "Not requested" : state(row.sewing);
    const pricing = purchase ? (purchase.complete_offer_coverage === true ? "Complete offer coverage" :
      "Incomplete offer coverage") : report || plan ? "Not requested" : state(row.sewing);
    const cost = metric(complete && purchase?.complete_offer_coverage === true ?
      (purchase.selected_observed_order_total ?? purchase.least_observed_independent_order_total) : null);
    const copies = report?.schema === "ftx-sewing-consumables/1" ? 1 : report?.production_copies;
    cells(totals, [row.key, metric(copies), quantity, metric(complete ? report.total_required_length_m : null), pricing,
      purchase?.currency || "Unknown", cost === "Unknown" ? cost : `${cost} ${purchase?.currency || "(currency unavailable)"}`]);
    if (report?.complete_mass_coverage != null || row.metrics?.thread_required_mass_kg != null ||
        [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
          .some(item => item.metric === "thread_required_mass_kg")) {
      const mass = table(parent, `${row.key}: consumed thread mass`,
        ["Mass coverage", "Complete consumed mass (kg)"]);
      cells(mass, [report?.complete_mass_coverage === true ? "Complete qualified yarn density" : "Incomplete or unavailable density",
        metric(complete && report?.complete_mass_coverage === true ? report.total_required_mass_kg : null)]);
      const yarns = table(parent, `${row.key}: exact yarn length and mass requirements`,
        ["Exact yarn snapshot", "Required length (m)", "Qualified linear density (kg/m)", "Consumed mass (kg)", "Missing mass evidence"]);
      for (const yarn of report?.material_requirements || []) cells(yarns, [yarn.material_sha256,
        metric(yarn.required_length_m), metric(yarn.mass?.linear_density_kg_m),
        metric(yarn.mass?.required_mass_kg), yarn.mass_unavailable_reason || (yarn.mass ? "None" : "Not supplied")]);
    }
    if (report?.operation_channels?.length) {
      const physicalCells = report.operation_channels.some(channel => typeof channel.cell === "string");
      const channels = table(parent, `${row.key}: selected operation and thread channel requirements`,
        [...(physicalCells ? ["Cell"] : []), "Operation", "Qualification", "Channel", "Exact yarn snapshot", "Traversed intervals", "Required length (m)"]);
      for (const channel of report.operation_channels) cells(channels, [...(physicalCells ? [channel.cell || "Unknown"] : []),
        channel.operation_id, channel.qualification_id,
        channel.channel_id, channel.material_sha256, metric(channel.stitch_intervals), metric(channel.required_length_m)]);
      if (report.operation_channels.some(channel => channel.per_copy)) {
        const perCopy = table(parent, `${row.key}: thread consumption per garment before production aggregation`,
          [...(physicalCells ? ["Cell"] : []), "Operation", "Channel", "Traversed intervals", "Traversal length (m)", "Start allowance (m)", "End allowance (m)", "Required length (m)"]);
        for (const channel of report.operation_channels) {
          const one = channel.per_copy;
          cells(perCopy, [...(physicalCells ? [channel.cell || "Unknown"] : []), channel.operation_id, channel.channel_id, metric(one?.stitch_intervals),
            metric(one?.traversal_length_m), metric(one?.start_allowance_m), metric(one?.end_allowance_m), metric(one?.required_length_m)]);
        }
      }
    }
    if (purchase?.order_allocation_optimality_proven != null) {
      const allocation = document.createElement("p");
      allocation.textContent = `${row.key}: ` + (purchase.order_allocation_optimality_proven ?
        "The least complete thread allocation among the listed offers is proved. " :
        purchase.complete_offer_coverage ? "A complete thread allocation is available; its minimum cost is unproved. " :
        "No complete thread allocation is available. ") +
        "Each operation/channel on each garment stays with one supplier; no mid-operation thread splice is inferred.";
      parent.append(allocation);
      const orders = table(parent, `${row.key}: complete thread orders across suppliers`,
        ["Exact yarn", "Offer", "Assigned operation / channel / garment", "Required length (m)",
          "Supplier order quantity", "Sale unit", "Merchandise cost", "Allocation search"]);
      for (const material of purchase.material_orders || []) {
        const result = material.allocation;
        for (const order of result?.selected_orders || []) {
          const assigned = (order.segment_indices || []).map(index => {
            const segment = purchase.segments?.[index], channel = report.operation_channels?.[segment?.operation_channel];
            return channel && segment ? `${channel.cell ? `${channel.cell} / ` : ""}${channel.operation_id} / ${channel.channel_id} / garment ${segment.copy + 1}` : "Unknown segment";
          }).join("; ");
          cells(orders, [material.requirement?.material_sha256 ?? "Unknown", order.offer?.quote?.offer_id ?? "Unknown",
            assigned, metric(order.required_length_m), metric(order.offer?.order_quantity), order.offer?.quote?.unit ?? "Unknown",
            `${metric(order.offer?.order_cost)} ${purchase.currency || "Unknown currency"}`,
            result.optimality_proven ? "Listed minimum proved" : result.budget_exhausted ? "Complete incumbent; budget ended" : "Unproved"]);
        }
      }
    }
    if (report?.missing_operations?.length || purchase?.unavailable_reason) {
      const missing = document.createElement("p");
      missing.textContent = `${row.key}: ` + (report.missing_operations || [])
        .map(item => `${item.cell ? `${item.cell} / ` : ""}${item.operation_id}: ${item.reason}`).join("; ") +
        (purchase?.unavailable_reason ? ` ${purchase.unavailable_reason}` : "");
      parent.append(missing);
    }
    detail(parent, `${row.key}: selected thread channels, exact yarns and order evidence (display only)`, {
      selected_recipe_choices: available(row.sewing_selection)?.selected_choices,
      selected_route: available(row.sewing_selection)?.selected_route,
      consumables: report || null, qualifications: plan?.options?.qualifications,
      cell_qualifications: (plan?.production?.routing?.cell_plans || plan?.production?.parallel?.cell_plans)?.map(source =>
        ({ cell: source.cell, qualifications: source.plan.options.qualifications })),
      request: plan?.options?.consumables, sewing_refusal: row.sewing?.refusal
    });
  }
  const note = document.createElement("p");
  note.textContent = "Thread demand follows every traversal in the selected sewing plan, with each declared start/end allowance applied once per operation and channel in each garment. " +
    "Partial channel rows are diagnostic; they are never a complete bill of materials or a priced subtotal. " +
    "Exact yarn requirements aggregate before supplier minimums, increments and observed stock are applied. " +
    "Study thread inventories use the same production copies as cutting; standalone sewing uses its explicitly declared production copies or one garment. " +
    "One continuous operation/channel requirement is retained without multiplying its splice-free length by the production count. " +
    "Qualified yarn linear density can supply consumed mass and convert kilogram offers while keeping supplier rounding in the original sale units. " +
    "Missing density leaves length-only results usable; mass never establishes an uninterrupted thread length. " +
    "Consumed thread includes the declared allowances and is not retained assembly mass. Explicit production schedules report their complete batch timing. " +
    "Thread prices retain their own currency and market; an explicitly requested cut-and-sew total separately checks complete fabric orders and a common source market. " +
    "Time-based recipe selection does not prove the least thread cost.";
  parent.append(note);
}
function cutAndSewPurchasingComparison(parent, alternatives, computed) {
  const requested = [...(computed?.policy?.objectives || []), ...(computed?.policy?.constraints || [])]
    .some(item => item.metric === "cut_and_sew_selected_order_cost") ||
    computed?.policy?.required_consumers?.includes("cut_and_sew_purchasing");
  if (!requested && !alternatives.some(row => row.cut_and_sew_purchasing &&
      row.cut_and_sew_purchasing.status !== "not_requested")) return;
  const t = table(parent, "Complete fabric + thread merchandise orders",
    ["Alternative", "Combined purchasing", "Garment copies", "Fabric cost", "Thread cost", "Complete selected cost",
      "Currency", "Order allocation proof", "Observed quantity coverage"]);
  for (const row of alternatives) {
    const r = available(row.cut_and_sew_purchasing);
    cells(t, [row.key, state(row.cut_and_sew_purchasing), metric(r?.production_copies),
      metric(r?.fabric_selected_order_cost), metric(r?.thread_selected_order_cost), metric(r?.selected_order_cost),
      r?.market?.currency || "Unknown", r ? (r.order_allocation_optimality_proven ? "Proved for fixed demands" :
        "Complete feasible orders; optimum unproved") : "Unknown",
      r ? (r.complete_observed_quantity_coverage ? "Complete observed capacity" : "Not fully observed") : "Unknown"]);
    detail(parent, `${row.key}: complete fabric and thread orders, common market and source history (display only)`,
      r || { refusal: row.cut_and_sew_purchasing?.refusal });
  }
  const note = document.createElement("p");
  note.textContent = "The total uses complete actual fabric orders and the selected sewing plan's complete thread orders for the same garment count. " +
    "Both sides share one comparable market and immutable quote history; current supplier stock aliases cannot supply both independently. " +
    "Supplier minimums, increments, tiers and declared capacity rules are already included by Rust. " +
    "A complete feasible total can remain available without a proved allocation minimum. Its proof applies to the fixed demands, not all designs, recipes or cell assignments. " +
    "Labor, equipment, unrepresented trims and costs outside the declared quote terms are outside this merchandise total. " +
    "A refused combined total leaves the independent fabric and thread results available for review.";
  parent.append(note);
}
function knifeProgramResult(parent, program, label = "Cutting plan", refusal) {
  if (!program) {
    if (refusal) {
      const note = document.createElement("p");
      note.textContent = `${label}: knife process unavailable — ${refusal.message}. Valid marker geometry and independent purchasing remain available.`;
      parent.append(note);
      detail(parent, `${label}: knife process refusal (display only)`, refusal);
    }
    return;
  }
  const t = table(parent, `${label}: material-qualified knife paths`,
    ["Marker", "Process observation", "Cut paths", "Cut length (mm)", "Raised travel (mm)",
      "Cutting (s)", "Travel (s)", "Lift / lower (s)", "Modeled path time (s)", "Travel ordering"]);
  for (const marker of program.markers || []) {
    const route = marker.selected;
    cells(t, [String(marker.marker + 1), marker.qualification, metric(route?.runs?.length),
      metric(route?.cut_length_mm), metric(route?.travel_length_mm), metric(route?.cutting_seconds),
      metric(route?.travel_seconds), metric(route?.lift_lower_seconds), metric(route?.modeled_path_seconds),
      marker.improved ? "Shorter complete traversal" : "Complete authored traversal"]);
  }
  const finite = (program.markers || []).filter(marker => marker.finite_stock);
  if (finite.length) {
    const frames = table(parent, `${label}: physical stock and machine placement`,
      ["Inventory marker", "Physical stock", "Frozen stock SHA-256", "Planning-frame translation to machine (mm)"]);
    for (const marker of finite) {
      const stock = marker.finite_stock;
      cells(frames, [String(marker.marker + 1), stock.stock_id, stock.stock_sha256,
        (stock.marker_to_machine_offset_mm || []).map(metric).join(", ")]);
    }
    const placement = document.createElement("p");
    placement.textContent = "Each used hide or remnant keeps its original inventory identity, outline, defects and occupied regions. " +
      "The whole stock bounding box is translated onto the declared bed without rotation; path coordinates use that machine frame. " +
      "The exact source-to-machine transform is retained below. Unused inventory is not a cutting program.";
    parent.append(placement);
  }
  const note = document.createElement("p");
  note.textContent = "Internal cuts finish before exterior pieces are released. Every path retains its authored geometry and direction; " +
    "the process observation is locked to the exact textile and full machine configuration. " +
    "Times cover declared constant-rate cutting, raised travel and lift/lower phases. Loading, acceleration, blade turning and inspection are outside this model. " +
    (program.budget_exhausted ? "The ordering budget ended; complete authored traversals remain available. " : "") +
    "This reviewable plan does not establish physical cut quality, an optimal route or permission to run a machine.";
  parent.append(note);
  detail(parent, `${label}: exact knife paths, material locks and process limits (display only)`, program);
}
function cuttingJobComparison(op, parent, alternatives) {
  if (!alternatives.some(row => row.cutting_job && row.cutting_job.status !== "not_requested")) return;
  const note = document.createElement("p");
  note.textContent = "A complete cutting job allocates each physical instance and copy once, using its declared roll or finite-stock route. " +
    "Each alternative starts from the same original inventory. Purchasing is separate: an allocated job can retain incomplete or refused offers. " +
    "Download both frozen job files to replay with cut-job; the result does not authorize machine execution.";
  parent.append(note);
  const t = table(parent, "Complete cutting jobs — rolls, hides and remnants", ["Alternative", "Cutting job", "Complete inventory",
    "Allocated instances / copies", "Roll markers", "Used finite items", "Engaged stock area (m²)",
    "External cut area (m²)", "Complete engaged-stock mass (kg)", "Purchasing", "Proven least complete cost"]);
  for (const [i, row] of alternatives.entries()) {
    const r = available(row.cutting_job), purchase = r?.purchasing;
    const cost = metric(purchase?.least_observed_independent_order_total);
    cells(t, [row.key, state(row.cutting_job), r ? (r.complete_inventory ? "Yes" : "Incomplete") : "Unknown",
      metric(r?.inventory?.length), r ? (r.roll ? metric(r.roll.rolls?.length) : "0") : "Unknown",
      r ? (r.finite_stock ? metric(r.finite_stock.used_stock_count) : "0") : "Unknown",
      metric(r?.stock_area_m2), metric(r?.external_cut_area_m2), metric(r?.stock_mass_kg),
      purchase ? (purchase.complete_offer_coverage ? "Complete offer coverage" : "Incomplete offer coverage") : r ? "Not requested" : "Unknown",
      cost === "Unknown" ? cost : `${cost} ${purchase.currency}`]);
    detail(parent, `${row.key}: routed physical inventory, source materials and purchasing outcomes (display only)`,
      r || { refusal: row.cutting_job?.refusal });
    if (!r) continue;
    knifeProgramResult(parent, r.roll?.program, `${row.key}: roll route`, r.roll?.program_refusal);
    knifeProgramResult(parent, r.finite_stock?.program, `${row.key}: finite-stock route`, r.finite_stock?.program_refusal);
    if (r.resolved_routes?.length) {
      const routes = table(parent, `${row.key}: cutting routes for the selected materials`,
        ["Source piece", "Stock route", "Material snapshot"]);
      for (const route of r.resolved_routes) cells(routes, [route.piece,
        route.stock === "roll" ? "Roll" : route.stock === "finite_stock" ? "Finite hide, sheet or remnant" : "Unknown",
        route.material_sha256]);
    }
    const rollPurchase = available(purchase?.roll);
    if (rollPurchase?.schema === "ftx-cut-procurement/3") {
      const selected = table(parent, `${row.key}: complete mixed-stock purchasing`,
        ["Complete offer coverage", "Selected complete cost", "Proven least complete cost", "Allocation optimum proved"]);
      const showCost = value => value == null ? "Unknown" : `${metric(value)} ${purchase.currency}`;
      cells(selected, [purchase.complete_offer_coverage ? "Yes" : "No",
        showCost(purchase.selected_observed_order_total), showCost(purchase.least_observed_independent_order_total),
        String(purchase.order_allocation_optimality_proven)]);
      splitProcurementResult(parent, rollPurchase, `${row.key}: roll-route supplier allocation`);
    }
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    if (typeof r.pattern_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-pattern.json`, text: r.pattern_json, mime: "application/json" });
    if (typeof r.replay_request_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-request.json`, text: r.replay_request_json, mime: "application/json" });
  }
}
function vaporCoverageComparison(op, parent, alternatives, simulations, requested, filePrefix = "") {
  if (!requested && ![...simulations.values()].some(value => available(value)?.vapor_coverage)) return;
  const note = document.createElement("p");
  note.textContent = "Vapor paths use this alternative's draped geometry, declared physical layers and source-qualified vapor resistance. " +
    "Only covered regions contribute to the reported potential transfer. Uncovered body fractions and unassigned cloth remain explicit; " +
    "the covered subtotal is not whole-body transfer, actual sweat evaporation or cooling supplied to the dry heat simulation.";
  parent.append(note);
  const t = table(parent, "Draped layered vapor coverage — covered-region potentials", ["Alternative", "Vapor coverage", "Body area (m²)",
    "Covered area (m²)", "Uncovered area (m²)", "Unassigned cloth reference area (m²)",
    "Covered potential latent power (W)", "Covered potential water flow (kg/s)"]);
  for (const [i, row] of alternatives.entries()) {
    const evaluation = simulations.get(row.key), initial = available(evaluation), r = available(initial?.vapor_coverage), c = r?.coverage;
    cells(t, [row.key, initial ? state(initial.vapor_coverage) : state(evaluation), metric(c?.body_area_m2),
      metric(c?.covered_area_m2), metric(c?.uncovered_area_m2), metric(c?.unassigned_cloth_reference_area_m2),
      vaporMetric(c, "covered_potential_heat_flow_w"), vaporMetric(c, "covered_potential_mass_flow_kg_s")]);
    vaporBoundsNote(parent, c, `${row.key}: covered vapor source bounds`);
    detail(parent, `${row.key}: vapor coverage regions, source observations and boundary assumptions (display only)`,
      r || { refusal: initial?.vapor_coverage?.refusal || evaluation?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `${filePrefix}alternative-${i + 1}-vapor-coverage-replay.json`, text: r.replay_request_json, mime: "application/json" });
    }
  }
}
function vaporComparison(op, parent, alternatives) {
  if (!alternatives.some(row => row.vapor && row.vapor.status !== "not_requested")) return;
  const note = document.createElement("p");
  note.textContent = "Each alternative resolves the declared vapor-stack layers from its final pattern assignments. " +
    "The common area and boundary pressures, films and latent heat are supplied assumptions. " +
    "These are potential transfer rates, not actual sweat evaporation, garment comfort or heat supplied to the separate thermal simulation. " +
    "Missing vapor observations do not erase independent manufacturing or simulation results.";
  parent.append(note);
  const t = table(parent, "Material-following potential vapor transfer", ["Alternative", "Vapor evaluation", "Declared area (m²)",
    "Total Ret (m²·Pa/W)", "Potential latent heat flow (W)", "Potential water mass flow (kg/s)", "Layer protocols"]);
  for (const [i, row] of alternatives.entries()) {
    const r = available(row.vapor);
    cells(t, [row.key, state(row.vapor), metric(r?.request.area_m2), vaporMetric(r, "total_resistance_m2_pa_w"),
      vaporMetric(r, "potential_heat_flow_w"), vaporMetric(r, "potential_mass_flow_kg_s"),
      r ? (r.mixed_protocols ? "Mixed; explicitly admitted" : "Reported protocols match") : "Unknown"]);
    vaporBoundsNote(parent, r, `${row.key}: vapor source bounds`);
    detail(parent, `${row.key}: vapor layers, source observations and boundary assumptions (display only)`,
      r || { refusal: row.vapor?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-vapor-replay.json`, text: r.replay_request_json, mime: "application/json" });
    }
  }
}
function inspectResult(r, selectionJson) {
  clear(["thermalPoint"]);
  inspected = selectionJson || r.selection_json || text(r.selection);
  $("detail").textContent = text(r.snapshot);
  $("capabilities").replaceChildren();
  const table = document.createElement("table");
  for (const [name, state] of Object.entries(r.capabilities)) {
    const row = document.createElement("tr"), a = document.createElement("th"), b = document.createElement("td");
    a.textContent = name.replaceAll("_", " ");
    b.textContent = state.ready ? "Coefficient inputs available" : state.refusal.message;
    row.append(a, b); table.append(row);
  }
  $("capabilities").append(table);
}
function thermalPointResult(parent, r) {
  const properties = table(parent, "Source-qualified thermal properties", ["Quantity", "Value", "Unit"]);
  cells(properties, ["Material temperature", metric(r.temperature_c), "°C"]);
  cells(properties, ["Effective sheet resistance", metric(r.resistance_m2_k_w), "m² K/W"]);
  cells(properties, ["Areal heat capacity", metric(r.areal_heat_capacity_j_m2_k), "J/(m² K)"]);
  cells(properties, ["Common source temperature interval", Array.isArray(r.temperature_domain_c) ?
    r.temperature_domain_c.map(metric).join(" to ") : "Not reported for this model", "°C"]);
  const note = document.createElement("p");
  note.textContent = "These properties describe the selected dry material at the stated temperature. " +
    "A source-qualified property value does not establish garment warmth or comfort. Keep the frozen selection with the reported temperature to replay this point.";
  parent.append(note);
  detail(parent, "Material observations, thermal laws and source conditions", r.material);
  return { schema: r.schema, material_id: r.selection?.id, material_sha256: r.material?.sha256,
    allow_estimates: r.selection?.allow_estimates, model: r.model,
    uncertainty_propagated: r.uncertainty_propagated, comfort_claim: r.comfort_claim,
    empirical_validation_claim: r.empirical_validation_claim };
}
function thermalHistoryResult(parent, r) {
  const energy = table(parent, "Signed heat and stored energy", ["Quantity", "Value", "Unit"]);
  for (const [label, key, unit] of [
    ["Incoming energy at the inside boundary", "incoming_energy_j", "J"],
    ["Outgoing energy at the outside boundary", "outgoing_energy_j", "J"],
    ["Stored energy change", "stored_energy_change_j", "J"],
    ["Largest absolute step energy-balance error", "largest_step_energy_balance_error_j", "J"],
    ["Steady heat through the same network", "steady_heat_w", "W"]
  ]) cells(energy, [label, metric(r[key]), unit]);
  if (r.exterior_exchange) {
    const exterior = table(parent, "Qualified exterior convection and radiation", ["Quantity", "Value", "Unit"]);
    for (const [label, key, unit] of [["Convective energy", "convective_energy_j", "J"],
      ["Radiative energy", "radiative_energy_j", "J"], ["Steady convective heat", "steady_convective_heat_w", "W"],
      ["Steady radiative heat", "steady_radiative_heat_w", "W"]]) cells(exterior, [label, metric(r.exterior_exchange[key]), unit]);
    detail(parent, "Exterior observations and boundary conditions", r.exterior_exchange);
  }
  const note = document.createElement("p");
  note.textContent = "Boundary heat keeps its native sign; cooling can produce negative stored energy and reversed heat flow. " +
    "The initial sample has no reported boundary power or accumulated energy. A small energy residual checks numerical balance, not measured material accuracy. " +
    (r.accuracy_note || "");
  parent.append(note);
  const trace = r.trace || [], count = r.steady_temperature_c?.length || 0;
  if (trace.length && count) {
    const label = document.createElement("label"), cell = document.createElement("input"), state = document.createElement("p");
    label.textContent = `Inspect thermal cell (1–${count}, ordered from inside to outside)`;
    cell.type = "number"; cell.min = "1"; cell.max = String(count); cell.step = "1"; cell.value = "1";
    label.append(cell); parent.append(label, state);
    const samples = table(parent, "Retained heating / cooling samples", ["Time (s)", "Selected cell temperature (°C)",
      "Inside boundary heat (W)", "Outside boundary heat (W)", "Stored energy change (J)"]);
    const body = document.createElement("tbody"); samples.append(body);
    const navigation = document.createElement("div"), position = document.createElement("p");
    const buttons = ["First samples", "Previous samples", "Next samples", "Final samples"].map(title => {
      const button = document.createElement("button"); button.type = "button"; button.textContent = title;
      navigation.append(button); return button;
    });
    parent.append(navigation, position);
    const pageSize = 50, lastPage = Math.floor((trace.length - 1) / pageSize); let page = 0;
    const render = () => {
      body.replaceChildren();
      const selected = Number(cell.value) - 1;
      if (!cell.value.trim() || !Number.isInteger(selected) || selected < 0 || selected >= count) {
        state.textContent = `Choose a whole cell number from 1 to ${count}.`; position.textContent = ""; return;
      }
      const layer = Math.floor(selected / r.model.cells_per_layer), material = r.model.layers[layer];
      state.textContent = `Cell ${selected + 1}; layer ${layer + 1}: ${material?.authored?.id || "Unknown"}. ` +
        `Initial ${metric(trace[0].temperature_c?.[selected])} °C; final ${metric(trace.at(-1).temperature_c?.[selected])} °C; ` +
        `steady ${metric(r.steady_temperature_c[selected])} °C.`;
      const start = page * pageSize, end = Math.min(trace.length, start + pageSize);
      for (let i = start; i < end; i++) {
        const sample = trace[i];
        cells(body, [metric(sample.time_seconds), metric(sample.temperature_c?.[selected]), metric(sample.inside_heat_w),
          metric(sample.outside_heat_w), metric(sample.stored_energy_change_j)]);
      }
      position.textContent = `Samples ${start + 1}–${end} of ${trace.length}. Every retained sample remains in the exact report download.`;
      buttons[0].disabled = buttons[1].disabled = page === 0;
      buttons[2].disabled = buttons[3].disabled = page === lastPage;
    };
    cell.addEventListener("input", render);
    buttons[0].onclick = () => { page = 0; render(); };
    buttons[1].onclick = () => { page = Math.max(0, page - 1); render(); };
    buttons[2].onclick = () => { page = Math.min(lastPage, page + 1); render(); };
    buttons[3].onclick = () => { page = lastPage; render(); };
    render();
  }
  detail(parent, "Frozen layers, thermal laws and source conditions", r.model);
  return { schema: r.schema, area_m2: r.area_m2, inside_c: r.inside_c, outside_c: r.outside_c,
    inside_resistance_m2_k_w: r.inside_resistance_m2_k_w, outside_resistance_m2_k_w: r.outside_resistance_m2_k_w,
    cells_per_layer: r.model?.cells_per_layer, temperature_domain_c: r.model?.temperature_domain_c,
    step_seconds: r.step_seconds, steps: r.steps, record_every: r.record_every,
    final_time_seconds: trace.at(-1)?.time_seconds, retained_samples: trace.length,
    comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim,
    scope: "Dry one-dimensional layers with prescribed boundaries; no garment comfort, moisture or transient seam storage." };
}
function compressionHistoryResult(parent, r) {
  const controlled = r.schema === "ftx-compressed-thermal-history-report/3";
  const controls = controlled ? r.input.legs.flatMap(leg => leg.holds.map(hold => hold.control)) : [];
  const t = table(parent, "Compression and heat history — chronological holds", ["Time (s)",
    ...(controlled ? ["Prescribed boundary"] : []), "Pressure (Pa)",
    "Gap (m)", "Cell temperatures (°C)", "Cumulative external work (J)", "Cumulative entering heat (J)",
    "Cumulative leaving heat (J)", "Stored energy change (J)", "Heat balance error (J)"]);
  for (const [index, step] of r.steps.entries()) {
    const control = controls[index];
    const boundary = control?.kind === "pressure" ? `Pressure: ${metric(control.pressure_pa)} Pa` :
      control?.kind === "gap" ? `Gap: ${metric(control.gap_m)} m` : "Unavailable";
    cells(t, [metric(step.elapsed_s), ...(controlled ? [boundary] : []), metric(step.pressure_pa), metric(step.thickness_m),
      step.temperature_c.map(metric).join(", "), metric(step.cumulative_external_work_j),
      metric(step.cumulative_inside_heat_j), metric(step.cumulative_outside_heat_j),
      metric(step.cumulative_stored_change_j), metric(step.cumulative_heat_balance_error_j)]);
  }
  const note = document.createElement("p");
  note.textContent = "External mechanical work is separate from sensible heat; no work-to-heat conversion is inferred. " +
    (controlled ? "Each hold prescribes either pressure or gap; the other value is computed from the same qualified material branches. " : "") +
    "Temperatures follow the same material cells across qualified branch changes. A final material selection does not save this temperature state; keep the full history report.";
  parent.append(note);
  if (r.transitions) detail(parent, "Source-qualified branch transitions", r.transitions);
  if (r.input?.adaptive) {
    const integration = table(parent, "Adaptive thermal integration — local estimates and charged work",
      ["Time (s)", "Accepted half steps", "Rejected trials", "Charged cell solves", "Cumulative cell solves", "Largest local error ratio"]);
    for (const step of r.steps) {
      const d = step.integration;
      cells(integration, [metric(step.elapsed_s), metric(d.accepted_substeps), metric(d.rejected_trials),
        metric(d.cell_solves), metric(d.cumulative_cell_solves), metric(d.max_accepted_error_ratio)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Boundary heat sums actual accepted half steps. Local temperature and signed heat error estimates are not global accuracy or confidence bounds. " +
      "Charged work includes rejected trials and continues across holds and branch changes. Spatial error and measured material qualification remain separate.";
    parent.append(scope);
    detail(parent, "Declared adaptive thermal policy", r.input.adaptive);
  }
  return { selected_layer: r.chosen_layer_id, holds: r.steps.length, branch_transitions: r.transitions?.length || 0,
    input_sha256: r.input_sha256, reference_mass_kg: r.final_compression.reference_mass_kg,
    initial_preparation_work_j: r.initial_compression.external_work_j, no_claim: r.no_claim,
    next: "Review the final selected layer's capabilities, then assign it above. Download all final layers and the complete history below." };
}
function vaporResult(parent, r) {
  if (r.schema === "ftx-vapor-coverage-history-report/1") {
    vaporHistoryResult(parent, "Replay", r, "Available");
    return { schema: r.schema, request_sha256: r.request_sha256, path_sha256: r.path_sha256,
      integration: r.integration, latent_heat_coupled: r.latent_heat_coupled,
      ...(r.wet_film ? { body_wet_film: "Available", textile_temperature_coupled: r.wet_film.textile_temperature_coupled } : {}),
      physical_validation_claim: r.physical_validation_claim,
      next: "Download the exact report, original input and frozen replay request. Replay it in this panel without a body model." };
  }
  if (r.schema === "ftx-vapor-coverage-report/1") {
    const totals = table(parent, "Layered vapor coverage", ["Quantity", "Value", "Unit"]);
    for (const [label, name, unit] of [
      ["Covered body area", "covered_area_m2", "m²"],
      ["Uncovered body area — transfer unknown", "uncovered_area_m2", "m²"],
      ["Covered potential heat flow", "covered_potential_heat_flow_w", "W"],
      ["Covered potential water flow", "covered_potential_mass_flow_kg_s", "kg/s"]
    ]) cells(totals, [label, vaporMetric(r.coverage, name), unit]);
    vaporBoundsNote(parent, r.coverage);
    detail(parent, "Ordered layers, pressure profiles and source conditions", r.coverage.regions);
    detail(parent, "Projection and boundary assumptions", { projection: r.projection, request: r.request });
    return { schema: r.schema, request_sha256: r.request_sha256, physical_validation_claim: r.physical_validation_claim,
      scope: "Potential transfer through covered regions only; uncovered transfer and moisture availability remain unknown." };
  }
  const note = document.createElement("p");
  note.textContent = "Potential steady transfer through the declared series of vapor resistances. " +
    "Boundary vapor pressures, film resistances, area and latent heat are supplied assumptions. " +
    "This does not establish available sweat, actual evaporative cooling, condensation, garment comfort or a coupled dry/wet heat history. " +
    (r.mixed_protocols ? "Different reported source protocols were explicitly admitted. " : "Reported layer protocols match. ") +
    "Source measurement conditions remain attached to each layer; they are not an operating range for this model.";
  parent.append(note);
  const totals = table(parent, "Potential vapor and latent heat exchange", ["Quantity", "Value", "Unit"]);
  for (const [label, name, unit] of [
    ["Total vapor resistance", "total_resistance_m2_pa_w", "m²·Pa/W"],
    ["Boundary vapor pressure difference", "pressure_difference_pa", "Pa"],
    ["Potential latent heat flux", "potential_heat_flux_w_m2", "W/m²"],
    ["Potential latent heat flow", "potential_heat_flow_w", "W"],
    ["Potential water mass flux", "potential_mass_flux_kg_m2_s", "kg/(m²·s)"],
    ["Potential water mass flow", "potential_mass_flow_kg_s", "kg/s"],
    ["Inside film pressure drop", "inside_film_pressure_drop_pa", "Pa"],
    ["Outside film pressure drop", "outside_film_pressure_drop_pa", "Pa"]
  ]) cells(totals, [label, vaporMetric(r, name), unit]);
  vaporBoundsNote(parent, r);
  const layers = table(parent, "Layer resistances and vapor pressure drops — inside to outside",
    ["Layer", "Material", "Ret (m²·Pa/W)", "Inside pressure (Pa)", "Outside pressure (Pa)", "Drop (Pa)", "Evidence"]);
  for (const row of r.layers) {
    cells(layers, [row.key, row.selection.id, metric(row.observation.value) +
      (row.observation.bounds ? `; source bounds [${row.observation.bounds.map(metric).join(", ")}]` : ""), metric(row.inside_pressure_pa),
      metric(row.outside_pressure_pa), metric(row.pressure_drop_pa), row.observation.evidence]);
    detail(parent, `${row.key}: measurement method, conditions and exact source`, row);
  }
  detail(parent, "Declared boundary conditions and effective model", { request: r.request, effective_model: r.effective_model });
  return { schema: r.schema, request_sha256: r.request_sha256, mixed_protocols: r.mixed_protocols,
    physical_validation_claim: r.physical_validation_claim, notes: r.notes,
    next: "Download the exact report, original input and frozen replay request to repeat this calculation." };
}
function vaporSurfaceResult(parent, r) {
  const note = document.createElement("p");
  note.textContent = "Independent sheet potentials use each piece's current draped area and retained vapor resistance. " +
    "Overlapping sheets are evaluated independently. These totals do not establish body coverage, available sweat, " +
    "actual wearer evaporation or latent cooling of the dry-heat model.";
  parent.append(note);
  const totals = table(parent, "Potential transfer on current cloth geometry", ["Quantity", "Value", "Unit"]);
  for (const [label, name, unit] of [
    ["Current cloth area", "area_m2", "m²"],
    ["Potential latent heat flow", "potential_heat_flow_w", "W"],
    ["Potential water mass flow", "potential_mass_flow_kg_s", "kg/s"]
  ]) cells(totals, [label, vaporMetric(r, name), unit]);
  vaporBoundsNote(parent, r);
  const pieces = table(parent, "Independent piece potentials", ["Piece", "Area (m²)", "Potential heat (W)", "Potential water (kg/s)"]);
  for (const row of r.pieces) cells(pieces, [row.instance, metric(row.area_m2),
    vaporMetric(row, "potential_heat_flow_w"), vaporMetric(row, "potential_mass_flow_kg_s")]);
  detail(parent, "Original resistance observations, boundaries and source protocols", r.columns);
  return { schema: r.schema, model_sha256: r.model_sha256, request_sha256: r.request_sha256,
    scope: r.scope, physical_validation_claim: r.physical_validation_claim,
    next: "Download the complete report, frozen replay, original conditions and retained drape below." };
}
function treatmentResult(parent, r) {
  const transitions = r.schema === "ftx-pattern-treatment-report/1" ? Object.entries(r.transitions) : [[r.input.specimen, r]];
  const rows = table(parent, "Source-qualified dimensional change", ["Specimen / transition", "Cycles",
    "Warp / wale remaining length", "Weft / course remaining length", "Area ratio", "Unsupported post-treatment properties"]);
  for (const [name, transition] of transitions) {
    cells(rows, [name, metric(transition.input.cycles), metric(transition.stretch[0]),
      metric(transition.stretch[1]), metric(transition.area_scale),
      [...transition.properties_no_longer_available, ...transition.responses_no_longer_available].join(", ") || "No original field lost"]);
    detail(parent, `${name}: exact process, conditioning, observations and before/after materials`, transition);
  }
  if (r.seams_after) {
    const seams = table(parent, "Seam compatibility after dimensional change", ["Seam", "Before", "After", "Length mismatch (mm)"]);
    for (const [i, row] of r.seams_after.entries()) cells(seams,
      [row.seam, r.seams_before[i]?.status ?? "Unknown", row.status, metric(row.residual_mm)]);
  }
  return { schema: r.schema, mode: r.mode, source_pattern_sha256: r.source_pattern_sha256,
    pattern_sha256: r.pattern_sha256, input_sha256: r.input_sha256,
    scope: r.scope || "Measured free dimensional change between two explicit material states; no old property is inherited.",
    next: r.pattern_json ? "Review the seam checks and downloads, then use the derived pattern to recompute fit, drape and manufacturing." :
      "Download the exact material selections or use this transition in a piece-treatment request." };
}
function sewingWorkholding(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel || plan?.production?.routing) return;
  const report = plan?.production?.workholding;
  if (!report) return;
  const replaced = plan.production.ordering?.baseline_replaced === true;
  const resources = table(parent, `${label}: finite fixture inventory`,
    ["Resource", "Available units", replaced ? "Feasible baseline peak" : "Original order peak", "Selected order peak"]);
  for (const resource of report.resources) cells(resources, [resource.id, metric(resource.capacity),
    metric(report.baseline.peak_occupancy[resource.id]), metric(report.selected.peak_occupancy[resource.id])]);
  const occupancy = values => Object.entries(values).map(([id, units]) => `${id}: ${metric(units)}`).join("; ");
  const timeline = table(parent, `${label}: selected fixture occupancy`,
    ["Operation", "Occupied before", "Acquired before start", "Occupied during", "Released after completion", "Occupied after"]);
  for (const step of report.selected.operations) cells(timeline, [step.operation,
    occupancy(step.occupied_before), step.acquired.join(", ") || "None", occupancy(step.occupied_during),
    step.released.join(", ") || "None", occupancy(step.occupied_after)]);
  const note = document.createElement("p");
  note.textContent = "The complete selected order respects the declared finite fixture inventory. " +
    (replaced ? "The original copy order could not satisfy the declared fixtures or lifecycle prerequisites; a complete feasible replacement baseline was found within the same search budget. " : "") +
    "Each garment acquires its units before the named operation starts and releases them after the named operation completes. " +
    "The inventory and holds are tied to the exact machine configuration and admitted material/process qualifications. " +
    "Fixture geometry, seam access, handling duration and hardware execution remain unvalidated.";
  parent.append(note);
  detail(parent, `${label}: fixture source, qualification locks and both complete timelines`, {
    policy: plan.options?.production?.workholding, report });
}
function sewingHandling(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel || plan?.production?.routing) return;
  const report = plan?.production?.handling;
  if (!report) return;
  const timing = plan.production.ordering?.handling_timing;
  const totals = table(parent, `${label}: serial production durations`,
    ["Schedule", "Feed (s)", "Setup (s)", "Handling (s)", "Modeled production (s)"]);
  const baselineLabel = plan.production.ordering?.baseline_replaced ? "Feasible replacement baseline" : "Original dependency order";
  for (const [name, prefix] of [[baselineLabel, "baseline"], ["Selected order", "selected"]]) {
    const setup = plan.production.ordering?.timing?.[`${prefix}_setup_milliseconds`];
    const handling = timing?.[`${prefix}_handling_milliseconds`];
    cells(totals, [name, metric(plan.production.feed_seconds),
      metric(typeof setup === "number" ? setup / 1000 : null),
      metric(typeof handling === "number" ? handling / 1000 : null),
      metric(timing?.[`${prefix}_modeled_production_seconds`])]);
  }
  const transitions = table(parent, `${label}: selected garment handling`,
    ["Previous operation", "Next operation", "Handling (ms)"]);
  for (const step of report.selected?.steps || []) cells(transitions,
    [step.from ?? "Initial loading", step.to ?? "Final unloading", metric(step.milliseconds)]);
  const note = document.createElement("p");
  note.textContent = "The selected order includes independently qualified initial loading, operation transitions and final unloading. " +
    "Same-garment and different-garment transitions have explicit durations; retaining a needle/thread setup does not imply zero handling. " +
    "Feed, setup and handling are serial, nonoverlapping phases in this declared model. A joint optimum does not prove the setup-only minimum. " +
    "The result remains conditional on the supplied timings and constraints; it does not qualify undeclared work, concurrent machines or hardware execution.";
  parent.append(note);
  detail(parent, `${label}: handling evidence, exact qualifications and complete timelines`,
    { policy: plan.options?.production?.handling, report, timing });
}
function sewingSharedWorkholding(parent, plan, label) {
  const report = plan.production.parallel.shared_workholding;
  if (!report) return;
  const inventory = table(parent, `${label}: shared fixture pools`,
    ["Pool", "Available units", "Baseline peak", "Selected peak", "Held after completion"]);
  for (const resource of report.resources) cells(inventory, [resource.id, metric(resource.capacity),
    metric(report.baseline?.peak_occupancy?.[resource.id]), metric(report.selected?.peak_occupancy?.[resource.id]),
    metric(report.selected?.final_occupancy?.[resource.id])]);
  const bindings = table(parent, `${label}: cell fixtures using shared inventory`,
    ["Cell", "Local fixture resource", "Shared pool"]);
  for (const binding of report.bindings) cells(bindings, [binding.cell, binding.resource, binding.pool]);
  const holds = table(parent, `${label}: selected shared fixture reservations`,
    ["Pool", "Cell", "Global garment copy", "Local hold", "Units", "Acquire operation", "Release operation",
      "Held from (s)", "Available again (s)"]);
  for (const hold of report.selected?.holds || []) cells(holds, [hold.pool, hold.cell, metric(hold.copy),
    hold.hold, metric(hold.units), hold.acquire, hold.release,
    metric(typeof hold.acquired_at_milliseconds === "number" ? hold.acquired_at_milliseconds / 1000 : null),
    metric(typeof hold.released_at_milliseconds === "number" ? hold.released_at_milliseconds / 1000 : null)]);
  const note = document.createElement("p");
  note.textContent = "These cells share the declared finite fixture inventory. Units are reserved before handling starts, " +
    "remain held through intervening operations and waits, and become available at the release operation's feed completion. " +
    "A release can supply an acquisition at the same instant. The qualified protocol includes pool access and return in cell handling. " +
    "The computed allocation respects pool capacities; physical pool access, garment transfers and hardware execution still require their own qualification.";
  parent.append(note);
  detail(parent, `${label}: shared pool source, exact cell locks and both reservation histories`, {
    policy: plan.options?.production?.parallel?.shared_workholding, report });
}
function sewingRoutedWorkholding(parent, routing, label) {
  const report = routing.workholding;
  if (!report) return;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const local = table(parent, `${label}: routed cell fixture inventory`,
    ["Cell", "Resource", "Available units", "Baseline peak", "Selected peak"]);
  for (const cell of report.cells || []) for (const resource of cell.resources || []) cells(local, [
    cell.cell, resource.id, metric(resource.capacity),
    metric(report.baseline?.local_peak_occupancy?.[cell.cell]?.[resource.id]),
    metric(report.selected?.local_peak_occupancy?.[cell.cell]?.[resource.id]),
  ]);
  if (report.shared_workholding) {
    const pools = table(parent, `${label}: routed shared fixture pools`,
      ["Pool", "Available units", "Baseline peak", "Selected peak"]);
    for (const pool of report.shared_workholding.resources || []) cells(pools, [pool.id, metric(pool.capacity),
      metric(report.baseline?.shared_peak_occupancy?.[pool.id]), metric(report.selected?.shared_peak_occupancy?.[pool.id])]);
  }
  const holds = table(parent, `${label}: selected routed fixture reservations`,
    ["Cell", "Garment copy", "Operation", "Hold", "Cell resource", "Shared pool", "Units", "Acquired (s)", "Released after unloading (s)"]);
  for (const hold of report.selected?.holds || []) cells(holds, [hold.cell, metric(hold.copy), hold.operation,
    hold.hold, hold.resource, hold.pool || "Cell local", metric(hold.units),
    metric(seconds(hold.acquired_at_milliseconds)), metric(seconds(hold.released_at_milliseconds))]);
  const note = document.createElement("p");
  note.textContent = "Routed fixtures are reserved before loading and remain occupied through setup, feed and unloading. " +
    "A release can supply an acquisition at the same instant. Shared pools constrain overlapping operations across specialist cells, including reservations already placed later in time. " +
    "These holds end at their operation; occupied fixtures are not transferred with garments. Physical fixture access, pool access and machine execution require separate qualification.";
  parent.append(note);
  detail(parent, `${label}: routed fixture sources, cell locks and complete reservation histories`, report);
}
function sewingRoutedLabor(parent, routing, label) {
  const report = routing.attending_labor;
  if (!report) return;
  const transfer = report.transfer;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: routed attending labor`,
    ["Schedule", "Travel (s)", "Waiting (s)", "Full attendance (s)", ...(transfer ? ["Garment carry (s)"] : [])]);
  for (const [name, timeline] of [["Baseline", report.baseline], ["Selected", report.selected]]) cells(totals, [name,
    metric(seconds(timeline?.travel_milliseconds)), metric(seconds(timeline?.waiting_milliseconds)),
    metric(seconds(timeline?.attended_milliseconds)), ...(transfer ? [metric(seconds(timeline?.transfer_milliseconds))] : [])]);
  const workers = table(parent, `${label}: qualified attending workers`,
    ["Worker", "Cells", "Available (s)", "Inclusive finish limit (s)"]);
  for (const worker of report.policy?.workers || []) cells(workers, [worker.id, (worker.cells || []).join(", "),
    metric(seconds(worker.available_at_milliseconds)), typeof worker.available_until_milliseconds === "number" ?
      metric(seconds(worker.available_until_milliseconds)) : "Open ended"]);
  const steps = table(parent, `${label}: selected attending worker timeline`,
    ["Worker", "Garment copy", "Operation", "From", "Cell", "Departure (s)", "Arrival (s)",
      "Loading (s)", "Feed (s)", "Unloading complete (s)"]);
  for (const step of report.selected?.steps || []) cells(steps, [step.worker, metric(step.copy), step.operation,
    step.from_cell ?? "Declared initial location", step.cell, metric(seconds(step.departure_milliseconds)),
    metric(seconds(step.arrival_milliseconds)), metric(seconds(step.loading_start_milliseconds)),
    metric(seconds(step.feed_start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  if (transfer) {
    const transfers = table(parent, `${label}: selected worker-owned garment transfers`,
      ["Worker", "Resource", "Garment copy", "After operation", "Before operation", "Worker from", "Pickup cell", "Deposit cell",
        "Approach starts (s)", "At pickup cell (s)", "Pickup and carry starts (s)", "Deposit complete (s)"]);
    for (const step of report.selected?.transfers || []) cells(transfers, [step.worker, step.resource, metric(step.copy),
      step.from_operation, step.to_operation, step.from_cell ?? "Declared initial location", step.source_cell, step.destination_cell,
      metric(seconds(step.departure_milliseconds)), metric(seconds(step.arrival_milliseconds)),
      metric(seconds(step.start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  }
  const note = document.createElement("p");
  note.textContent = "Each named worker attends the complete operation, including feed and final unloading. " +
    "Travel and waiting also occupy that worker; cells assigned to the same person cannot use them concurrently. " +
    (transfer ? `Worker ${transfer.attending_worker} performs the garment handoffs using resource ${transfer.id}. ` +
      "Empty approach is charged to worker travel; pickup, carrying and deposit occupy the same worker and end at the destination cell. " +
      "The worker's machine attendance and handoffs cannot overlap. " : "Garment handoffs retain their separate qualified resource. ") +
    "These source-qualified modeled times do not establish measured labor performance or hardware authorization.";
  parent.append(note);
  detail(parent, `${label}: attending worker sources, cell locks and complete replay`, report);
}
function sewingRouting(parent, plan, label = "Selected sewing plan") {
  const report = plan?.production?.routing;
  if (!report) return;
  const policy = plan.options?.production?.routing;
  const eligible = policy?.model === "eligible_operation_route/1";
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: routed completion and aggregate work`,
    ["Schedule", "Completion (s)", "Total feed work (s)", "Scheduled feed work (s)",
      "Total setup work (s)", "Total loading/unloading (s)", "Total transfer work (s)"]);
  for (const [name, timeline] of [[report.baseline_replaced ? "Feasible replacement baseline" : "Copy-order baseline", report.baseline],
      [eligible ? "Selected route and cell dispatch" : "Selected route dispatch", report.selected]]) cells(totals, [name,
    metric(seconds(timeline?.makespan_milliseconds)), metric(timeline?.feed_seconds),
    metric(seconds(timeline?.scheduled_feed_milliseconds)), metric(seconds(timeline?.setup_milliseconds)),
    metric(seconds(timeline?.handling_milliseconds)), metric(seconds(timeline?.transfer_milliseconds))]);
  const route = table(parent, `${label}: ${eligible ? "eligible cells by garment operation" : "fixed garment route"}`,
    ["Step", "Operation", "Physical cell", ...(eligible ? ["Authored seed", "Selected copies"] : []),
      "Qualified machine", "Needle", "Thread", "Maximum stitch (mm)", "Stitches/min"]);
  for (const [index, step] of (report.route || []).entries()) {
    for (const cell of eligible ? step.eligible_cells || [] : [step.cell]) {
      const source = report.cell_plans?.find(item => item.cell === cell);
      const pass = source?.plan?.seams?.find(item => (item.pass.id || item.pass.seam) === step.operation)?.pass;
      const copies = (report.selected?.operations || []).filter(item => item.operation_id === step.operation && item.cell === cell).length;
      cells(route, [metric(index + 1), step.operation, cell,
        ...(eligible ? [cell === step.cell ? "Yes" : "No", metric(copies)] : []),
        source?.plan?.options?.machine?.id || "Unavailable", pass?.needle || "Unavailable", pass?.thread || "Unavailable",
        metric(pass?.max_stitch_mm), metric(pass?.stitches_per_minute)]);
    }
  }
  const operations = table(parent, `${label}: selected routed operation timeline`,
    ["Global garment copy", "Operation", "Cell", "Loading starts (s)", "Setup starts (s)",
      "Feed starts (s)", "Feed finishes (s)", "Unloaded and cell free (s)"]);
  for (const step of report.selected?.operations || []) cells(operations, [metric(step.copy), step.operation_id, step.cell,
    metric(seconds(step.loading_start_milliseconds)), metric(seconds(step.setup_start_milliseconds)),
    metric(seconds(step.feed_start_milliseconds)), metric(seconds(step.feed_finish_milliseconds)),
    metric(seconds(step.finish_milliseconds))]);
  const transfers = table(parent, `${label}: selected garment transfers`,
    ["Global garment copy", "After operation", "Before operation", "From cell", "To cell", "Starts (s)", "Arrives (s)"]);
  for (const step of report.selected?.transfers || []) cells(transfers, [metric(step.copy), step.from_operation,
    step.to_operation, step.from, step.to, metric(seconds(step.start_milliseconds)), metric(seconds(step.finish_milliseconds))]);
  const resources = table(parent, `${label}: qualified routing availability`,
    ["Resource", "Available (s)", "Inclusive finish limit (s)"]);
  for (const cell of policy?.cells || []) cells(resources, [cell.id, metric(seconds(cell.available_at_milliseconds)),
    typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended"]);
  if (policy?.transfer) cells(resources, [policy.transfer.id, metric(seconds(policy.transfer.available_at_milliseconds)),
    typeof policy.transfer.available_until_milliseconds === "number" ? metric(seconds(policy.transfer.available_until_milliseconds)) : "Open ended"]);
  if (report.availability_calendars) {
    const calendars = report.availability_calendars;
    const windows = table(parent, `${label}: nonpreemptive operating windows`,
      ["Resource", "Window", "Opens (s)", "Inclusive finish (s)"]);
    for (const [resource, intervals] of Object.entries(calendars.resources)) {
      for (const [index, interval] of intervals.entries()) cells(windows, [resource, metric(index + 1),
        metric(seconds(interval.start_milliseconds)), metric(seconds(interval.end_milliseconds))]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Each listed window contains a complete occupied interval. Machine loading, setup, feed and unloading stay together; " +
      "worker windows include empty travel, waiting and full attendance or carrying. Fixture delays recheck the windows. " +
      "Gaps preserve machine setup and worker location. Resources without a listed calendar retain their outer availability. " +
      "These qualified planning inputs do not reserve live resources or authorize hardware.";
    parent.append(scope);
    detail(parent, `${label}: operating calendar source and exact windows`, calendars);
  }
  sewingRoutedWorkholding(parent, report, label);
  sewingRoutedLabor(parent, report, label);
  for (const source of report.cell_plans || []) {
    sewingAccess(parent, source.plan, `${label}: routed cell ${source.cell}`);
    detail(parent, `${label}: routed cell ${source.cell} admitted operations and exact geometry`, source);
  }
  const note = document.createElement("p");
  note.textContent = (report.optimality_proven ?
    (eligible ? "Minimum completion time proved by the declared eligible-cell model's independent lower bound. " :
      "Minimum completion time proved within the declared fixed-route dispatch search. ") :
    "Complete feasible routed schedule retained; minimum completion time is not proved. ") +
    `Lower bound: ${metric(seconds(report.lower_bound_makespan_milliseconds))} s; search work: ${metric(report.search_nodes)}. ` +
    (report.budget_exhausted ? "The dispatch search reached its work limit. " : "") +
    (report.baseline_replaced ? "The copy-order seed could not satisfy the qualified routing constraints; a complete feasible replacement is shown. " : "") +
    (eligible ?
      "Each garment follows the same explicit operation sequence and can choose a different qualified cell at each step. " +
      "Every eligible operation/cell pair is qualified; selected copy counts distinguish used recipes from unused alternatives. " +
      "The outer one-garment seam table is the authored seed reference; the selected timeline, stitch stream and batch totals use actual cell assignments. " :
      "Each garment follows the same explicit operation route. Each specialist cell qualifies only its assigned operations. ") +
    (report.attending_labor ? "Explicit worker ownership and availability constrain the complete loading, setup, feed and unloading interval. " :
      "Each dedicated worker attends loading, setup, feed and unloading before the cell becomes free. ") +
    (policy?.transfer?.attending_worker ?
      `Worker ${policy.transfer.attending_worker} and the qualified transfer resource are both reserved from pickup through deposit; ` +
      "empty approach uses the worker's travel qualification. " :
      "One separately qualified transfer resource serializes unloaded garment handoffs; its source duration includes positioning, pickup, movement and deposit. ") +
    `Qualified staging has ${metric(policy?.staging_capacity)} garment slots. ` +
    "Stitch geometry and thread demand come from the actual assigned cell. Elapsed completion is distinct from summed machine and transfer work. " +
    "The proof covers the supplied route and dispatch model; it does not establish unrestricted factory optimality, measured seam quality or hardware authorization.";
  parent.append(note);
  detail(parent, `${label}: route sources, cell locks and complete baseline/selected timelines`, { policy, report });
}
function sewingParallel(parent, plan, label = "Selected sewing plan") {
  const report = plan?.production?.parallel;
  if (!report) return;
  const heterogeneous = Boolean(report.cell_plans?.length);
  const windowed = Boolean(report.availability);
  const operator = report.shared_operator;
  const pools = report.shared_workholding;
  const seconds = value => typeof value === "number" ? value / 1000 : null;
  const totals = table(parent, `${label}: parallel completion and aggregate work`,
    ["Allocation", "Completion (s)", "Total feed work (s)", "Scheduled feed work (s)", "Total setup work (s)", "Total handling work (s)"]);
  const baselineLabel = report.availability?.baseline_replaced || operator?.baseline_replaced ? "Feasible replacement baseline" : "Single-cell baseline";
  for (const [name, allocation] of [[baselineLabel, report.baseline], ["Selected cells", report.selected]]) {
    cells(totals, [name, metric(seconds(allocation?.makespan_milliseconds)), metric(allocation?.feed_seconds),
      metric(seconds(allocation?.scheduled_feed_milliseconds)), metric(seconds(allocation?.setup_milliseconds)),
      metric(seconds(allocation?.handling_milliseconds))]);
  }
  const machines = table(parent, `${label}: selected physical cells`,
    ["Cell", ...(heterogeneous ? ["Qualified machine"] : []), "Global garment copies (zero-based)", "Available (s)",
      ...(windowed ? ["Finish limit (s)"] : []), "Complete (s)", "Raw feed (s)", "Feed rounding (ms)", "Cell order minimum proved"]);
  const operations = table(parent, `${label}: cell-local sewing order`,
    ["Cell", "Position in cell", "Global garment copy", "Operation", "Needle", "Thread",
      ...(heterogeneous ? ["Maximum stitch (mm)", "Stitches/min"] : []), "Required local predecessors"]);
  for (const cell of report.selected?.cells || []) {
    const schedule = cell.schedule;
    const source = report.cell_plans?.find(item => item.cell === cell.cell);
    const sourcePlan = source?.plan || plan;
    cells(machines, [cell.cell, ...(heterogeneous ? [sourcePlan.options?.machine?.id || "Unavailable"] : []),
      cell.copies.join(", ") || "Idle", metric(seconds(cell.available_at_milliseconds)),
      ...(windowed ? [typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended"] : []),
      schedule ? metric(seconds(cell.finish_milliseconds)) : "Idle", metric(schedule?.feed_seconds),
      metric(typeof cell.feed_padding_seconds === "number" ? cell.feed_padding_seconds * 1000 : null),
      schedule ? String(schedule.ordering.optimality_proven) : "No work"]);
    if (!schedule) continue;
    for (const [i, id] of schedule.ordering.selected_order.entries()) {
      const operation = schedule.operations.find(item => item.id === id);
      const pass = sourcePlan.seams[operation.seam_index].pass;
      cells(operations, [cell.cell, metric(i + 1), metric(cell.copies[operation.copy]),
        operation.operation_id, pass.needle, pass.thread,
        ...(heterogeneous ? [metric(pass.max_stitch_mm), metric(pass.stitches_per_minute)] : []),
        operation.after.join(", ") || "None"]);
    }
    const policy = plan.options?.production?.parallel?.cells?.find(item => item.id === cell.cell);
    const local = { production: schedule, options: { production: policy } };
    sewingWorkholding(parent, local, `${label}: cell ${cell.cell} (local copy IDs)`);
    sewingHandling(parent, local, `${label}: cell ${cell.cell} (local copy IDs)`);
    if (heterogeneous) {
      sewingAccess(parent, sourcePlan, `${label}: cell ${cell.cell}`);
      detail(parent, `${label}: cell ${cell.cell} qualified machine, recipes and exact station geometry`, source);
    }
  }
  if (windowed) {
    const windows = table(parent, `${label}: ${operator ? "cell-only window relaxation" : "qualified cell availability"}`,
      ["Cell", "Garment count", "Finish limit (s)", "Complete incumbent finish (s)", "Finish lower bound (s)", "Window admission"]);
    for (const cell of report.availability.cells || []) {
      for (const count of cell.counts || []) cells(windows, [cell.cell, metric(count.copies),
        typeof cell.available_until_milliseconds === "number" ? metric(seconds(cell.available_until_milliseconds)) : "Open ended",
        metric(seconds(count.incumbent_finish_milliseconds)), metric(seconds(count.lower_bound_finish_milliseconds)),
        count.feasible ? (pools ? "Fits without shared-resource contention" : operator ? "Fits without operator contention" : "Fits") : count.infeasibility_proven ? "Cannot fit: proved" : "Unresolved search"]);
    }
  }
  if (operator) {
    const labor = table(parent, `${label}: shared operator work`,
      ["Operator", "Busy (s)", "Travel (s)", "Waiting (s)", "Restricted search complete", "Operator work limit reached"]);
    cells(labor, [operator.operator, metric(seconds(operator.selected?.busy_milliseconds)),
      metric(seconds(operator.selected?.transfer_milliseconds)), metric(seconds(operator.selected?.waiting_milliseconds)),
      String(operator.search_complete), String(operator.budget_exhausted)]);
    const timeline = table(parent, `${label}: selected shared operator timeline`,
      ["Cell", "Local operation", "Global copy", "Departure (s)", "Arrival (s)", "Handling starts (s)",
        "Setup starts (s)", "Operator free (s)", "Feed complete (s)"]);
    for (const step of operator.selected?.steps || []) cells(timeline,
      [step.cell, step.operation ?? "Final unloading", typeof step.copy === "number" ? metric(step.copy) : "Batch end",
        metric(seconds(step.departure_milliseconds)), metric(seconds(step.arrival_milliseconds)),
        metric(seconds(step.handling_start_milliseconds)), metric(seconds(step.setup_start_milliseconds)),
        metric(seconds(step.operator_release_milliseconds)), metric(seconds(step.feed_finish_milliseconds))]);
  }
  sewingSharedWorkholding(parent, plan, label);
  const note = document.createElement("p");
  note.textContent = (report.optimality_proven ?
    "Minimum parallel completion time proved under the declared millisecond planning model. " :
    "Complete feasible cell allocation retained; the minimum parallel completion time is not proved. ") +
    `Lower bound: ${metric(seconds(report.lower_bound_makespan_milliseconds))} s; search work: ${metric(report.search_nodes)}. ` +
    (report.budget_exhausted ? (operator ? "At least one serial or operator search reached its work limit. " : "At least one inner schedule reached its work limit. ") : "") +
    (windowed ? "Every used cell finishes inside its inclusive qualified availability window, including final unloading. " +
      "A lower bound that still fits leaves an over-limit incumbent unresolved. " : "") +
    (report.availability?.baseline_replaced || operator?.baseline_replaced ?
      `The first-cell baseline failed a qualified cell or operator window${pools ? " or shared fixture constraint" : ""}; the displayed replacement is the complete selected feasible assignment. ` : "") +
    "Every garment stays in one physical cell. " +
    (operator ? "One qualified operator performs exclusive travel, handling, setup and final unloading while explicitly qualified autonomous machine feed may overlap. " +
      "The operator search considers the retained serial operation orders; completing that search alone does not prove an unrestricted joint optimum. " +
      (pools ? "Cell-only window receipts ignore shared operator and fixture contention; the selected timeline enforces cell, operator and pool limits. " :
        "Cell-only window receipts ignore operator contention; the selected timeline enforces both cell and operator limits. ") +
      "Each positive feed operation keeps its own upward-rounded time slot, including after attended work or waiting. " :
      "Concurrent operation requires independently qualified machines, fixtures and handling labor. Feed is rounded upward once per used cell. ") +
    "Each used cell includes its own availability, initial setup and final unloading; raw feed and padding remain separate. " +
    "Elapsed completion is distinct from summed machine work. The retained single-machine reference is not the selected concurrent order. " +
    (heterogeneous ? "Stitch pitch, feed, qualifications and thread consumption come from each selected cell's own admitted plan. " +
      "The allocation minimizes elapsed completion; its thread purchasing cost and total feed are reported consequences, not separately optimized quantities. " : "") +
    (operator ? "This calculation does not qualify garment transfers, unmodeled shared resources or hardware execution." :
      "This calculation does not qualify garment transfers, shared operators or hardware execution.");
  parent.append(note);
  detail(parent, `${label}: cell independence, timing sources and complete allocations`, {
    policy: plan.options?.production?.parallel, report });
}
function sewingAccess(parent, plan, label = "Selected sewing plan") {
  if (plan?.production?.parallel?.cell_plans?.length || plan?.production?.routing) return;
  const seams = plan?.seams?.filter(row => row.access || row.pass?.access) || [];
  if (!seams.length) return;
  const clearances = table(parent, `${label}: planar tool and fixture clearance`,
    ["Operation", "Result", "Tool radius (mm)", "Required gap (mm)", "Minimum gap (mm)", "Fixtures", "Complete path segments"]);
  const witnesses = table(parent, `${label}: limiting planar clearance witnesses`,
    ["Operation", "Fixture", "Needle segment", "Fixture edge", "Needle point (mm)", "Fixture point (mm)"]);
  const point = value => Array.isArray(value) ? `[${value.map(metric).join(", ")}]` : "Unknown";
  for (const seam of seams) {
    const report = seam.access, operation = seam.pass.id || seam.pass.seam;
    cells(clearances, [operation, report ? "Declared planar model checked" : "No retained clearance result",
      metric(report?.tool_radius_mm), metric(report?.required_clearance_mm), metric(report?.minimum_clearance_mm),
      metric(report?.fixture_count), metric(report?.checked_segments)]);
    if (report?.witness) {
      const w = report.witness;
      cells(witnesses, [operation, w.fixture, point(w.needle_segment), point(w.fixture_edge),
        point(w.needle_mm), point(w.fixture_mm)]);
    }
    detail(parent, `${label}: ${operation} tool geometry, assembly and qualification sources`, {
      policy: seam.pass.access, assembly: seam.assembly, report });
  }
  const note = document.createElement("p");
  note.textContent = "Clearance uses the declared circular tool envelope over every complete needle-path segment in the operation-local assembly frame. " +
    "Reverse and backtack runs revisit the same swept geometry. The source policy is tied to the exact machine and admitted material/process qualification. " +
    "These retained planar calculations do not establish fixture grip, three-dimensional approach clearance, machine registration or hardware authorization. " +
    "Operations without a retained result remain unchecked.";
  parent.append(note);
}
function sewingResult(parent, r) {
  const plan = r.plan, production = plan?.production, parallel = production?.parallel, routing = production?.routing;
  const order = parallel || routing ? null : production?.ordering || plan?.ordering;
  if (order) {
    const counts = table(parent, "Sewing setup changes — declared needle and thread identities",
      ["Schedule", "Needle changes", "Thread changes", "Total changes"]);
    for (const [label, changes] of [[order.baseline_replaced ? "Feasible replacement baseline" : "Original dependency order", order.baseline_changes],
      ["Selected order", order.selected_changes]]) {
      cells(counts, [label, metric(changes.needle_changes), metric(changes.thread_changes), metric(changes.total_changes)]);
    }
    const operations = table(parent, production ? "Selected cross-garment sewing order" : "Selected sewing operation order",
      ["Position", "Operation", "Source seam", "Needle", "Thread", "Required predecessors"]);
    const selected = production ? order.selected_order.map(id => {
      const operation = production.operations.find(o => o.id === id);
      return { ...plan.seams[operation.seam_index], productionOperation: operation };
    }) : plan.seams;
    for (const [i, row] of selected.entries()) {
      const pass = row.pass;
      cells(operations, [metric(i + 1), row.productionOperation?.id || pass.id || pass.seam,
        pass.seam, pass.needle, pass.thread, (row.productionOperation?.after || pass.after).join(", ") || "None declared"]);
    }
    const note = document.createElement("p");
    const objective = order.handling_timing ? "declared setup plus handling duration" :
      order.timing ? "declared setup duration" : "setup-change count";
    note.textContent = (order.optimality_proven ?
      `Minimum ${objective} proven for the declared operations, dependencies, initial setup${production?.workholding ? " and finite fixtures" : ""}. ` :
      `Best complete feasible schedule found; minimum ${objective} has not been proven. `) +
      (order.budget_exhausted ? "The search reached its declared work limit. " : "") +
      (order.handling_timing ? "Needle and thread changes remain separate counts; declared handling is included in the joint duration objective. " :
        "Needle and thread changes each count once. Handling, rethreading duration, assembly access and total manufacturing time are not inferred. ") +
      ((plan.options.production?.ordering || plan.options.ordering).initial_setup ? "The supplied initial setup is included in the counts." :
        "No initial setup was supplied, so installing the first setup is not counted.");
    parent.append(note);
    detail(parent, "Baseline order, selected order and bounded search result", order);
  }
  sewingWorkholding(parent, plan);
  sewingHandling(parent, plan);
  sewingParallel(parent, plan);
  sewingRouting(parent, plan);
  if (!parallel?.cell_plans?.length && !routing) sewingAccess(parent, plan);
  if (plan?.seams.some(row => row.source_span)) {
    const spans = table(parent, routing?.reference_scope ? "Seed-route partial-seam reference — one garment" :
      "Partial-seam operations — original source correspondence",
      ["Operation", "Source seam", "Side A range", "Side B range", "Full source lengths (mm)", "Traveled lengths (mm)", "Direction"]);
    for (const row of plan.seams) {
      const span = row.source_span, range = side => span ? `[${span.source_ranges[side].map(metric).join(", ")}]` : "Whole seam";
      cells(spans, [row.pass.id || row.pass.seam, row.pass.seam, range(0), range(1),
        span ? span.source_lengths_mm.map(metric).join(" / ") : "See full seam stations",
        span ? span.sewn_lengths_mm.map(metric).join(" / ") : "See full seam stations", row.pass.direction]);
      if (span) detail(parent, `${row.pass.id || row.pass.seam}: retained source span`, span);
    }
    const note = document.createElement("p");
    note.textContent = "Ranges are fractions of each original seam side in paired traversal order; side B includes the seam's reverse setting. " +
      "The planner checks complete coverage on both source sides. Overlapping passes can reinforce a join. " +
      "These ranges preserve the declared notch correspondence and do not authorize a physical fold transition or machine execution.";
    parent.append(note);
  }
  if (plan?.consumables) sewingConsumablesComparison(parent,
    [{ key: "Selected sewing plan", sewing: { status: "available", result: plan } }]);
  return { schema: r.schema, stitches: r.stitches?.length, production_copies: production?.production_copies,
    feed_seconds: production?.feed_seconds ?? plan?.feed_seconds,
    setup_seconds: order?.timing ? order.timing.selected_setup_milliseconds / 1000 : undefined,
    feed_and_setup_seconds: order?.timing?.feed_and_setup_seconds,
    handling_seconds: order?.handling_timing ? order.handling_timing.selected_handling_milliseconds / 1000 : undefined,
    modeled_production_seconds: order?.handling_timing?.selected_modeled_production_seconds,
    parallel_makespan_seconds: parallel ? parallel.selected.makespan_milliseconds / 1000 : undefined,
    parallel_lower_bound_seconds: parallel ? parallel.lower_bound_makespan_milliseconds / 1000 : undefined,
    parallel_optimality_proven: parallel?.optimality_proven,
    parallel_search_work: parallel?.search_nodes,
    routed_makespan_seconds: routing ? routing.selected.makespan_milliseconds / 1000 : undefined,
    routed_lower_bound_seconds: routing ? routing.lower_bound_makespan_milliseconds / 1000 : undefined,
    routed_optimality_proven: routing?.optimality_proven,
    routed_search_work: routing?.search_nodes,
    routed_calendar_resources: routing?.availability_calendars ? Object.keys(routing.availability_calendars.resources).length : undefined,
    garment_transfers: routing?.selected?.transfers?.length,
    transfer_seconds: routing ? routing.selected.transfer_milliseconds / 1000 : undefined,
    setup_changes: order?.selected_changes,
    ordering_objective: order ? (order.handling_timing ? "minimize_production_time" :
      order.timing ? "minimize_setup_time" : "minimize_setup_changes") : undefined,
    ordering_optimality_proven: order?.optimality_proven,
    finite_workholding_enforced: (routing ? routing.workholding : parallel ? parallel.selected.cells.some(cell => cell.schedule?.workholding) : production?.workholding) ? true : undefined,
    shared_fixture_pools_enforced: (routing?.workholding?.shared_workholding || parallel?.shared_workholding) ? true : undefined,
    planar_access_checked_operations: (routing ? new Set(routing.selected.operations
      .filter(operation => routing.cell_plans.find(source => source.cell === operation.cell)?.plan.seams[operation.seam_index]?.access)
      .map(operation => JSON.stringify([operation.cell, operation.seam_index]))).size : parallel?.cell_plans?.length ? parallel.selected.cells
      .filter(cell => cell.copies.length).reduce((count, cell) => count +
        (parallel.cell_plans.find(source => source.cell === cell.cell)?.plan.seams.filter(row => row.access).length || 0), 0) :
      plan?.seams?.filter(row => row.access).length) || undefined,
    search_nodes: order?.search_nodes, hardware_execution_authorized: plan?.hardware_execution_authorized };
}
function permanentStretchResult(parent, r) {
  const c = r.calibration, d = c.diagnostics;
  const recovery = c.input.material.membrane_relaxation;
  const identifiedRecovery = c.input.joint_relaxation;
  const joint = Boolean(identifiedRecovery || recovery?.parallel_plasticity);
  const decision = document.createElement("p");
  decision.textContent = `Permanent-stretch identification: ${c.status}. Held-out replay: ${c.check_status}. ` +
    c.decision_reasons.join(" ") + (c.check_refusal ? ` ${c.check_refusal.message}` : "");
  parent.append(decision);
  const errors = table(parent, "Cyclic load errors — independent whole histories",
    ["Partition", "Observed components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], [`Held out: ${c.check_status}`, c.check]]) {
    cells(errors, [name, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  const parameters = table(parent, "Selected FIT coefficients and declared search bounds",
    ["Material direction", "Yield (N/m)", "Yield search range (N/m)", "Hardening (N/m)", "Hardening search range (N/m)"]);
  const domain = table(parent, "Source-declared total and accumulated Green-strain limits",
    ["Material direction", "Total strain limit", "Accumulated plastic-strain limit"]);
  const sourceQuantity = q => q ? `${metric(q.value)} ${q.unit}` : "Unknown";
  for (const [i, axis] of ["warp", "weft", "shear"].entries()) {
    const name = ["Warp / wale", "Weft / course", "Engineering shear"][i];
    const bounds = values => `[${values.map(metric).join(", ")}]`;
    cells(parameters, [name, metric(c.parameters?.yield_n_m[i]), bounds(c.input.yield_bounds_n_m[i]),
      metric(c.parameters?.hardening_n_m[i]), bounds(c.input.hardening_bounds_n_m[i])]);
    const properties = c.input.material.properties;
    cells(domain, [name, sourceQuantity(properties[`membrane_strain_limit_${axis}`]),
      sourceQuantity(properties[`membrane_accumulation_limit_${axis}`])]);
  }
  if (joint) {
    if (identifiedRecovery) {
      const spectrum = table(parent, "Jointly identified recovery — selected FIT coefficients",
        ["Branch / coefficient", "Selected value", "Search range", "FIT sensitivity per unit", "At search bound"]);
      const selected = c.parameters?.relaxation, diagnostics = d.joint_relaxation;
      for (const [i, label, value, bounds] of [
        [0, "Equilibrium-tensor stiffness ratio", selected?.stiffness_ratio, identifiedRecovery.stiffness_ratio_bounds],
        [1, "Relaxation time (s)", selected?.relaxation_time_seconds, identifiedRecovery.relaxation_time_bounds_seconds]
      ]) cells(spectrum, [`${identifiedRecovery.branch_id}: ${label}`, metric(value),
        `[${bounds.map(metric).join(", ")}]`,
        d.sensitivity_evaluated ? metric(diagnostics?.parameter_sensitivity[i]) : "Not evaluated",
        d.sensitivity_evaluated ? (diagnostics?.at_bounds[i] ? "Yes" : "No") : "Not evaluated"]);
      detail(parent, "Joint model, fixed material properties and original sources",
        { joint_relaxation: identifiedRecovery, fixed_properties: c.input.material.properties,
          replaced_recovery: recovery, sources: c.input.material.sources });
    } else {
      const spectrum = table(parent, "Delayed recovery — independently supplied, held fixed during identification",
      ["Branch", "Stiffness definition", "Relaxation time", "Time evidence", "Time source"]);
      for (const branch of recovery.branches || []) cells(spectrum,
        [branch.id, `Equilibrium-tensor ratio: ${sourceQuantity(branch.stiffness_ratio)}`,
          sourceQuantity(branch.relaxation_time), branch.relaxation_time.evidence, branch.relaxation_time.source]);
      for (const branch of recovery.orthotropic_branches || []) cells(spectrum,
        [branch.id, ["warp", "weft", "coupling", "shear"].map(axis =>
          `${axis}: ${sourceQuantity(branch.stiffness[axis])}`).join("; "),
          sourceQuantity(branch.relaxation_time), branch.relaxation_time.evidence, branch.relaxation_time.source]);
      detail(parent, "Fixed recovery spectrum, joint-network qualification and original sources",
        { recovery, sources: c.input.material.sources });
    }
    const endings = new Map();
    for (const prediction of c.predictions || []) if (prediction.parallel_history) endings.set(prediction.experiment, prediction);
    if (endings.size) {
      const histories = table(parent, "Final recorded state of each independent history — work per reference area",
        ["History / partition", "Equilibrium storage (J/m²)", "Maxwell storage (J/m²)", "Total storage (J/m²)",
          "Plastic loss (J/m²)", "Dashpot work (J/m²)"]);
      for (const prediction of endings.values()) {
        const state = prediction.parallel_history;
        cells(histories, [`${prediction.experiment} / ${prediction.partition}`, metric(prediction.stored_j_m2),
          metric(state.maxwell_stored_j_m2), metric(state.total_stored_j_m2),
          metric(prediction.plastic_dissipation_j_m2), metric(state.maxwell_dissipation_j_m2)]);
      }
    }
  }
  const work = table(parent, "Identification work and FIT excitation", ["Diagnostic", "Value"]);
  for (const row of [
    ["Completed / attempted starts", `${metric(d.starts_completed)} / ${metric(d.starts_attempted)}`],
    ["Forward evaluations / limit", `${metric(d.evaluations)} / ${metric(d.evaluation_limit)}`],
    ["Reserved history nodes / limit", `${metric(d.history_steps)} / ${metric(d.history_step_limit)}`],
    ["Refused forward trials", metric(d.trials_refused)],
    ["FIT excitation pivot", d.sensitivity_evaluated ? metric(d.excitation_pivot) : "Not evaluated"]
  ]) cells(work, row);
  if (identifiedRecovery) cells(work, ["Joint Maxwell branch-node advances (included above)", metric(d.joint_relaxation?.branch_steps)]);
  else if (joint) cells(work, ["Fixed Maxwell branch-node advances (included above)", metric(d.fixed_maxwell_branch_steps)]);
  const note = document.createElement("p");
  note.textContent = (identifiedRecovery ?
    "Six permanent-stretch and hardening coefficients and one Maxwell stiffness ratio and recovery time are identified together. " +
    "The FIT excitation check includes all eight unknowns; a locally unresolved combination cannot publish a material. " +
    "Every trial advances both histories at every recorded node, including nodes with unobserved forces. " +
    "The supplied equilibrium elasticity, domains and network interpretation remain fixed. " : joint ?
    "Only the six permanent-stretch and hardening coefficients are identified. The independently qualified Maxwell spectrum stays fixed. " +
    "Recorded elapsed times advance its delayed-recovery state, including nodes with unobserved forces; residuals compare the total parallel-network load. " :
    "The shared rate-independent law follows every recorded loading, unloading and reversal node, including nodes with unobserved forces. ") +
    "Strains are raw material-frame Green strain; loads are second-Piola sheet resultants in N/m. " +
    "FIT chooses the coefficients before CHECK replay. Search ranges, force scales and local sensitivity are not confidence intervals or a proof of global uniqueness. " +
    "Applicability limits remain supplied source observations; the fitter does not infer them from the largest load. Fitted coefficients remain estimates." +
    (joint ? " Equilibrium and Maxwell storage remain separate from plastic loss and physical dashpot work; none is automatically injected as heat." : "");
  parent.append(note);
  detail(parent, "Complete source histories, permanent strain, stored energy, dissipation and residuals", c);
  return { response: identifiedRecovery ? "Joint permanent stretch and delayed-recovery identification" :
    joint ? "Permanent stretch with independently characterized delayed recovery" :
    "Permanent stretch and directional hardening", status: c.status,
    held_out: c.check_status, input_sha256: c.input_sha256, material_id: r.selection?.id,
    allow_estimates: r.selection?.allow_estimates,
    mechanical_coefficients_available: r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material?.capabilities.mechanical_coefficients?.refusal,
    next: r.selection ? "Review capabilities, then assign the identified selection above and bind the pattern." :
      "Review the retained diagnostics and source histories. No identified selection is available to assign." };
}
function areaCalibrationResult(parent, c) {
  const area = c.material?.membrane_response?.area_response, fittedRange = c.fitted_area_ratio_range;
  const declared = c.input.area_response?.area_ratio_range;
  const inherited = c.input.material?.membrane_response?.area_response?.area_ratio_range;
  if (!area && !fittedRange && !declared && !inherited) return;
  const preserved = c.input.area_response?.mode === "preserve";
  if (area) {
    const parameters = table(parent, preserved ? "Area coefficients — preserved source observations" :
      "Area coefficients — joint identification", ["Energy term", "Coefficient", "Unit", "Evidence", "Source"]);
    for (const [label, quantity] of [["(J − 1)²", area.quadratic], ["(ln J)²", area.log_squared]]) {
      cells(parameters, [label, metric(quantity.value), quantity.unit, quantity.evidence, quantity.source]);
    }
    const conditions = document.createElement("p");
    conditions.textContent = `Area specimen: ${area.conditions.specimen}. Conditioning: ${area.conditions.conditioning}.`;
    parent.append(conditions);
    detail(parent, "Area response, complete applicability conditions and source evidence", {
      response: area,
      sources: Object.fromEntries([...new Set([area.quadratic.source, area.log_squared.source])]
        .map(source => [source, c.material.sources?.[source] ?? null]))
    });
  }
  const domain = table(parent, "Physical area ratio J — applicability", ["Domain", "Minimum J", "Maximum J"]);
  if (declared) cells(domain, ["Admissible input domain", ...declared.map(metric)]);
  if (inherited) cells(domain, [declared ? "Inherited material domain" : "Admissible input domain", ...inherited.map(metric)]);
  if (fittedRange) cells(domain, ["FIT observations and undeformed reference", ...fittedRange.map(metric)]);
  if (area) cells(domain, ["Published material domain", ...area.area_ratio_range.map(metric)]);
  if (c.parameter_count != null) {
    const excitation = table(parent, "Joint identification diagnostics", ["Diagnostic", "Value"]);
    cells(excitation, ["Identified coefficients", metric(c.parameter_count)]);
    cells(excitation, ["Normalized FIT excitation pivot", metric(c.excitation_pivot)]);
  }
  const note = document.createElement("p");
  note.textContent = "J is deformed area divided by reference area. " + (preserved ?
    "The area coefficients, source evidence and area-ratio interval are retained from the supplied material. " :
    fittedRange ? "FIT observations determine the area coefficients and their supported J range, including the undeformed reference. CHECK cannot widen that range. " :
      "Only an accepted result publishes area coefficients and a supported J range. ") +
    (declared && inherited ? "Trials must satisfy both the declared and inherited J intervals. " : "") +
    "The area domain also intersects the directional Green-strain limits. An area-energy response does not impose constant area.";
  parent.append(note);
}
function stripCalibrationResult(parent, r) {
  const c = r.calibration, family = c.input.response_family, nonlinear = c.input.nonlinear;
  const quality = table(parent, "Clamped strip observations — independent partitions",
    ["Partition", "Observed channels", "Scaled RMS", "Scaled peak"]);
  for (const [name, q] of [["FIT", c.fit], ["CHECK", c.check]]) {
    cells(quality, [name, metric(q?.components), metric(q?.rms_scaled), metric(q?.peak_scaled)]);
  }
  const area = c.input.area_response, scalarArea = area && !family && !nonlinear;
  const parameterTitle = area?.mode === "identify" ? scalarArea ? "scalar base and physical-area coefficients" :
    "directional and physical-area coefficients" :
    scalarArea ? "scalar orthotropic base coefficients" :
    nonlinear ? "nonlinear sheet tangents and coupling" :
    family ? "nonlinear shape scales and coupling" : "orthotropic sheet coefficients";
  const parameters = table(parent, `${c.accepted ? "Identified" : "Candidate"} ${parameterTitle}`,
    ["Coefficient", "Value", "Unit", "Search minimum", "Search maximum"]);
  const descriptors = c.parameter_descriptors ?? [
    {label:"Warp / wale stiffness",unit:"N/m"},
    {label:"Weft / course stiffness",unit:"N/m"},
    {label:"Major Poisson ratio",unit:"1"},
    {label:"Shear stiffness",unit:"N/m"}
  ];
  const parameterBounds = c.parameter_bounds?.length ? c.parameter_bounds : c.input.parameter_bounds;
  for (const [i, descriptor] of descriptors.entries()) {
    cells(parameters, [descriptor.label, metric(c.parameters[i]), descriptor.unit,
      ...parameterBounds[i].map(metric)]);
  }
  if (scalarArea) {
    const support = table(parent, "Scalar base — declared trial strain support",
      ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(support, [axis, ...area.scalar_green_strain_range[i].map(metric)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "The four scalar coefficients describe the orthotropic base; physical-area energy is a separate contribution. " +
      "The base Poisson ratio is not the transverse response of the combined law. " +
      "These signed intervals constrain trials. Published support comes from FIT element strains and production replay.";
    parent.append(scope);
  }
  if (nonlinear) {
    const knots = table(parent, "Declared nonlinear Green-strain intervals",
      ["Material direction", "Fixed strain knots", "Independent tangent intervals"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(knots, [axis, nonlinear.strain_knots[i].map(metric).join(", "),
        metric(nonlinear.tangent_bounds[i].length)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "Every declared signed interval has its own fitted positive tangent. " +
      "The fixed knots are caller-supplied model choices; no compression branch or symmetry is inferred. " +
      "Accepted curves retain only the strain support established by FIT and verified by production replay.";
    parent.append(scope);
  }
  if (family) {
    const scope = document.createElement("p");
    scope.textContent = "The warp, weft and shear curve shapes and strain knots were supplied by the request. " +
      "The fit identifies three dimensionless shape scales and signed normal coupling in N/m" +
      (area?.mode === "identify" ? ", jointly with the two physical-area coefficients. " : ". ") +
      "It does not identify the curve shapes or impose area preservation. Shape evidence, acquisition observations and fitted outputs require their separate permissions.";
    parent.append(scope);
    detail(parent, "Declared nonlinear response shapes and source evidence", family);
  }
  if (area) {
    const scope = document.createElement("p");
    scope.textContent = area.mode === "identify" ?
      "The physical-area coefficients participate in the same joint fit, excitation and work limits. " +
        "Their declared J domain and FIT-only strain support remain authoritative; search bounds are not confidence intervals. " +
        (!c.input.material?.membrane_response?.area_response ? "No prior area coefficients were supplied." : "") :
      "The physical-area coefficients, J domain and source evidence remain fixed while directionality is identified.";
    parent.append(scope);
    const model = c.input.material?.membrane_response?.area_response;
    if (model) detail(parent, "Declared physical-area model and source conditions", model);
  }
  const geometry = table(parent, "Actual strip geometry and declared width comparison",
    ["Experiment", "Partition", "Width (m)", "Free gauge (m)", "Warp axis [across, along]", "Authored mesh cells", "Width observation"]);
  for (const e of c.input.experiments) {
    const observed = [];
    if (e.observations.some(o => o.mid_width != null)) observed.push("Chord at half the reference gauge");
    if (e.observations.some(o => o.extensometers?.length)) observed.push("Declared extensometers (see below)");
    cells(geometry, [e.id, e.partition, metric(e.fixture.width_m), metric(e.fixture.gauge_length_m),
      e.fixture.warp_axis.map(metric).join(", "), e.fixture.cells.join(" × "),
      observed.join("; ") || "Not observed"]);
  }
  if (c.input.experiments.some(e => e.observations.some(o => o.extensometers?.length))) {
    const measured = table(parent, "Declared transverse extensometer acquisitions",
      ["Experiment / observation", "Partition", "Channel", "Operator", "Reference station fraction", "Projection axis [across, along]",
        "Observed value (m)", "Residual scale (m)"]);
    for (const e of c.input.experiments) for (const o of e.observations) for (const channel of o.extensometers || []) {
      const operator = channel.operator;
      cells(measured, [`${e.id} / ${o.id}`, e.partition, channel.id,
        operator.kind === "edge_chord" ? "Euclidean edge chord" : "Signed edge projection",
        metric(operator.station_fraction), operator.axis?.map(metric).join(", ") || "Not applicable",
        metric(channel.value_m), metric(channel.scale_m)]);
    }
  }
  const method = document.createElement("p"); method.textContent = c.input.method; parent.append(method);
  if (c.fitted_green_strain_range) {
    const domain = table(parent, "Observed material strain range in FIT element fields",
      ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
    for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
      cells(domain, [axis, ...c.fitted_green_strain_range[i].map(metric)]);
    }
  }
  areaCalibrationResult(parent, c);
  const work = table(parent, "Inverse solve and excitation diagnostics", ["Diagnostic", "Value"]);
  for (const row of [
    ["Identification outcome", c.accepted ? "ACCEPTED" : "NOT ACCEPTED"],
    ["Stationary / attempted starts", `${c.starts.filter(s => s.converged).length} / ${c.starts.length}`],
    ["Normalized excitation pivot", metric(c.excitation_pivot)],
    ["Forward solves / limit", `${metric(c.forward_solves)} / ${metric(c.input.policy.max_forward_solves)}`],
    ["Doubled-mesh change / limit", `${metric(c.mesh_change_peak_scaled)} / ${metric(c.input.policy.max_mesh_scaled)}`],
    ["Enforced-domain replay", c.domain_replay_verified ? "Verified" : "Not verified"]
  ]) cells(work, row);
  const predictions = table(parent, "Actual refined strip predictions and observation residuals",
    ["Experiment / observation", "Partition", "Grip extension", "Force (N)", "Force residual (N)", "Mid-width (m)", "Width residual (m)"]);
  for (const p of c.predictions) cells(predictions, [`${p.experiment} / ${p.observation}`, p.partition,
    metric(p.state.extension_fraction), metric(p.state.force_n), metric(p.force_residual_n),
    metric(p.state.mid_width_m), p.width_residual_m == null ? "Not observed" : metric(p.width_residual_m)]);
  if (c.predictions.some(p => p.extensometers?.length)) {
    const measured = table(parent, "Refined extensometer predictions and residuals",
      ["Experiment / observation", "Partition", "Channel", "Operator", "Reference station fraction", "Projection axis [across, along]",
        "Predicted value (m)", "Residual (m)"]);
    for (const p of c.predictions) for (const channel of p.extensometers || []) {
      const operator = channel.operator;
      cells(measured, [`${p.experiment} / ${p.observation}`, p.partition, channel.id,
        operator.kind === "edge_chord" ? "Euclidean edge chord" : "Signed edge projection",
        metric(operator.station_fraction), operator.axis?.map(metric).join(", ") || "Not applicable",
        metric(channel.predicted_m), metric(channel.residual_m)]);
    }
  }
  if (c.reasons.length) {
    const reasons = document.createElement("p"); reasons.textContent = c.reasons.join(" "); parent.append(reasons);
  }
  const note = document.createElement("p");
  note.textContent = "Each prediction solves the finite strip with clamped ends, free in-plane interior motion, and a prescribed planar quasistatic boundary. " +
    "Grip extension is not treated as uniform fabric strain. Transverse measurements need the declared reference station and chord or signed-projection definition; no source contraction is mapped automatically. " +
    "This existing reversible law cannot supply hysteresis, damage or rate dependence. FIT determines the parameters before CHECK replay. " +
    "Search bounds, force scales and local sensitivity are not confidence intervals or a global uniqueness proof.";
  parent.append(note);
  detail(parent, "Decision, complete source acquisitions, physical grip reactions, deformed widths and residuals", c);
  return { response: nonlinear ? "Nonlinear interval clamped strip" : "Finite clamped strip", status: c.accepted ? "ACCEPTED" : "NOT ACCEPTED",
    held_out: c.check ? "Evaluated; inspect independent residuals and acceptance reasons" : "Not supplied",
    input_sha256: c.input_sha256, material_id: r.selection?.id,
    allow_estimates: r.selection?.allow_estimates,
    mechanical_coefficients_available: r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material?.capabilities.mechanical_coefficients?.refusal,
    next: r.selection ? "Review capabilities and model assumptions, then assign the identified selection and bind the pattern." :
      "Review the retained acquisition and fit diagnostics. No identified selection is available to assign." };
}
function measuredMechanicsResult(parent, r) {
  const c = r.calibration, friction = c.schema === "ftx-friction-calibration-report/1";
  const bending = c.schema === "ftx-bending-calibration-report/1";
  const pair = r.binding?.kind === "interface";
  const response = friction ? ({ pair: "Kinetic friction between two materials", self_contact: "Same-material kinetic friction",
    body: "Kinetic friction against a declared body counterface" })[c.input.target.kind] :
    bending ? "Directional bending and preferred curvature" : "Linear orthotropic stretch and shear";
  const errors = table(parent, friction ? "Observed sliding-force errors — independent acquisitions" :
    bending ? "Observed moment errors — independent partitions" : "Observed force errors — independent partitions",
  ["Partition", "Measured components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], ["Held out", c.check]]) {
    cells(errors, [quality ? name : `${name}: not supplied`, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  if (friction) {
    const fitted = table(parent, "Effective isotropic kinetic coefficient", ["Target", "Coefficient", "Evidence", "Source"]);
    cells(fitted, [response, metric(c.coefficient.value), c.coefficient.evidence, c.coefficient.source]);
    const range = table(parent, "Observed FIT load and speed ranges", ["Observation", "Minimum", "Maximum"]);
    cells(range, ["Normal force (N)", ...c.fit_normal_n_range.map(metric)]);
    cells(range, ["Sliding speed (m/s)", ...c.fit_speed_m_s_range.map(metric)]);
    if (pair) {
      const admission = table(parent, "Exact contact pair and separate material readiness",
        ["Scope", "Source identity", "Coefficients admitted", "Reason"]);
      cells(admission, ["Contact interface", r.binding.interface.materials.join(" / "), r.readiness.ready ? "Yes" : "No",
        r.readiness.refusal?.message || "See the retained interface evidence"]);
      for (const sheet of r.readiness.sheets || []) cells(admission,
        ["Material mechanics", sheet.material_sha256, sheet.ready ? "Yes" : "No", sheet.refusal?.message || "Available"]);
    }
    const scope = document.createElement("p");
    scope.textContent = "These ranges describe the fitting observations; they do not enforce a runtime load or speed domain. " +
      "The fitted law describes constant isotropic kinetic friction, with no static-friction or condition-dependent response inferred. " +
      (pair ? "The pair has its own estimate admission. Its readiness does not establish either sheet's mechanical completeness or a successful garment simulation. " :
        c.input.target.kind === "body" ? "The declared counterface remains in the receipt; a tested surrogate does not establish human-skin behavior. " : "") +
      "Review the original acquisition sources and corrected forces before using the result.";
    parent.append(scope);
    detail(parent, "Exact contact binding, counterfaces and independent readiness", { binding: r.binding, readiness: r.readiness });
  } else {
    const directions = bending ? ["Warp / wale (κuu)", "Weft / course (κvv)", "Twist (2 κuv)"] :
      ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"];
    const matrix = table(parent, bending ? "Directional bending tensor (N·m)" : "Linear membrane stiffness tensor (N/m)",
      ["Response direction", ...directions]);
    for (const [i, row] of (bending ? c.bending_nm : c.stiffness_n_m).entries()) cells(matrix, [directions[i], ...row.map(metric)]);
    if (bending) {
      const preferred = table(parent, c.input.fit_preferred_curvature ? "Identified preferred curvature" : "Preferred curvature used by the fit",
        ["Material direction", "Preferred curvature (1/m)"]);
      for (const [i, direction] of directions.entries()) cells(preferred, [direction, metric(c.preferred_curvature_m_inv[i])]);
    } else {
      const domain = table(parent, "Strain domain from fitting observations", ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
      for (const [i, direction] of directions.entries()) cells(domain, [direction, ...c.fitted_strain_range[i].map(metric)]);
    }
    const excitation = document.createElement("p");
    excitation.textContent = `FIT excitation pivot: ${metric(c.excitation_pivot)}. ` + (bending ?
      "The fitted panel response retains independent seam bending and all unrelated material properties; missing properties remain unavailable." :
      "The fitted strain domain comes from FIT observations. CHECK observations do not widen it.");
    parent.append(excitation);
  }
  const note = document.createElement("p");
  note.textContent = "Scaled residuals use the source request's declared measurement scales, not confidence intervals. " +
    "An absent held-out partition is not a passed check. Fitted coefficients remain estimates; source identity does not authenticate the observations or establish measured garment fit.";
  parent.append(note);
  detail(parent, "Complete source observations, fitted response and independent residuals", c);
  return { response, material_id: r.selection?.id, allow_estimates: pair ? r.binding.interface.allow_estimates : r.selection?.allow_estimates,
    readiness: r.readiness, mechanical_coefficients_available: pair ? undefined : r.material?.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: pair ? undefined : r.material?.capabilities.mechanical_coefficients?.refusal,
    missing_mechanics: c.missing_mechanics, input_sha256: c.input_sha256,
    error_budgets: { max_rms_scaled: c.input.max_rms_scaled, max_peak_scaled: c.input.max_peak_scaled },
    next: pair ? "Assign the exact source materials to the garment, then stage this calibrated contact pair and bind. The inspected material selection is unchanged." :
      "Review capabilities, then assign the identified material selection and bind the pattern." };
}
function calibrationResult(parent, r) {
  if (r.calibration?.schema === "ftx-strip-calibration-report/1") return stripCalibrationResult(parent, r);
  if (r.calibration?.schema === "ftx-membrane-plasticity-calibration-report/1") return permanentStretchResult(parent, r);
  if (["ftx-swatch-calibration-report/1", "ftx-bending-calibration-report/1", "ftx-friction-calibration-report/1"].includes(r.calibration?.schema)) {
    return measuredMechanicsResult(parent, r);
  }
  if (r.comparison) {
    const comparison = r.comparison, decision = comparison.fit_decision;
    const models = table(parent, "Recovery model comparison — FIT choice before held-out checks",
      ["Model", "Declared parameters", "FIT outcome", "FIT RMS", "FIT peak", "FIT equivalent", "Held-out outcome"]);
    for (const candidate of comparison.candidates) {
      const quality = candidate.calibration?.fit;
      cells(models, [candidate.id, metric(candidate.declared_parameters), candidate.fit_status,
        metric(quality?.rms_scaled), metric(quality?.peak_scaled),
        decision.equivalent_candidate_ids.includes(candidate.id) ? "Yes" : "No", candidate.check_status]);
    }
    const explanation = document.createElement("p");
    explanation.textContent = `Comparison outcome: ${comparison.status}. ` +
      `FIT-equivalent alternatives: ${decision.equivalent_candidate_ids.join(", ") || "none"}. ` +
      `Choice before held-out replay: ${decision.chosen_candidate_id || "unresolved"}. ` +
      "Fewer declared parameters is a reported preference and does not resolve equivalent spectra automatically. " +
      "A failed held-out check rejects the same chosen candidate; it never chooses another model. " +
      "Force scales and the RMS equivalence tolerance are not confidence intervals.";
    parent.append(explanation);
    detail(parent, "Alternative spectra, prediction differences, source observations and shared numerical work", comparison);
    if (!r.calibration) return { response: "Directional recovery model comparison", status: comparison.status,
      fit_decision: decision, objective_evaluations: comparison.objective_evaluations,
      projected_iterations: comparison.projected_iterations,
      next: decision.ambiguous ? "Review the retained alternatives and supply select_candidate_id in a new comparison request to choose one explicitly. It must still pass held-out checks." :
        "Review the candidate refusals and acquisition errors. This comparison has no material selection to assign." };
  }
  const c = r.calibration, nonlinear = c.schema === "ftx-nonlinear-swatch-calibration-report/1";
  const timeSearch = c.orthotropic_fit?.time_search;
  const method = (c.input.plastic_equilibrium ? "Fixed permanent stretch with " : "") +
    (nonlinear ? c.material?.membrane_response?.area_response ? "Nonlinear stretch, shear and area response" :
    "Nonlinear stretch and shear" : c.input.orthotropic ?
    timeSearch ? "Directional relaxation with times identified within declared bounds" : "Directional relaxation at the declared times" :
    "Proportional relaxation ratios and times");
  const t = table(parent, "Observed force errors — independent partitions", ["Partition", "Measured components", "Scaled RMS", "Scaled peak"]);
  for (const [name, quality] of [["Fit", c.fit], ["Held out", c.check]]) {
    cells(t, [quality ? name : `${name}: not supplied`, metric(quality?.components), metric(quality?.rms_scaled), metric(quality?.peak_scaled)]);
  }
  const range = nonlinear ? c.fitted_strain_range : c.input.strain_range;
  const domain = table(parent, nonlinear ? "Strain domain from fitting observations" : "Source-declared strain applicability",
    ["Material direction", "Minimum Green strain", "Maximum Green strain"]);
  for (const [i, axis] of ["Warp / wale (E11)", "Weft / course (E22)", "Engineering shear (2 E12)"].entries()) {
    cells(domain, [axis, metric(range?.[i]?.[0]), metric(range?.[i]?.[1])]);
  }
  if (nonlinear) areaCalibrationResult(parent, c);
  if (c.input.plastic_equilibrium) {
    const known = table(parent, "Known permanent-stretch branch at every acquisition node",
      ["Acquisition", "Partition", "Time (s)", "Permanent Green strain [warp, weft, shear]",
        "Accumulated permanent strain", "Known nominal force (N/m)", "Step loss (J/m²)", "Cumulative loss (J/m²)"]);
    for (const p of c.predictions || []) {
      const fixed = p.plastic_equilibrium;
      if (!fixed) continue;
      cells(known, [p.experiment, p.partition, metric(p.time_seconds), fixed.plastic_strain.map(metric).join(", "),
        fixed.accumulated_strain.map(metric).join(", "),
        fixed.nominal_n_m.map(row => row.map(metric).join(", ")).join(" / "),
        metric(fixed.step_dissipated_j_m2), metric(fixed.cumulative_dissipated_j_m2)]);
    }
    const scope = document.createElement("p");
    scope.textContent = "The permanent-stretch coefficients and their evidence are independently supplied and remain fixed. " +
      "Each acquisition starts at its declared pristine reference and advances the plastic state at every recorded node, including nodes without observed force. " +
      "Only the Maxwell recovery branches are identified. Their convolution integrates the declared piecewise-linear Green strain analytically; " +
      "the plastic response is a recorded-node return map, without a continuous-path or refinement claim. " +
      "FIT chooses recovery parameters before CHECK replay. The combined network requires its own source qualification.";
    parent.append(scope);
    detail(parent, "Fixed permanent-stretch observations and combined-network qualification", {
      plastic_equilibrium: c.input.plastic_equilibrium,
      material_id: c.input.material?.id,
      fixed_properties: c.input.material?.properties,
      sources: c.input.material?.sources,
      time_integration: r.time_integration });
  }
  if (c.orthotropic_fit) {
    const recovery = table(parent, timeSearch ? "Directional recovery — published stiffness at identified times" :
      "Directional recovery — published stiffness at supplied times", ["Time (s)", "Retained branch",
      "Warp / wale (N/m)", "Weft / course (N/m)", "Normal coupling (N/m)", "Shear (N/m)"]);
    for (const row of c.orthotropic_fit.candidates) {
      cells(recovery, [metric(row.time_seconds), row.active ? "Active" : "Inactive", ...row.stiffness_n_m.map(metric)]);
    }
    if (timeSearch) {
      const windows = table(parent, "Directional time search — declared windows and local sensitivity",
        ["Minimum time (s)", "Identified time (s)", "Maximum time (s)", "Time sensitivity", "Log-window boundary distance"]);
      for (const [i, row] of c.orthotropic_fit.candidates.entries()) {
        const bounds = c.input.orthotropic.time_bounds_seconds[i];
        cells(windows, [metric(bounds[0]), metric(row.time_seconds), metric(bounds[1]),
          metric(timeSearch.time_sensitivity[i]), metric(timeSearch.boundary_distance_fraction[i])]);
      }
      const diagnostics = table(parent, "Directional time search — local optimizer diagnostics", ["Diagnostic", "Value"]);
      for (const row of [
        ["Converged starts / attempted", `${metric(timeSearch.starts_converged)} / ${metric(timeSearch.starts_attempted)}`],
        ["Iterations in selected solve", metric(timeSearch.iterations)],
        ["Objective evaluations across starts and trials", metric(timeSearch.objective_evaluations)],
        ["Projected tensor iterations across all solves", metric(timeSearch.projected_iterations)],
        ["Maximum scaled gradient", metric(timeSearch.gradient_scaled_max)],
        ["Joint stiffness / time excitation pivot", metric(timeSearch.joint_excitation_pivot)]
      ]) cells(diagnostics, row);
      const interpretation = document.createElement("p");
      interpretation.textContent = "Times were identified within the request's declared windows. Boundary distance is a fraction of the log-time window: 0 is an endpoint and 0.5 is its center. " +
        "Time sensitivity is the scaled force response to log-time changes with fitted tensors held fixed. The gradient measures the remaining objective slope after fitting the tensors; the joint excitation pivot uses column-normalized fit data. " +
        "Review local convergence and excitation alongside the held-out errors; these diagnostics do not establish a global optimum, a unique spectrum or confidence intervals.";
      parent.append(interpretation);
    }
    const scope = document.createElement("p");
    scope.textContent = (timeSearch ?
      "Every identified time must retain an observable recovery branch; individual directional coefficients may still be zero. " :
      "Candidate times are supplied by the request, not optimized from the histories. Inactive candidates remain visible; an entirely inactive spectrum retains only the supplied equilibrium. ") +
      "The table shows the published tensors after any explicit zero threshold. " +
      "The source-declared strain domain is not inferred from small residuals.";
    parent.append(scope);
    detail(parent, "Unconstrained, physically constrained and published candidate tensors (display only)", c.orthotropic_fit);
  }
  const note = document.createElement("p");
  note.textContent = "Scaled errors use the input's declared force scales; they are not confidence intervals. " +
    "An absent held-out partition is not a passed check. Fitted coefficients remain estimates, and coefficient availability does not establish a successful garment simulation or measured fit.";
  parent.append(note);
  if (!nonlinear) detail(parent, "Identified recovery branches and initial-state declaration (display only)",
    c.material.membrane_relaxation || { response: "Supplied equilibrium only; no active recovery branches." });
  detail(parent, "Complete observations, predictions, residuals and identification diagnostics (display only)", c);
  return { response: method, material_id: r.selection.id, allow_estimates: r.selection.allow_estimates,
    mechanical_coefficients_available: r.material.capabilities.mechanical_coefficients?.ready === true,
    mechanics_refusal: r.material.capabilities.mechanical_coefficients?.refusal,
    missing_mechanics: c.missing_mechanics, input_sha256: c.input_sha256,
    error_budgets: { max_rms_scaled: c.input.max_rms_scaled, max_peak_scaled: c.input.max_peak_scaled },
    next: "Review capabilities, then assign the identified selection above and bind the pattern." };
}
function stripHistoryResult(parent, r) {
  const note = document.createElement("p");
  note.textContent = "The prescribed grips load, hold and unload one material specimen. Accepted material history " +
    "persists throughout the program. Stored energy, free-vertex motion and dissipated work remain separate; " +
    "these values do not establish total actuator work, out-of-plane buckling or measured garment behavior.";
  parent.append(note);
  const parallel = r.input?.material?.record?.membrane_relaxation?.parallel_plasticity;
  if (parallel?.model === "parallel_green_maxwell_plastic/1") {
    const network = document.createElement("p");
    network.textContent = "Declared parallel network: the equilibrium branch retains permanent strain, while Maxwell branches retain their own viscous strains. " +
      "Plastic flow uses equilibrium-branch stress; Maxwell arms share the total material strain. " +
      "The report keeps plastic and Maxwell dissipation separate. Matched specimen and garment validation remain independent requirements.";
    parent.append(network);
    detail(parent, "Declared permanent-set and recovery network", {
      declaration: parallel, material_sha256: r.input.material.expected_sha256, model_scope: r.model_scope });
  }
  const states = table(parent, "Loading and recovery history", ["Time (s)", "Grip extension fraction",
    "Moving-grip force (N)", "Mid-gauge width (m)", "Dynamic residual (N)", "Stored energy (J)",
    "Free kinetic energy (J)", "Maxwell dissipation (J)", "Plastic dissipation (J)"]);
  for (const state of r.states) cells(states, [state.time_s, state.extension_fraction, state.force_n,
    state.mid_width_m, state.free_dynamic_residual_n, state.stored_membrane_energy_j,
    state.free_kinetic_energy_j, state.maxwell_dissipation_j, state.plastic_dissipation_j].map(metric));
  detail(parent, "Frozen material, fixture and complete loading program", r.input);
  return { schema: r.schema, input_sha256: r.input_sha256, steps: r.states.length,
    model_scope: r.model_scope,
    next: "Download the original request, complete report and exact frozen replay below." };
}
function stripResult(parent, op, r) {
  if (op === "stripAcquisition") {
    const curves = table(parent, "Original complete force acquisitions", ["Specimen", "Cut direction", "Original rows", "Archive entry", "Source SHA-256"]);
    for (const curve of r.curves) {
      cells(curves, [curve.specimen, curve.direction, metric(curve.observations.length), curve.entry, curve.sha256]);
      detail(parent, `${curve.specimen} / ${curve.direction}: original loading and unloading observations`, curve);
    }
    if (r.transverse_curves?.length) {
      const transverse = table(parent, "Original transverse acquisitions", ["Specimen", "Cut direction", "Original rows", "Archive entry", "Source SHA-256"]);
      for (const curve of r.transverse_curves) {
        cells(transverse, [curve.specimen, curve.direction, metric(curve.observations.length), curve.entry, curve.sha256]);
        detail(parent, `${curve.specimen} / ${curve.direction}: original transverse observations and measurement scope`, curve);
      }
      const note = document.createElement("p");
      note.textContent = "Transverse compression fractions retain the original source channels. Exact spatial sampling and loading/unloading tags are not reported; equivalence to the solver's mid-gauge width is unverified. These values are not converted to widths, local strains or Poisson ratios.";
      parent.append(note);
    }
    if (r.transverse_readme) detail(parent, "Original acquisition README, source identity and fractional units", r.transverse_readme);
    detail(parent, "Source protocol, declared conditions and unverified model assumptions", r.protocol);
    detail(parent, "Original specimen metadata and license", { metadata_json: r.metadata_json, license: r.license });
    return { acquisition_sha256: r.acquisition_sha256, archive_url: r.archive_url, retrieved_on: r.retrieved_on,
      free_gauge_length_m: r.protocol.free_gauge_length_m, reference_width_m: r.protocol.reference_width_m,
      next: "Review the complete source curves and protocol before authoring a separate experiment or calibration request. Grip extension does not establish local membrane strain or transverse deformation." };
  }
  if (op === "stripExperiment") {
    const states = table(parent, "Production-solver grip reactions", ["Grip extension fraction", "Moving-grip force (N)",
      "Fixed-grip force (N)", "Free-force residual (N)", "Grip imbalance (N)", "Mid-gauge width (m)", "Strain energy (J)"]);
    for (const state of r.states) cells(states, [state.extension_fraction, state.force_n, state.fixed_grip_force_n,
      state.free_force_residual_n, state.grip_balance_residual_n, state.mid_width_m, state.energy_j].map(metric));
    detail(parent, "Frozen request, nonuniform element strains and equilibrium positions", r);
    return { input_sha256: r.input_sha256, fixture: r.input.fixture, controls: r.input.controls, model_scope: r.model_scope };
  }
  return stripCalibrationResult(parent, r);
}
function show(op, msg) {
  const r = msg.result;
  if (op === "catalog") {
    $("catalog").replaceChildren();
    for (const record of r.materials) {
      const option = document.createElement("option"); option.value = record.id;
      option.textContent = `${record.name} · ${record.form} · ${record.property_count} observations`;
      $("catalog").append(option);
    }
  } else if (op === "inspect") {
    preparedBy = null; inspectResult(r);
  } else if (op === "garmentcode") {
    preparedImport = r.pattern_json;
    const report = r.report, s = sections.get(op); s.section.hidden = false;
    s.summary.textContent = text({ status: report.status, scope: report.scope,
      source_units_per_meter: report.source_units_per_meter, counts: report.counts,
      source_sha256: report.source_sha256, annotations_sha256: report.annotations_sha256,
      circular_arc_approximations: report.circular_arcs?.curves.length,
      missing_semantics: report.missing_semantics, limitations: report.limitations });
    const note = document.createElement("p");
    note.textContent = "REVIEW_REQUIRED: conversion preserves supported evaluated geometry; it does not establish garment completeness or fit. " +
      "Review the missing construction information, then choose Use imported pattern to replace the current workbench pattern. " +
      (r.fit_spec_json ? "A supplied fit specification is available to download and select in section 3." : "No fit specification was supplied.");
    s.comparison.append(note);
    const panels = table(s.comparison, "Original panels and native physical pieces", ["Source panel", "Native piece", "Winding reversed"]);
    for (const panel of report.panels) cells(panels, [panel.source_panel, panel.piece, panel.winding_reversed ? "Yes" : "No"]);
    if (report.circular_arcs) {
      const arcs = report.circular_arcs, explanation = document.createElement("p");
      explanation.textContent = "Circular edges are imported as polylines under your explicit limits. " + arcs.scope;
      s.comparison.append(explanation);
      const t = table(s.comparison, "Circular-arc approximation: admitted bound / requested limit", [
        "Source panel / edge", "Native edge", "Radius (mm)", "Source arc", "Segments",
        "Distance bound / limit (mm)", "Length error bound / limit (mm)", "Polyline / circular length (mm)",
      ]);
      for (const arc of arcs.curves) cells(t, [
        `${arc.source_panel} / ${arc.source_edge}`, `${arc.piece}/${arc.edge}`, metric(arc.radius_mm),
        `${arc.large_arc ? "Major" : "Minor"}; ${arc.right ? "right" : "left"}`, metric(arc.segments),
        `${metric(arc.max_deviation_bound_mm)} / ${metric(arcs.policy.max_sagitta_mm)}`,
        `${metric(arc.length_error_bound_mm)} / ${metric(arcs.policy.max_length_error_mm)}`,
        `${metric(arc.polyline_length_mm)} / ${metric(arc.arc_length_mm)}`,
      ]);
    }
    detail(s.comparison, "Complete panel, edge, stitch, geometry and source review", report);
    for (const f of msg.files) link(op, s.links, f);
  } else if (op === "outfit") {
    outfit = { request_json: r.request_json, composition_json: r.composition_json };
    installPattern(r.pattern_json, `${r.composition.garments.length} garments in one composed outfit`, r.composition.garments);
    $("selfContact").checked = true;
    const s = sections.get(op); s.section.hidden = false;
    s.summary.textContent = text({ request_sha256: r.request_sha256,
      composed_pattern_sha256: r.composition.composed_pattern_sha256,
      garments: r.composition.garments.map(g => ({ id: g.id, pieces: g.pieces, attached_mass: g.attached_mass })),
      scope: r.scope });
    detail(s.comparison, "Original garment, piece, seam and accessory correspondence", r.composition);
    for (const f of msg.files) link(op, s.links, f);
  } else if (op === "bind") {
    bound = r;
    const pre = document.createElement("pre"); pre.textContent = text({ pattern_sha256: r.pattern_sha256, geometry_changed: r.geometry_changed,
      pieces: r.pieces, material_resolution: { without_self_contact: r.drape_materials_without_self_contact, with_self_contact: r.drape_materials_with_self_contact } });
    $("boundSummary").replaceChildren(pre);
    for (const f of msg.files) link(op, $("bindFiles"), f);
  } else if (op === "stageInterface") {
    ++interfaceRead; interfaceChange = r.interfaces_json; interfaceReady = true;
    $("interfacesFile").value = "";
    $("replaceCalibratedInterface").checked = false;
    edits(); clear(["bind", ...boundOperations]);
    $("interfacesSummary").textContent = `Staged calibrated pair in ${JSON.parse(r.interfaces_json).length} retained contact pair(s). Freeze selections & bind to apply.`;
  } else {
    const pairCalibration = op === "calibrate" && r.binding?.kind === "interface";
    if (op === "treatment") preparedTreatment = typeof r.pattern_json === "string" ? r.pattern_json : null;
    if (pairCalibration) {
      calibratedPair = r.interface_json;
      $("calibratedInterfaceSummary").textContent = `Calibrated pair: ${r.binding.interface.materials.join(" / ")}. Stage this pair after assigning those exact materials.`;
    }
    if (op === "pressure" || (op === "calibrate" && !pairCalibration) || op === "stripFit") {
      clear(["thermalPoint"]);
      ++selectionRead; preparedBy = op;
      if (typeof r.selection_json === "string" && (op !== "stripFit" || r.calibration?.accepted === true)) {
        $("selection").value = r.selection_json;
        inspectResult(op === "pressure" ? r.inspection : r.material, r.selection_json);
      } else {
        $("selection").value = "";
        inspected = null;
        $("detail").textContent = "";
        $("capabilities").replaceChildren();
      }
    }
    const s = sections.get(op); s.section.hidden = false;
    if (op === "cut" || op === "stock") {
      knifeProgramResult(s.comparison, r.program, op === "stock" ? "Finite-stock plan" : "Cutting plan", r.program_refusal);
    }
    const summary = ["screen", "study", "scenario"].includes(op) ? comparison(op, r) :
      op === "calibrate" ? calibrationResult(s.comparison, r) :
      op === "stripHistory" ? stripHistoryResult(s.comparison, r) :
      ["stripAcquisition", "stripExperiment", "stripFit"].includes(op) ? stripResult(s.comparison, op, r) :
      op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis,
        body_source: r.body?.shape_source || "Tape-fitted MODEL_FIT shape", body_fit: r.body?.fit,
        measurement_comparison: r.body?.measurement_comparison } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, maximum_marker_length_m: r.options?.maximum_marker_length_m,
        rolls: r.rolls.map((x, i) => ({ marker_number: i + 1, material: x.material?.authored?.id,
        width_m: x.width_m, ...(x.layout_width ? { layout_width: x.layout_width } : {}),
        length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? procurementResult(s.comparison, r) :
      op === "stock" ? { used_stock_count: r.used_stock_count, used_gross_area_m2: r.used_gross_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, markers: r.markers.map(m => ({ id: m.id, pieces: m.placements.length,
          gross_area_m2: m.gross_area_m2, defect_free_area_m2: m.defect_free_area_m2, utilization: m.utilization,
          stock_mass_kg: m.stock_mass_kg, mass_unavailable: m.mass_unavailable })), limitations: r.limitations } :
      op === "sew" ? sewingResult(s.comparison, r) :
      op === "treatment" ? treatmentResult(s.comparison, r) :
      op === "vapor" ? vaporResult(s.comparison, r) :
      op === "thermalPoint" ? thermalPointResult(s.comparison, r) :
      op === "thermalHistory" ? thermalHistoryResult(s.comparison, r) :
      op === "vaporSurface" ? vaporSurfaceResult(s.comparison, r) :
      op === "heat" || op === "coverage" ? coverageResult(s.comparison, r) :
      op === "pressure" && ["ftx-compressed-thermal-history-report/1", "ftx-compressed-thermal-history-report/2", "ftx-compressed-thermal-history-report/3"].includes(r.schema) ? compressionHistoryResult(s.comparison, r) :
      op === "pressure" && r.schema === "ftx-compression-stack-report/1" ? {
        selected_layer: r.chosen_layer_id, pressure_pa: r.pressure_pa,
        common_pressure_range_pa: r.common_pressure_range_pa, requested_gap_m: r.input.gap_m,
        thickness_m: r.thickness_m, closure_residual_m: r.closure_residual_m,
        normal_force_n: r.normal_force_n, external_work_j: r.external_work_j,
        reference_mass_kg: r.reference_mass_kg, material_resistance_m2_k_w: r.material_resistance_m2_k_w,
        layers: r.layers.map(l => ({ id: l.id, thickness_m: l.thickness_m, closure_m: l.closure_m,
          external_work_j_m2: l.external_work_j_m2, unavailable: l.unavailable,
          discarded_properties: l.conditioned.discarded_properties })),
        no_claim: r.no_claim, next: "Assign the explicitly selected layer above. Change the layer ID and prepare again to select another layer." } :
      op === "pressure" ? { material_id: r.selection.id, pressure_pa: r.input.pressure_pa,
        pressure_range_pa: r.pressure_range_pa, bracket_pa: r.bracket_pa, interpolated: r.interpolated,
        discarded_properties: r.discarded_properties, discarded_responses: r.discarded_responses,
        discarded_structure: r.discarded_structure,
        next: "Review the prepared selection's capabilities, then assign it to the pattern above." } : r;
    s.summary.textContent = text(summary);
    for (const f of msg.files) {
      const url = link(op, s.links, f);
      if (op === "fold" && f.mime === "text/html") { $("viewer").src = url; $("viewer").hidden = false; }
      if (op === "cut" && f.mime === "image/svg+xml") { $("marker").src = url; $("marker").hidden = false; }
      if (op === "stock" && f.mime === "image/svg+xml") { $("stockMarker").src = url; $("stockMarker").hidden = false; }
      if (["screen", "study", "scenario"].includes(op) && ["screen-request.json", "study-request.json"].includes(f.name)) {
        detail(s.comparison, `${f.name}: declared objectives, evidence and process policy (display only)`, JSON.parse(f.text));
      }
    }
    // The parsed result is for display only: JSON.stringify changes 1.0/-0.0 and breaks locks.
    if (op === "fold") drape = msg.reportText;
  }
}
async function read(id, binary = false) {
  if (!binary && roundFiles.has(id)) return roundFiles.get(id);
  const f = binary && roundFiles.has(id) ? roundFiles.get(id) : $(id).files[0];
  if (!f) throw Error(`Choose ${id} first.`);
  if (id === "bodyFitFile") {
    if (f.size > 16 * 1024 * 1024) throw Error("Accepted body-fit report exceeds 16 MiB.");
    const bytes = await f.arrayBuffer();
    if (bytes.byteLength > 16 * 1024 * 1024) throw Error("Accepted body-fit report exceeds 16 MiB.");
    const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
    JSON.parse(raw);
    return raw;
  }
  if (f.size > (binary ? 256 : 64) * 1024 * 1024) throw Error(`${id} exceeds its input budget.`);
  const data = binary ? await f.arrayBuffer() : await f.text();
  if (binary && data.byteLength > 256 * 1024 * 1024) throw Error(`${id} exceeds its input budget.`);
  if (!binary) JSON.parse(data);
  return data;
}
function acceptedBodyAngle() {
  const angle = $("bodyArmDeg").value.trim();
  if (!angle || !Number.isFinite(Number(angle)) || Number(angle) < 0 || Number(angle) > 180) {
    throw Error("Supply a finite drape arm angle from 0 to 180 degrees.");
  }
  return Number(angle);
}
async function studyBodyInput() {
  const mode = studyBodyMode();
  if (mode === "measurements") return read("measurements");
  if (mode === "request") return read("scenarioBody");
  if (mode !== "accepted") throw Error("Choose a supported study body source.");
  if (!has("bodyFitFile")) throw Error("Choose an accepted body-fit report for the selected study body source.");
  if (!$("bodyFitConsent").checked) throw Error("Associate the accepted fit with this wearer before running body-dependent studies.");
  const angle = acceptedBodyAngle(), fit = await read("bodyFitFile");
  const measurements = has("measurements") ? await read("measurements") : "{}";
  // The existing Rust request is the authority. Embed validated source text without rounding its numbers.
  return '{"schema":"ftx-scenario-body/1","measurements":' + measurements +
    ',"accepted_body_fit":' + fit + ',"arm_abduction_deg":' + angle + '}';
}
async function readGarmentcode(id) {
  const f = $(id).files[0]; if (!f) throw Error(`Choose ${id} first.`);
  if (f.size > 4 * 1024 * 1024) throw Error("Each GarmentCode input is limited to 4 MiB.");
  const bytes = await f.arrayBuffer();
  if (bytes.byteLength > 4 * 1024 * 1024) throw Error("Each GarmentCode input is limited to 4 MiB.");
  let raw;
  try { raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { throw Error(`${id} must be valid UTF-8; no replacement characters were substituted.`); }
  // Preserve every valid source byte. Retaining a BOM also preserves the native JSON refusal.
  JSON.parse(raw);
  return raw;
}
function circularArcAnnotations() {
  if (!$("garmentcodeArcs").checked) return undefined;
  const positive = (id, label) => {
    const raw = $(id).value.trim(), value = Number(raw);
    if (!raw || !Number.isFinite(value) || value <= 0) throw Error(`Supply a positive finite ${label} in millimetres.`);
    return value;
  };
  return text({ schema: "fabric-garmentcode-annotations/1", circular_arcs: {
    model: "bounded_chord_polylines/1",
    max_sagitta_mm: positive("garmentcodeArcSagitta", "circular-arc distance tolerance"),
    max_length_error_mm: positive("garmentcodeArcLength", "circular-edge length tolerance"),
  } }) + "\n";
}
function changes() {
  const pieces = [...pieceChanges].map(([id, selection]) => `${JSON.stringify(id)}:${selection}`).join(",");
  const interfaces = interfaceChange === null ? "" : `,"interfaces":${interfaceChange}`;
  return `{"schema":"ftx-material-bindings/1","default":${defaultChange || "null"},"pieces":{${pieces}}${interfaces}}`;
}
function edits() { $("edits").textContent = text(JSON.parse(changes())); }
function showCombinations() {
  const parent = $("combinationGroups"); parent.replaceChildren();
  let count = combinationGroups.size ? 1 : 0;
  for (const [key, group] of [...combinationGroups].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    count *= group.choices.size;
    const heading = document.createElement("p");
    heading.textContent = `${key} — ${group.target.scope === "default" ? "Pattern default" : group.target.ids.join(", ")}`;
    parent.append(heading);
    for (const [choice, selection] of group.choices) {
      const row = document.createElement("div"), label = document.createElement("span"), remove = document.createElement("button");
      label.textContent = `${choice}: ${JSON.parse(selection).id} `; remove.textContent = `Remove ${choice}`;
      remove.onclick = () => {
        group.choices.delete(choice);
        if (!group.choices.size) combinationGroups.delete(key);
        showCombinations(); invalidate(["study", "scenario"]);
      };
      row.append(label, remove); parent.append(row);
    }
  }
  $("combinationCount").textContent = count ? `${count} complete material combination${count === 1 ? "" : "s"}; limit 32.` : "No material choices added.";
}
async function studyInput() {
  const raw = await read("studyRequest");
  if (!$("combinationMode").checked) return raw;
  const policy = JSON.parse(raw);
  if (!policy || typeof policy !== "object" || Array.isArray(policy)
    || !["ftx-material-study/1", "ftx-design-study/1"].includes(policy.schema)) {
    throw Error("Combination mode needs an ftx-material-study/1 or ftx-design-study/1 policy object.");
  }
  if (["variants", "screened", "combinations"].some(key => Object.hasOwn(policy, key))) {
    throw Error("In combination mode, the study policy must omit variants, screened and combinations. Turn off combination mode to use a complete study file.");
  }
  if (!combinationGroups.size) throw Error("Add material choices before running combinations.");
  const groups = [...combinationGroups].map(([key, group]) => {
    const choices = [...group.choices].map(([choice, selection]) => `{"key":${JSON.stringify(choice)},"selection":${selection}}`);
    return `{"key":${JSON.stringify(key)},"target":${JSON.stringify(group.target)},"choices":[${choices.join(",")}]}`;
  });
  let interfaces = "";
  if (has("combinationInterfaces")) {
    const pairs = await read("combinationInterfaces");
    if (!Array.isArray(JSON.parse(pairs))) throw Error("The combination interface pool must be a JSON list.");
    interfaces = `,"interfaces":${pairs}`;
  }
  // Insert authored fields without reserializing source-bound quantities, policies or selections.
  return `${raw.trim().slice(0, -1)},"combinations":{"groups":[${groups.join(",")}]${interfaces}}}`;
}
function resetInterfaces() {
  ++interfaceRead; interfaceChange = null; interfaceReady = true;
  $("interfacesFile").value = "";
  $("interfacesSummary").textContent = "Keep the original pattern's interface evidence.";
}
async function execute(op) {
  if (!ready || pending) return;
  if (op === "bind") clear(["bind", ...boundOperations]);
  else if (op === "fold") clear(["fold", "heat", "vaporSurface", "coverage"]);
  else if (sections.has(op)) clear([op]);
  if (selectionOperations.includes(op) && !(op === "calibrate" && $("calibrationMode").value === "friction_pair")) {
    ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  }
  const id = ++sequence; pending = { id, op }; $("error").hidden = true;
  $("status").textContent = `Running ${op}…`; controls();
  try {
    const msg = { type: op, request: id };
    if (op === "catalog") msg.query = $("query").value;
    else if (op === "inspect") msg.selection = $("selection").value;
    else if (op === "thermalPoint") {
      if (!$("thermalPointTemperature").value.trim() || !Number.isFinite(Number($("thermalPointTemperature").value))) throw Error("Supply a finite material temperature in degrees Celsius.");
      msg.selection = $("selection").value; msg.temperatureC = Number($("thermalPointTemperature").value);
    }
    else if (op === "thermalHistory") msg.thermalRequest = await read("thermalRequest");
    else if (op === "garmentcode") {
      msg.source = await readGarmentcode("garmentcodeSource");
      if (has("garmentcodeAnnotations")) msg.annotations = await readGarmentcode("garmentcodeAnnotations");
      else msg.annotations = circularArcAnnotations();
    }
    else if (op === "bind") {
      if (!interfaceReady) throw Error("Load a valid replacement interface list or choose Keep original interfaces.");
      msg.pattern = sourceText; msg.changes = changes();
    }
    else if (op === "stageInterface") {
      if (!sourceText || !interfaceReady || !calibratedPair) throw Error("Load the garment, retain valid interface edits and identify a current contact pair first.");
      msg.pattern = sourceText; msg.changes = changes();
      msg.interfaceJson = calibratedPair; msg.replaceExisting = $("replaceCalibratedInterface").checked;
    }
    else if (op === "pressure") {
      msg.compressionLayer = $("compressionLayer").value;
      msg.compressionRequest = await read("compressionRequest");
    }
    else if (op === "treatment") {
      msg.pattern = bound?.pattern_json || sourceText;
      msg.treatmentRequest = await read("treatmentRequest");
    }
    else if (op === "vapor") msg.vaporRequest = await read("vaporRequest");
    else if (op === "calibrate") {
      msg.calibrationMode = $("calibrationMode").value;
      msg.allowEstimates = $("calibrationEstimates").checked;
      msg.calibrationRequest = await read("calibrationRequest");
    }
    else if (["stripAcquisition", "stripExperiment", "stripHistory", "stripFit"].includes(op)) msg.stripRequest = await read(`${op}Request`);
    else if (op === "screen") msg.screenRequest = await read("screenRequest");
    else if (op === "study" || op === "scenario") {
      if (!sourceText) throw Error("Load the original pattern first.");
      // Alternatives always start at the original, never the currently bound single-material preview.
      msg.pattern = sourceText; msg.studyRequest = await studyInput();
      if (op === "study") msg.onBody = $("studyOnBody").checked;
      if (op === "scenario" || msg.onBody) {
        if (op === "scenario") msg.scenarioRequest = await read("scenarioRequest");
        msg.measurements = await studyBodyInput();
        msg.bytes = await read("body", true);
      }
    }
    else {
      if (!bound) throw Error("Bind the current materials first.");
      msg.pattern = bound.pattern_json;
      if (op === "fold" || op === "exports") { msg.measurements = await read("measurements"); msg.fit = await read("fit"); }
      if (op === "fold") {
        if (has("bodyFitFile")) {
          if (!$("bodyFitConsent").checked) throw Error("Associate the accepted fit with this wearer before draping.");
          const angle = acceptedBodyAngle();
          msg.bodyFit = await read("bodyFitFile"); msg.bodyFitConsent = true; msg.armDeg = angle;
        }
        msg.bytes = await read("body", true); msg.selfContactMm = $("selfContact").checked ? 2 : 0;
      }
      if (op === "exports") { const d = new Date(), pad = n => String(n).padStart(2, "0");
        msg.today = `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())}`;
        msg.created = `${msg.today}T${pad(d.getHours())}:${pad(d.getMinutes())}`; }
      if (op === "heat" || op === "vaporSurface" || op === "coverage") {
        if (!drape) throw Error("Re-drape the current material first."); msg.report = drape;
        msg.conditions = await read(op === "heat" ? "heatConditions" : op === "vaporSurface" ? "vaporSurfaceConditions" : "coverageConditions");
        if (op === "coverage") {
          msg.map = await read("map");
          if (has("coverageBridges")) msg.thermalBridges = await read("coverageBridges");
        }
      }
      if (op === "cut" || op === "price") msg.options = await read("cutOptions");
      if (op === "cut") {
        const marker = Number($("rollPreview").value || "1");
        if (!Number.isSafeInteger(marker) || marker < 1) throw Error("Preview marker number must be a positive integer.");
        msg.rollIndex = marker - 1;
      }
      if (op === "stock") msg.stockRequest = await read("stockRequest");
      if (op === "price") { msg.quotes = await read("quotes"); msg.market = await read("market"); }
      if (op === "sew") msg.options = await read("sewOptions");
    }
    if (outfit && (op === "bind" || boundOperations.includes(op) || op === "study" || op === "scenario")) msg.outfit = outfit;
    if (pending?.id !== id) return;
    worker.postMessage(msg, msg.bytes ? [msg.bytes] : []);
  } catch (e) {
    if (pending?.id !== id) return;
    pending = null; error(String(e.message || e)); $("status").textContent = `${op} refused.`; controls();
  }
}
for (const el of commands) el.onclick = () => execute(el.dataset.command);
$("rollPreview").addEventListener("change", () => invalidate(["cut"]));
$("cancel").onclick = () => { start(); $("status").textContent = "Cancelled; reloading local engine."; };
$("catalog").onchange = () => {
  if (!$("catalog").value) return;
  invalidateThermalPoint();
  ++selectionRead;
  if (selectionOperations.includes(pending?.op)) start();
  $("selection").value = text({ id: $("catalog").value, allow_estimates: false });
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); controls();
};
$("selection").addEventListener("input", () => {
  invalidateThermalPoint();
  ++selectionRead;
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
});
$("selectionFile").onchange = async () => {
  invalidateThermalPoint();
  const rev = ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
  try { const data = await read("selectionFile"); if (rev !== selectionRead) return;
    invalidateThermalPoint(); $("selection").value = data; controls(); }
  catch (e) { if (rev === selectionRead) error(String(e.message || e)); }
};
function parsePattern(raw) {
  const data = JSON.parse(raw);
  if (!Array.isArray(data?.pieces) || data.pieces.length > 4096) throw Error("Pattern needs at most 4096 source pieces.");
  return data;
}
function installPattern(raw, label, garments = [], data = parsePattern(raw)) {
  source = data; sourceText = raw;
  $("patternName").textContent = `${label}: ${data.pieces.length} source pieces. Original retained.`;
  const group = (key, label, ids) => {
    outfitTargets.set(key, ids);
    const option = document.createElement("option"); option.value = key; option.textContent = label;
    $("target").append(option);
  };
  if (garments.length) {
    group("@outfit", "Every source piece in this outfit", data.pieces.map(p => p.id));
    garments.forEach((garment, i) => group(`@garment:${i}`, `${garment.id} — every source piece`, Object.values(garment.pieces)));
  }
  for (const p of data.pieces) {
    for (const target of ["target", "combinationPieces"]) {
      const o = document.createElement("option"); o.value = p.id; o.textContent = p.id; $(target).append(o);
    }
  }
}
function resetPatternInput(keepImport = false) {
  ++patternRead; source = null; sourceText = null; outfit = null; adoptedImport = false;
  $("bodyFitConsent").checked = false;
  defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits();
  outfitTargets.clear();
  combinationGroups.clear(); showCombinations(); $("combinationPieces").replaceChildren();
  invalidate(["bind", ...boundOperations, "study", "scenario", "outfit", ...(keepImport ? [] : ["garmentcode"])]);
  $("patternName").textContent = "No pattern loaded.";
  $("target").replaceChildren(); const base = document.createElement("option"); base.value = "";
  base.textContent = "Pattern default (preserve explicit piece choices)"; $("target").append(base);
  return patternRead;
}
$("patternFile").onchange = async () => {
  clearRoundSources();
  const rev = resetPatternInput();
  try { const raw = await read("patternFile"), data = JSON.parse(raw); if (rev !== patternRead) return;
    if (data.schema === "ftx-outfit-drape/1") {
      const id = ++sequence; pending = { id, op: "outfit" };
      $("status").textContent = "Preparing source-qualified garments and contact pairs…";
      controls(); worker.postMessage({ type: "outfit", request: id, outfitRequest: raw });
      return;
    }
    installPattern(raw, $("patternFile").files[0].name);
    controls();
  } catch (e) { if (rev === patternRead) { source = null; error(String(e.message || e)); controls(); } }
};
$("assign").onclick = () => {
  if (!inspected || !source || pending) return;
  const target = $("target").value;
  if (target === "") defaultChange = inspected;
  else if (outfitTargets.has(target)) for (const id of outfitTargets.get(target)) pieceChanges.set(id, inspected);
  else pieceChanges.set(target, inspected);
  edits(); invalidate(["bind", ...boundOperations]);
};
$("reset").onclick = () => { defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits(); invalidate(["bind", ...boundOperations]); };
$("useTreatment").onclick = () => {
  if (!preparedTreatment || pending) return;
  const raw = preparedTreatment;
  resetPatternInput();
  installPattern(raw, "Derived treatment pattern");
  $("status").textContent = "Derived pattern loaded. Freeze selections and bind to recompute its consumers.";
  controls();
};
$("useGarmentcode").onclick = () => {
  if (!preparedImport || pending) return;
  const raw = preparedImport;
  clearRoundSources();
  resetPatternInput(true); preparedImport = null;
  $("patternFile").value = "";
  installPattern(raw, "Imported GarmentCode pattern — REVIEW_REQUIRED");
  adoptedImport = true;
  $("status").textContent = "Imported pattern loaded for review. Assign materials and freeze selections to recompute its consumers. Conversion does not establish fit.";
  controls();
};
function invalidateGarmentcode() {
  if (adoptedImport) {
    resetPatternInput();
    $("patternName").textContent = "Imported pattern withdrawn. Convert the changed inputs and review the new result before adopting it.";
  } else invalidate(["garmentcode"]);
}
for (const id of ["garmentcodeSource", "garmentcodeAnnotations"]) $(id).addEventListener("change", invalidateGarmentcode);
$("garmentcodeArcs").addEventListener("change", invalidateGarmentcode);
for (const id of ["garmentcodeArcSagitta", "garmentcodeArcLength"]) $(id).addEventListener("input", invalidateGarmentcode);
$("clearGarmentcodeAnnotations").onclick = () => { $("garmentcodeAnnotations").value = ""; invalidateGarmentcode(); };
$("addCombination").onclick = () => {
  if (!inspected || !source || pending) return;
  try {
    const key = $("combinationGroup").value.trim(), choice = $("combinationChoice").value.trim();
    if (!key || !choice) throw Error("Name both the material group and its choice.");
    const target = $("combinationScope").value === "default" ? { scope: "default" } :
      { scope: "pieces", ids: [...$("combinationPieces").children].filter(o => o.selected).map(o => o.value).sort() };
    if (target.scope === "pieces" && !target.ids.length) throw Error("Select source pieces for this group.");
    const old = combinationGroups.get(key);
    if (!old && combinationGroups.size >= 8) throw Error("Use at most eight material choice groups.");
    if (old && JSON.stringify(old.target) !== JSON.stringify(target)) throw Error("A group's choices must share the same target. Remove that group's choices before changing its pieces.");
    for (const [other, group] of combinationGroups) {
      if (other === key) continue;
      if (target.scope === "default" && group.target.scope === "default") throw Error("Only one group may change the pattern default.");
      if (target.scope === "pieces" && group.target.scope === "pieces" && target.ids.some(id => group.target.ids.includes(id))) {
        throw Error("Source-piece groups must be disjoint.");
      }
    }
    const choices = new Map(old?.choices || []); choices.set(choice, inspected);
    let count = choices.size;
    for (const [other, group] of combinationGroups) if (other !== key) count *= group.choices.size;
    if (count > 32) throw Error("These choices exceed the limit of 32 complete combinations. Remove a choice first.");
    combinationGroups.set(key, { target, choices }); showCombinations(); invalidate(["study", "scenario"]);
  } catch (e) { error(String(e.message || e)); controls(); }
};
$("clearCombinations").onclick = () => { combinationGroups.clear(); showCombinations(); invalidate(["study", "scenario"]); };
$("combinationMode").addEventListener("change", () => invalidate(["study", "scenario"]));
$("combinationInterfaces").addEventListener("change", () => invalidate(["study", "scenario"]));
$("clearInterfaces").onclick = () => { resetInterfaces(); edits(); invalidate(["bind", ...boundOperations]); };
$("interfacesFile").onchange = async () => {
  const rev = ++interfaceRead; interfaceChange = null; interfaceReady = false;
  edits(); invalidate(["bind", ...boundOperations]);
  $("interfacesSummary").textContent = "Reading replacement interface evidence…";
  if (!has("interfacesFile")) { resetInterfaces(); controls(); return; }
  try {
    const raw = await read("interfacesFile"); if (rev !== interfaceRead) return;
    const pairs = JSON.parse(raw);
    if (!Array.isArray(pairs)) throw Error("Replacement interfaces must be a JSON list; [] explicitly removes all original pairs.");
    // Keep the authored numerical JSON. Rust validates evidence and exact final material identities.
    interfaceChange = raw; interfaceReady = true;
    $("interfacesSummary").textContent = `Replace the complete interface table with ${pairs.length} supplied pair(s). Freeze checks their sources and assigned material identities.`;
    edits(); controls();
  } catch (e) {
    if (rev !== interfaceRead) return;
    $("interfacesSummary").textContent = "Replacement evidence is unavailable; binding is disabled until corrected or cleared.";
    error(String(e.message || e)); controls();
  }
};
for (const [id, operation] of [["compressionRequest", "pressure"], ["compressionLayer", "pressure"], ["calibrationRequest", "calibrate"],
  ["calibrationMode", "calibrate"], ["calibrationEstimates", "calibrate"], ["stripFitRequest", "stripFit"]]) {
  $(id).addEventListener(id === "compressionLayer" ? "input" : "change", () => {
    if (preparedBy === operation) {
      ++selectionRead; preparedBy = null; inspected = null;
      $("detail").textContent = ""; $("capabilities").replaceChildren();
    }
    invalidate([operation]);
  });
}
for (const [id, keys] of Object.entries({ measurements:["exports","fold","heat","vaporSurface","coverage"], fit:["exports","fold","heat","vaporSurface","coverage"],
  body:["fold","heat","vaporSurface","coverage"], selfContact:["fold","heat","vaporSurface","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  stockRequest:["stock"], sewOptions:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"], vaporRequest:["vapor"],
  stripAcquisitionRequest:["stripAcquisition"], stripExperimentRequest:["stripExperiment"],
  stripHistoryRequest:["stripHistory"], vaporSurfaceConditions:["vaporSurface"],
  treatmentRequest:["treatment"], screenRequest:["screen"], studyRequest:["study","scenario"], scenarioRequest:["scenario"], scenarioBody:[] })) {
  $(id).addEventListener("change", () => {
    if (id === "measurements" || id === "body") $("bodyFitConsent").checked = false;
    if (id === "measurements" || id === "fit" || id === "body") {
      ++roundRead; roundToken = null; window.opener = null; roundFiles.delete(id);
      if (id === "body") showReceivedBody();
    }
    invalidate([...keys, ...bodyDependentStudies(id)], ["measurements", "fit", "body", "selfContact", "scenarioBody"].includes(id));
  });
}
$("bodyFitFile").addEventListener("change", () => {
  ++roundRead; roundToken = null; window.opener = null; roundFiles.delete("bodyFitFile");
  showReceivedBody();
  $("bodyFitConsent").checked = false;
  invalidate(["fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies("bodyFitFile")], true);
});
for (const id of ["bodyFitConsent", "bodyArmDeg"]) {
  $(id).addEventListener(id === "bodyArmDeg" ? "input" : "change", () => {
    ++roundRead; roundToken = null; window.opener = null;
    invalidate(["fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies(id)], true);
  });
}
$("clearBodyFit").onclick = () => {
  ++roundRead; roundToken = null; window.opener = null;
  $("bodyFitFile").value = ""; roundFiles.delete("bodyFitFile"); $("bodyFitConsent").checked = false;
  showReceivedBody();
  invalidate(["fold", "heat", "vaporSurface", "coverage", ...bodyDependentStudies("bodyFitFile")], true);
};
$("studyOnBody").addEventListener("change", () => invalidate(["study"], true));
$("studyBodySource").addEventListener("change", () => invalidate(["scenario", ...($("studyOnBody").checked ? ["study"] : [])], true));
$("thermalPointTemperature").addEventListener("input", invalidateThermalPoint);
$("thermalRequest").addEventListener("change", () => invalidate(["thermalHistory"]));
$("clearCoverageBridges").onclick = () => { $("coverageBridges").value = ""; invalidate(["coverage"]); };
$("coverageBridges").addEventListener("change", () => invalidate(["coverage"]));
function showReceivedBody() {
  clear(["roundBody"]); $("receivedBodyFiles").replaceChildren();
  const model = roundFiles.get("body"), fit = roundFiles.get("bodyFitFile");
  $("receivedBodyNote").hidden = !model && !fit;
  $("receivedBodyNote").textContent = "Received body sources remain local immutable snapshots. Manual file choices replace them. " +
    (fit ? "Review the original fit, confirm this wearer, and choose the exact model before draping. The transferred angle selects the ordinary drape pose." :
      "The received model will be fitted to the current measurements when you choose Re-drape.");
  if (model) link("roundBody", $("receivedBodyFiles"), { name: "body-model.ftxbody", bytes: model, mime: "application/octet-stream" });
  if (fit) link("roundBody", $("receivedBodyFiles"), { name: "body_fit.json", text: fit, mime: "application/json" });
}
function clearRoundSources() {
  ++roundRead; roundToken = null; roundFiles.clear(); window.opener = null;
  $("bodyFitConsent").checked = false; showReceivedBody();
  $("roundImportNote").hidden = true;
}
$("clearRoundImport").onclick = () => { clearRoundSources(); resetPatternInput(); controls(); };
const roundOrigin = new URL(window.location.href).origin;
if (roundToken && /^[0-9a-f]{32}$/.test(roundToken) && roundOrigin !== "null" && window.opener) {
  const sender = window.opener;
  $("roundImportNote").hidden = false;
  $("roundImportText").textContent = "Waiting for the current flat round. Material binding and every computation require your action.";
  window.addEventListener("message", async event => {
    const m = event.data;
    if (!roundToken || event.origin !== roundOrigin || event.source !== sender || m?.token !== roundToken ||
      !["ftx-round-sources", "ftx-round-sources-cancelled"].includes(m.type)) return;
    roundToken = null; window.opener = null;
    const rev = ++roundRead;
    resetPatternInput();
    try {
      if (m.type === "ftx-round-sources-cancelled") throw Error("The source round changed before transfer. Draft and check it again.");
      const files = new Map();
      for (const [key, id] of [["pattern", "patternFile"], ["measurements", "measurements"], ["fit", "fit"]]) {
        const blob = m.files?.[key];
        if (!(blob instanceof Blob) || blob.size > 64 * 1024 * 1024) throw Error("Each of the three round files must be JSON within 64 MiB.");
        const bytes = await blob.arrayBuffer();
        if (rev !== roundRead) return;
        if (bytes.byteLength > 64 * 1024 * 1024) throw Error("A round file exceeds 64 MiB.");
        const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
        JSON.parse(raw); files.set(id, raw);
      }
      const context = m.bodySource;
      if (context != null) {
        if (typeof context !== "object" || Array.isArray(context) || (!context.model && !context.fit)) throw Error("Body context needs a model or an accepted-fit source.");
        if (context.model != null) {
          if (!(context.model instanceof Blob) || !context.model.size || context.model.size > 256 * 1024 * 1024) throw Error("Received body model must be a nonempty file within 256 MiB.");
          files.set("body", context.model);
        }
        if (context.fit != null) {
          if (!(context.fit instanceof Blob) || context.fit.size > 16 * 1024 * 1024) throw Error("Received accepted fit exceeds its 16 MiB input budget.");
          if (!Number.isFinite(context.armDeg) || context.armDeg < 0 || context.armDeg > 180) throw Error("Received drape arm angle must be from 0 to 180 degrees.");
          const bytes = await context.fit.arrayBuffer();
          if (rev !== roundRead) return;
          if (bytes.byteLength > 16 * 1024 * 1024) throw Error("Received accepted fit exceeds its 16 MiB input budget.");
          const raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
          const parsed = JSON.parse(raw);
          if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw Error("Received accepted fit must be a JSON object.");
          files.set("bodyFitFile", raw);
        } else if (context.armDeg != null) throw Error("A received drape pose requires an accepted-fit source.");
      }
      installPattern(files.get("patternFile"), "Received round pattern.json");
      for (const [id, raw] of files) roundFiles.set(id, raw);
      if (context?.fit) $("bodyArmDeg").value = String(context.armDeg);
      $("bodyFitConsent").checked = false; showReceivedBody();
      $("roundImportText").textContent = "Received pattern.json, measurements.json and fit_spec.json. Originals retained in this tab. Review sources, then Freeze selections and bind before running a consumer. " +
        (context ? "Selected body sources also arrived; review them and confirm the accepted-fit association separately before draping. " : "Choose a body separately for draping. ") +
        "Manual file choices replace received inputs.";
      controls();
    } catch (e) {
      if (rev !== roundRead) return;
      roundFiles.clear(); showReceivedBody(); resetPatternInput();
      $("roundImportText").textContent = `Round handoff refused: ${e.message || e} Choose the files manually or return to the current round.`;
      controls();
    }
  });
  sender.postMessage({ type: "ftx-round-sources-ready", token: roundToken }, roundOrigin);
} else roundToken = null;
window.addEventListener("pagehide", () => { clearRoundSources(); ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();

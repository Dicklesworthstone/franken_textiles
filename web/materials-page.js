// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const boundOperations = ["exports", "fold", "heat", "coverage", "cut", "stock", "price", "sew"];
const operations = [...boundOperations, "screen", "study", "scenario"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let sourceText = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
const pieceChanges = new Map(), urls = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = ({ screen: "Source-qualified screening",
    study: "Fixed-pattern manufacturing alternatives", scenario: "Fresh drape and heat alternatives" })[op] || op;
  const summary = document.createElement("pre"), links = document.createElement("div"); links.className = "links";
  const comparison = document.createElement("div"); comparison.className = "comparison";
  section.append(title, summary, comparison, links); $("outputs").append(section);
  sections.set(op, { section, summary, comparison, links });
}
const text = value => JSON.stringify(value, null, 2);
const has = id => !!$(id).files.length;
function controls() {
  for (const el of commands) el.disabled = !ready || !!pending;
  $("bind").disabled ||= !source;
  $("assign").disabled = !!pending || !source || !inspected;
  $("reset").disabled = !!pending || !source;
  for (const op of boundOperations) $(op).disabled ||= !bound;
  $("screen").disabled ||= !has("screenRequest");
  $("study").disabled ||= !source || !has("studyRequest");
  $("scenario").disabled ||= !source || !has("studyRequest") || !has("scenarioRequest") || !has("body") || (!has("scenarioBody") && !has("measurements"));
  $("exports").disabled ||= !has("measurements") || !has("fit");
  $("fold").disabled ||= !has("measurements") || !has("fit") || !has("body");
  $("cut").disabled ||= !has("cutOptions");
  $("stock").disabled ||= !has("stockRequest");
  $("price").disabled ||= !has("cutOptions") || !has("quotes") || !has("market");
  $("sew").disabled ||= !has("sewOptions");
  $("heat").disabled ||= !drape || !has("heatConditions");
  $("coverage").disabled ||= !drape || !has("map") || !has("coverageConditions");
  $("cancel").disabled = !pending;
}
function clear(keys) {
  for (const key of keys) {
    for (const url of urls.get(key) || []) URL.revokeObjectURL(url);
    urls.delete(key);
    if (sections.has(key)) { const s = sections.get(key); s.section.hidden = true; s.links.replaceChildren(); s.comparison.replaceChildren(); s.summary.textContent = ""; }
    if (key === "fold") { drape = null; $("viewer").hidden = true; $("viewer").removeAttribute("src"); }
    if (key === "cut") { $("marker").hidden = true; $("marker").removeAttribute("src"); }
    if (key === "stock") { $("stockMarker").hidden = true; $("stockMarker").removeAttribute("src"); }
    if (key === "bind") { bound = null; $("boundSummary").textContent = "No current bound pattern."; $("bindFiles").replaceChildren(); }
  }
}
function error(message) { $("error").textContent = message; $("error").hidden = false; }
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
function invalidate(keys) {
  if (pending) start(); // terminate synchronous Rust work; never wait for its stale success
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
function objectiveComparison(parent, r) {
  const note = document.createElement("p");
  note.textContent = `Computed ${r.policy.basis} frontier: ${r.frontier.length ? r.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
    (r.policy.allow_mixed_protocols ? "Mixed source protocols explicitly admitted. " : "Reported input protocols must match. ") +
    "Conditional on these inputs and evaluated alternatives; no propagated uncertainty, empirical fit or global optimum claim.";
  parent.append(note);
  const objectives = r.policy.objectives;
  const constraints = r.policy.constraints.filter(c => !objectives.some(o => o.metric === c.metric));
  const metrics = [...objectives, ...constraints];
  const t = table(parent, "Declared computed objectives — authoritative Rust comparison", ["Alternative", "Admission", "Dominated by",
    ...metrics.map(o => `${o.metric} (${o.direction || "constraint"})`)]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(e => e.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
      ...metrics.map(o => { const q = row.readings[o.metric]; return q ? `${metric(q.value)} ${q.unit}` : "Unknown"; })]);
  }
  for (const pair of r.incomparable) {
    const p = document.createElement("p"); p.textContent = `${pair.alternatives.join(" / ")}: ${pair.reason}`; parent.append(p);
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
function comparison(op, r) {
  const parent = sections.get(op).comparison;
  if (op === "screen") {
    screening(parent, r);
    return { schema: r.schema, basis: r.basis, allow_mixed_protocols: r.allow_mixed_protocols,
      scalar_frontier: r.frontier, request_sha256: r.request_sha256, notes: r.notes };
  }
  if (r.comparison) objectiveComparison(parent, r.comparison);
  const study = op === "scenario" ? r.manufacturing : r;
  const t = table(parent, "Independent manufacturing outcomes", ["Alternative", "Binding", "Cutting", "Sewing", "Purchasing",
    "Stock area (m²)", "Cut area (m²)", "Complete stock mass (kg)", "Observed order cost", "Stitch intervals", "Feed-only seconds"]);
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
    }
    detail(parent, `${row.key}: frozen assignments and source observations (display only)`, {
      pattern_sha256: binding?.pattern_sha256, frozen_changes: row.frozen_changes, pieces: binding?.pieces,
      refusals: { binding: row.binding.refusal, cutting: row.cutting.refusal, sewing: row.sewing.refusal, purchasing: row.purchasing.refusal } });
  }
  assemblyMassComparison(parent, study.alternatives);
  if (study.screening) screening(parent, study.screening);
  if (op === "scenario") {
    const simulations = new Map(r.simulations.map(row => [row.key, row.evaluation]));
    const sim = table(parent, "Computed final-frame results — review drape flags before interpreting heat", ["Alternative", "Drape",
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
    }
    detail(parent, "Retained body fit, scenario options and environment (display only)", r.scenario);
  }
  return { schema: r.schema, source_pattern_sha256: study.source_pattern_sha256, request_sha256: r.request_sha256,
    geometry_changed: study.geometry_changed, physical_validation_claim: study.physical_validation_claim,
    hardware_execution_authorized: study.hardware_execution_authorized, notes: study.notes,
    computed_frontier: r.comparison?.frontier ?? null,
    comparison_basis: r.comparison ? "Declared nominal objectives evaluated by Rust; conditional Pareto set, not an empirical fit/warmth claim or global optimum." :
      "Actual consumer results; no computed objectives requested. Unknowns are not zero, and no ranking is inferred by this page." };
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
    inspected = r.selection;
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
  } else if (op === "bind") {
    bound = r;
    const pre = document.createElement("pre"); pre.textContent = text({ pattern_sha256: r.pattern_sha256, geometry_changed: r.geometry_changed,
      pieces: r.pieces, material_resolution: { without_self_contact: r.drape_materials_without_self_contact, with_self_contact: r.drape_materials_with_self_contact } });
    $("boundSummary").replaceChildren(pre);
    for (const f of msg.files) link(op, $("bindFiles"), f);
  } else {
    const s = sections.get(op); s.section.hidden = false;
    const summary = ["screen", "study", "scenario"].includes(op) ? comparison(op, r) :
      op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, rolls: r.rolls.map(x => ({ material: x.material?.authored?.id,
        width_m: x.width_m, length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? { complete_offer_coverage: r.complete_offer_coverage, currency: r.currency, least_observed_independent_order_total: r.least_observed_independent_order_total, limits: r.limits } :
      op === "stock" ? { used_stock_count: r.used_stock_count, used_gross_area_m2: r.used_gross_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, markers: r.markers.map(m => ({ id: m.id, pieces: m.placements.length,
          gross_area_m2: m.gross_area_m2, defect_free_area_m2: m.defect_free_area_m2, utilization: m.utilization,
          stock_mass_kg: m.stock_mass_kg, mass_unavailable: m.mass_unavailable })), limitations: r.limitations } :
      op === "sew" ? { schema: r.schema, stitches: r.stitches?.length, hardware_execution_authorized: r.plan?.hardware_execution_authorized } :
      op === "heat" || op === "coverage" ? { steady: r.steady, comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim } : r;
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
  const f = $(id).files[0]; if (!f) throw Error(`Choose ${id} first.`);
  if (f.size > (binary ? 256 : 64) * 1024 * 1024) throw Error(`${id} exceeds its input budget.`);
  const data = binary ? await f.arrayBuffer() : await f.text();
  if (!binary) JSON.parse(data);
  return data;
}
function changes() { return { schema: "ftx-material-bindings/1", default: defaultChange, pieces: Object.fromEntries(pieceChanges) }; }
function edits() { $("edits").textContent = text(changes()); }
async function execute(op) {
  if (!ready || pending) return;
  if (op === "bind") clear(["bind", ...boundOperations]);
  else if (op === "fold") clear(["fold", "heat", "coverage"]);
  else if (sections.has(op)) clear([op]);
  if (op === "inspect") { ++selectionRead; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); }
  const id = ++sequence; pending = { id, op }; $("error").hidden = true;
  $("status").textContent = `Running ${op}…`; controls();
  try {
    const msg = { type: op, request: id };
    if (op === "catalog") msg.query = $("query").value;
    else if (op === "inspect") msg.selection = $("selection").value;
    else if (op === "bind") { msg.pattern = sourceText; msg.changes = text(changes()); }
    else if (op === "screen") msg.screenRequest = await read("screenRequest");
    else if (op === "study" || op === "scenario") {
      if (!sourceText) throw Error("Load the original pattern first.");
      // Alternatives always start at the original, never the currently bound single-material preview.
      msg.pattern = sourceText; msg.studyRequest = await read("studyRequest");
      if (op === "scenario") {
        msg.scenarioRequest = await read("scenarioRequest");
        msg.measurements = await read(has("scenarioBody") ? "scenarioBody" : "measurements");
        msg.bytes = await read("body", true);
      }
    }
    else {
      if (!bound) throw Error("Bind the current materials first.");
      msg.pattern = bound.pattern_json;
      if (op === "fold" || op === "exports") { msg.measurements = await read("measurements"); msg.fit = await read("fit"); }
      if (op === "fold") { msg.bytes = await read("body", true); msg.selfContactMm = $("selfContact").checked ? 2 : 0; }
      if (op === "exports") { const d = new Date(), pad = n => String(n).padStart(2, "0");
        msg.today = `${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getDate())}`;
        msg.created = `${msg.today}T${pad(d.getHours())}:${pad(d.getMinutes())}`; }
      if (op === "heat" || op === "coverage") {
        if (!drape) throw Error("Re-drape the current material first."); msg.report = drape;
        msg.conditions = await read(op === "heat" ? "heatConditions" : "coverageConditions");
        if (op === "coverage") msg.map = await read("map");
      }
      if (op === "cut" || op === "price") msg.options = await read("cutOptions");
      if (op === "stock") msg.stockRequest = await read("stockRequest");
      if (op === "price") { msg.quotes = await read("quotes"); msg.market = await read("market"); }
      if (op === "sew") msg.options = await read("sewOptions");
    }
    if (pending?.id !== id) return;
    worker.postMessage(msg, msg.bytes ? [msg.bytes] : []);
  } catch (e) {
    if (pending?.id !== id) return;
    pending = null; error(String(e.message || e)); $("status").textContent = `${op} refused.`; controls();
  }
}
for (const el of commands) el.onclick = () => execute(el.dataset.command);
$("cancel").onclick = () => { start(); $("status").textContent = "Cancelled; reloading local engine."; };
$("catalog").onchange = () => {
  if (!$("catalog").value) return;
  ++selectionRead;
  if (pending?.op === "inspect") start();
  $("selection").value = text({ id: $("catalog").value, allow_estimates: false });
  inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); controls();
};
$("selection").addEventListener("input", () => {
  ++selectionRead;
  inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (pending?.op === "inspect") start(); controls();
});
$("selectionFile").onchange = async () => {
  const rev = ++selectionRead; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (pending?.op === "inspect") start(); controls();
  try { const data = await read("selectionFile"); if (rev !== selectionRead) return;
    $("selection").value = data; controls(); }
  catch (e) { if (rev === selectionRead) error(String(e.message || e)); }
};
$("patternFile").onchange = async () => {
  const rev = ++patternRead; source = null; sourceText = null; defaultChange = null; pieceChanges.clear(); edits();
  invalidate(["bind", ...operations]); $("patternName").textContent = "No pattern loaded.";
  $("target").replaceChildren(); const base = document.createElement("option"); base.value = "";
  base.textContent = "Pattern default (preserve explicit piece choices)"; $("target").append(base);
  try { const raw = await read("patternFile"), data = JSON.parse(raw); if (rev !== patternRead) return;
    if (!Array.isArray(data.pieces) || data.pieces.length > 4096) throw Error("Pattern needs at most 4096 source pieces.");
    source = data; sourceText = raw; $("patternName").textContent = `${$("patternFile").files[0].name}: ${data.pieces.length} source pieces. Original retained.`;
    for (const p of data.pieces) { const o = document.createElement("option"); o.value = p.id; o.textContent = p.id; $("target").append(o); }
    controls();
  } catch (e) { if (rev === patternRead) { source = null; error(String(e.message || e)); controls(); } }
};
$("assign").onclick = () => {
  if (!inspected || !source || pending) return;
  if ($("target").value === "") defaultChange = inspected; else pieceChanges.set($("target").value, inspected);
  edits(); invalidate(["bind", ...boundOperations]);
};
$("reset").onclick = () => { defaultChange = null; pieceChanges.clear(); edits(); invalidate(["bind", ...boundOperations]); };
for (const [id, keys] of Object.entries({ measurements:["exports","fold","heat","coverage","scenario"], fit:["exports","fold","heat","coverage"],
  body:["fold","heat","coverage","scenario"], selfContact:["fold","heat","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  stockRequest:["stock"], sewOptions:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"],
  screenRequest:["screen"], studyRequest:["study","scenario"], scenarioRequest:["scenario"], scenarioBody:["scenario"] })) {
  $(id).addEventListener("change", () => invalidate(id === "measurements" && has("scenarioBody") ? keys.filter(k => k !== "scenario") : keys));
}
window.addEventListener("pagehide", () => { ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();

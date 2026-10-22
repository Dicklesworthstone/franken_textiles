// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const boundOperations = ["exports", "fold", "heat", "vaporSurface", "coverage", "cut", "stock", "price", "sew"];
const operations = [...boundOperations, "screen", "study", "scenario", "pressure", "vapor", "calibrate",
  "stripAcquisition", "stripExperiment", "stripHistory", "stripFit", "outfit"];
const selectionOperations = ["inspect", "pressure", "calibrate", "stripFit"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let preparedBy = null;
let sourceText = null, outfit = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
let interfaceChange = null, interfaceRead = 0, interfaceReady = true;
const pieceChanges = new Map(), urls = new Map();
const combinationGroups = new Map();
const outfitTargets = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = ({ screen: "Source-qualified screening",
    study: "Fixed-pattern manufacturing alternatives", scenario: "Fresh drape and heat alternatives",
    pressure: "Pressure-conditioned material", vapor: "Potential evaporative transfer", calibrate: "Identified material behavior",
    stripAcquisition: "Complete clamped-strip acquisitions", stripExperiment: "Clamped-strip experiment",
    stripHistory: "Strip loading and recovery", vaporSurface: "Vapor transfer on this drape",
    stripFit: "Clamped-strip identification", outfit: "Imported joint outfit" })[op] || op;
  const summary = document.createElement("pre"), links = document.createElement("div"); links.className = "links";
  const comparison = document.createElement("div"); comparison.className = "comparison";
  section.append(title, summary, comparison, links); $("outputs").append(section);
  sections.set(op, { section, summary, comparison, links });
}
const text = value => JSON.stringify(value, null, 2);
const has = id => !!$(id).files.length;
function controls() {
  for (const el of commands) el.disabled = !ready || !!pending;
  $("bind").disabled ||= !source || !interfaceReady;
  $("assign").disabled = !!pending || !source || !inspected;
  $("reset").disabled = !!pending || !source;
  $("addCombination").disabled = !!pending || !source || !inspected;
  $("clearCombinations").disabled = !!pending || !combinationGroups.size;
  $("clearInterfaces").disabled = interfaceChange === null && interfaceReady;
  for (const op of boundOperations) $(op).disabled ||= !bound;
  $("screen").disabled ||= !has("screenRequest");
  $("pressure").disabled ||= !has("compressionRequest");
  $("vapor").disabled ||= !has("vaporRequest");
  $("calibrate").disabled ||= !has("calibrationRequest");
  for (const op of ["stripAcquisition", "stripExperiment", "stripHistory", "stripFit"]) $(op).disabled ||= !has(`${op}Request`);
  $("study").disabled ||= !source || !has("studyRequest");
  $("scenario").disabled ||= !source || !has("studyRequest") || !has("scenarioRequest") || !has("body") || (!has("scenarioBody") && !has("measurements"));
  if ($("combinationMode").checked && !combinationGroups.size) {
    $("study").disabled = true; $("scenario").disabled = true;
  }
  $("exports").disabled ||= !has("measurements") || !has("fit");
  $("fold").disabled ||= !has("measurements") || !has("fit") || !has("body");
  $("selfContact").disabled = !!outfit;
  $("cut").disabled ||= !has("cutOptions");
  $("stock").disabled ||= !has("stockRequest");
  $("price").disabled ||= !has("cutOptions") || !has("quotes") || !has("market");
  $("sew").disabled ||= !has("sewOptions");
  $("heat").disabled ||= !drape || !has("heatConditions");
  $("vaporSurface").disabled ||= !drape || !has("vaporSurfaceConditions");
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
function procurementResult(parent, r) {
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
    const t = table(parent, `${label} — latest supplier observations`,
      ["Offer", "Observed (UTC)", "Valid through (UTC)", "Admission", "Order quantity", `Merchandise cost (${scope.currency})`, "Source continuous-cut limit"]);
    const entries = [...sample.offers.map(offer => ({ offer, quote: offer.quote })),
      ...(sample.excluded_offers || []).map(excluded => ({ excluded, quote: excluded.quote }))];
    for (const { quote: q, offer, excluded } of entries) {
      const expiry = [q.valid_through_at, q.valid_through ? `${q.valid_through} (date limit)` : null].filter(Boolean).join("; ");
      const quantity = offer?.order_quantity ?? excluded?.required_order_quantity;
      const continuity = q.maximum_continuous_quantity == null ? "Not reported" :
        `${metric(q.maximum_continuous_quantity)} ${q.unit}` +
        (offer?.within_observed_continuous_limit === true ? "; within observed limit" : "");
      cells(t, [q.offer_id, q.observed_at || `${q.observed_on} (date only)`, expiry || "Not reported",
        excluded ? `Excluded: ${excluded.reason.replaceAll("_", " ")}` : "Eligible",
        typeof quantity === "number" ? `${metric(quantity)} ${q.unit}` : "Unknown", metric(offer?.order_cost), continuity]);
    }
    if (!entries.length) cells(t, ["No observation in the requested market at this time", "Unknown", "Unknown", "No eligible offer", "Unknown", "Unknown", "Not reported"]);
    detail(parent, `${label}: selected order, source evidence and stock exclusions`, row);
  }
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage, currency: r.currency,
    complete_continuous_length_limit_coverage: r.complete_continuous_length_limit_coverage,
    least_observed_independent_order_total: r.least_observed_independent_order_total, limits: r.limits };
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
  const motionQualified = r.motion_required === true;
  const massBounds = r.policy.basis === "reported_bounds";
  const note = document.createElement("p");
  note.textContent = `Computed ${r.policy.basis} frontier: ${r.frontier.length ? r.frontier.join(", ") : "No eligible non-dominated alternatives"}. ` +
    (r.policy.allow_mixed_protocols ? "Mixed source protocols explicitly admitted. " : "Reported input protocols must match. ") +
    (massBounds ? "Mass limits and dominance use complete reported mass ranges at fixed authored geometry and inventory. " +
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
  const t = table(parent, "Declared computed objectives — authoritative Rust comparison", ["Alternative", "Admission", "Dominated by",
    ...metrics.map(o => `${o.metric} (${o.direction || "constraint"})`)]);
  for (const row of r.alternatives) {
    cells(t, [row.key, row.eligible ? "Eligible" : `Excluded: ${row.exclusions.map(e => e.message).join("; ")}`,
      row.dominated_by.length ? row.dominated_by.join(", ") : "None reported",
      ...metrics.map(o => {
        const q = row.readings[o.metric];
        if (!q) return "Unknown";
        const range = q.reported_bounds ? `; reported range [${q.reported_bounds.map(metric).join(", ")}] ${q.unit}` :
          massBounds ? "; reported range unknown" : "";
        return `${metric(q.value)} ${q.unit}${range}`;
      })]);
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
function transientHeatComparison(parent, key, result, dependency, policy) {
  const consumers = [["transient_surface", "Independent sheets"], ["transient_coverage", "Body coverage + exposed cloth"]]
    .filter(([name]) => policy?.[name] || result?.[name]);
  if (!consumers.length) return;
  const t = table(parent, `${key}: transient heat — complete time history or refusal`, ["Thermal domain", "Status", "Duration (s)",
    "Thermal substeps", "Final temperature range (°C)", "Net boundary energy (J)", "Physical stored change (J)",
    "Energy residual (J)", "Signed remap residual (J)"]);
  for (const [name, label] of consumers) {
    const output = result?.[name], h = available(output), range = h?.trace?.at(-1)?.temperature_range_c;
    cells(t, [label, output ? state(output) : `Unavailable: ${state(dependency)}`, metric(h?.duration_s), metric(h?.thermal_steps),
      range ? `[${range.map(metric).join(", ")}]` : "Unknown", metric(h?.net_boundary_energy_j),
      metric(h?.physical_stored_change_j ?? h?.stored_change_j), metric(h?.balance_error_j),
      name === "transient_surface" ? "Not applicable" : metric(h?.signed_remap_residual_j)]);
    detail(parent, `${key}: ${label} temperature history, source and energy accounting (display only)`, h || { refusal: output?.refusal || dependency?.refusal });
    const channels = [
      ["plastic_dissipation_heat", "Plastic yield", "plastic_dissipation_j", "initial_plastic_dissipation_j"],
      ["relaxation_dissipation_heat", "Maxwell dashpots", "viscous_dissipation_j", "initial_viscous_dissipation_j"]
    ].filter(([field]) => h?.[field]);
    if (channels.length) {
      const domain = name === "transient_surface" ? "independent-sheet heat" : "coverage and exposed-cloth heat";
      const work = table(parent, `${key}: material work supplied to ${domain}`, ["Physical source", "Work since motion start (J)",
        "Supplied material heat (J)", "Unconverted work (J)", "Initial work excluded (J)"]);
      for (const [field, label, total, initial] of channels) {
        const source = h[field];
        cells(work, [label, metric(source[total]), metric(source.supplied_heat_j),
          metric(source.unconverted_dissipation_j), metric(source[initial])]);
      }
      const scope = document.createElement("p");
      scope.textContent = "The energy residual is physical stored change minus net boundary energy minus the sum of qualified source heat. " +
        "Each source needs its own material conversion evidence. Work already accumulated in the initial drape, reversible storage and numerical loss are excluded. " +
        (channels.every(([field]) => h[field].distribution === "uniform_reference_thickness" && h[field].temporal_distribution === "uniform_over_accepted_interval") ?
          "Converted heat is distributed uniformly through each sheet's reference thickness and over its accepted motion interval. " :
          "Inspect the retained report for the declared heat distribution. ") +
        (name === "transient_coverage" ? "Source heat follows covered and exposed reference material; bare body receives none. " : "") +
        "Temperatures do not feed back into mechanics. " +
        "The temperature-history report retains the material fractions and their source evidence.";
      parent.append(scope);
    }
  }
  const note = document.createElement("p");
  note.textContent = "These are separate one-way thermal models with declared initial temperatures and interval films. Do not add their energy totals. " +
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
function motionComparison(parent, rows, simulations, requested) {
  if (!requested && ![...simulations.values()].some(o => available(o)?.motion)) return;
  const note = document.createElement("p");
  note.textContent = "Motion continues the initial sewn cloth; it does not re-drape each pose. Motion enters the computed frontier through explicitly declared motion objectives or required consumers. " +
    "Endpoint steady snapshots are separate from explicitly requested transient histories. Successful motion is not a settled-garment or empirical fit claim.";
  parent.append(note);
  const t = table(parent, "Prescribed motion — complete trajectory or refusal", ["Alternative", "Motion", "Initial drape settled",
    "Duration (s)", "Accepted substeps", "Attempted substeps", "Peak absolute panel-edge strain (%)"]);
  for (const row of rows) {
    const evaluation = simulations.get(row.key), initial = available(evaluation), result = available(initial?.motion);
    cells(t, [row.key, initial ? state(initial.motion) : state(evaluation),
      initial?.drape?.settled === true ? "Yes — also review initial flags" : initial?.drape?.settled === false ? "NO — review initial state" : "Unknown",
      metric(result?.duration_s), metric(result?.samples?.length), metric(result?.trials), metric(result?.metrics?.max_absolute_edge_strain_pct)]);
    transientHeatComparison(parent, row.key, result, initial?.motion || evaluation, requested);
    if (!result) continue;
    const endpoints = table(parent, `${row.key}: endpoint steady snapshots — not transient heat`, ["Time (s)", "Arm angle (degrees)",
      "Surface heat", "Signed surface power (W)", "Body coverage", "Signed coverage power (W)"]);
    for (const endpoint of result.keyframes) {
      cells(endpoints, [metric(endpoint.time_s), metric(endpoint.arm_abduction_deg), state(endpoint.surface),
        metric(available(endpoint.surface)?.steady?.power_w), state(endpoint.coverage), metric(available(endpoint.coverage)?.steady?.power_w)]);
    }
    if (result.keyframes.some(e => e.vapor_surface && e.vapor_surface.status !== "not_requested")) {
      const vapor = table(parent, `${row.key}: endpoint vapor potentials — independent sheets`,
        ["Time (s)", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const endpoint of result.keyframes) {
        const transfer = available(endpoint.vapor_surface);
        cells(vapor, [metric(endpoint.time_s), state(endpoint.vapor_surface), metric(transfer?.area_m2),
          metric(transfer?.potential_heat_flow_w), metric(transfer?.potential_mass_flow_kg_s)]);
      }
    }
    detail(parent, `${row.key}: motion identities, material history and endpoint refusals (display only)`, {
      path_sha256: result.path_sha256, initial_positions_sha256: result.initial_positions_sha256,
      initial_body_collision_sha256: result.initial_body_collision_sha256, initial_drape_gating: result.initial_drape_gating,
      retained_vertex_samples: result.retained_vertex_samples, pairs_tested: result.pairs_tested,
      membrane_history: result.membrane_history, bending_history: result.bending_history,
      membrane_plasticity_history: result.membrane_plasticity_history,
      endpoints: result.keyframes.map(e => ({ time_s: e.time_s, positions_sha256: e.positions_sha256,
        body_collision_sha256: e.body_collision_sha256, surface_refusal: e.surface?.refusal, coverage_refusal: e.coverage?.refusal, vapor_refusal: e.vapor_surface?.refusal })),
      scope: result.scope, thermal_basis: result.thermal_basis });
    motionSamples(parent, row.key, result);
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
  if (study.alternatives.some(row => row.choices)) {
    const choices = table(parent, "Material combinations evaluated by the study", ["Alternative", "Group choices"]);
    for (const row of study.alternatives) cells(choices, [row.key,
      Object.entries(row.choices || {}).map(([group, choice]) => `${group}: ${choice}`).join("; ")]);
  }
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
      pattern_sha256: binding?.pattern_sha256, choices: row.choices, frozen_changes: row.frozen_changes, pieces: binding?.pieces,
      refusals: { binding: row.binding.refusal, cutting: row.cutting.refusal, finite_stock: row.finite_stock?.refusal,
        cutting_job: row.cutting_job?.refusal, sewing: row.sewing.refusal, purchasing: row.purchasing.refusal } });
  }
  if (study.alternatives.some(row => row.finite_stock && row.finite_stock.status !== "not_requested")) {
    const stock = table(parent, "Finite hides and remnants — used physical inventory", ["Alternative", "Finite stock",
      "Used items", "Used gross area (m²)", "Allocated cut area (m²)", "Complete used-stock mass (kg)"]);
    for (const row of study.alternatives) {
      const m = row.metrics || {};
      cells(stock, [row.key, state(row.finite_stock), metric(m.used_stock_count), metric(m.used_stock_gross_area_m2),
        metric(m.finite_stock_external_cut_area_m2), metric(m.used_stock_mass_kg)]);
      const plan = available(row.finite_stock);
      if (plan) detail(parent, `${row.key}: physical stock, defects and allocated pieces (display only)`, plan);
    }
  }
  if (study.alternatives.some(row => row.sewing_selection && row.sewing_selection.status !== "not_requested")) {
    const selection = table(parent, "Material-specific sewing recipe search", ["Alternative", "Status", "Chosen recipes",
      "Trials evaluated", "Search complete", "Budget exhausted", "Listed optimum proved", "Unresolved trials"]);
    for (const row of study.alternatives) {
      const plan = available(row.sewing_selection);
      const choices = plan?.selected_choices ? Object.entries(plan.selected_choices)
        .map(([operation, recipe]) => `${operation}: ${recipe}`).join("; ") :
        plan ? "No listed recipe selected" : "Unavailable";
      cells(selection, [row.key, state(row.sewing_selection), choices, metric(plan?.trials?.length),
        plan ? String(plan.search_complete) : "Unknown", plan ? String(plan.budget_exhausted) : "Unknown",
        plan ? String(plan.optimality_proven) : "Unknown", metric(plan?.unresolved_trials)]);
      detail(parent, `${row.key}: sewing recipe choices, work limits and refusals (display only)`,
        plan || { refusal: row.sewing_selection?.refusal });
    }
  }
  cuttingJobComparison(op, parent, study.alternatives);
  assemblyMassComparison(parent, study.alternatives);
  vaporComparison(op, parent, study.alternatives);
  if (study.screening) screening(parent, study.screening);
  if (op === "scenario") {
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
    }
    vaporCoverageComparison(op, parent, study.alternatives, simulations, r.scenario?.scenario?.vapor_coverage);
    if ([...simulations.values()].some(o => {
      const vapor = available(o)?.vapor_surface;
      return vapor && vapor.status !== "not_requested";
    })) {
      const vapor = table(parent, "Initial drape vapor potentials — independent sheets",
        ["Alternative", "Vapor transfer", "Current sheet area (m²)", "Potential heat transfer (W)", "Potential mass transfer (kg/s)"]);
      for (const row of study.alternatives) {
        const outcome = simulations.get(row.key), initial = available(outcome), transfer = available(initial?.vapor_surface);
        cells(vapor, [row.key, initial ? state(initial.vapor_surface) : state(outcome), metric(transfer?.area_m2),
          metric(transfer?.potential_heat_flow_w), metric(transfer?.potential_mass_flow_kg_s)]);
        detail(parent, `${row.key}: vapor conditions, source columns and refusal (display only)`,
          transfer || { refusal: initial?.vapor_surface?.refusal || outcome?.refusal });
      }
    }
    attachedMassComparison(parent, study.alternatives, simulations);
    motionComparison(parent, study.alternatives, simulations, r.scenario?.scenario?.motion);
    detail(parent, "Retained body fit, scenario options and environment (display only)", r.scenario);
  }
  return { schema: r.schema, source_pattern_sha256: study.source_pattern_sha256, request_sha256: r.request_sha256,
    geometry_changed: study.geometry_changed, physical_validation_claim: study.physical_validation_claim,
    hardware_execution_authorized: study.hardware_execution_authorized, notes: study.notes,
    computed_frontier: r.comparison?.frontier ?? null,
    comparison_basis: r.comparison ? (r.comparison.policy.basis === "reported_bounds" ?
      "Declared assembly mass ranges evaluated by Rust with fixed authored geometry and inventory; unresolved overlap and unknown ranges remain explicit." :
      "Declared nominal objectives evaluated by Rust; conditional Pareto set, not an empirical fit/warmth claim or global optimum.") :
      "Actual consumer results; no computed objectives requested. Unknowns are not zero, and no ranking is inferred by this page." };
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
    "External cut area (m²)", "Complete engaged-stock mass (kg)", "Purchasing", "Observed complete order cost"]);
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
    const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
    if (typeof r.pattern_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-pattern.json`, text: r.pattern_json, mime: "application/json" });
    if (typeof r.replay_request_json === "string") link(op, actions,
      { name: `alternative-${i + 1}-cut-job-request.json`, text: r.replay_request_json, mime: "application/json" });
  }
}
function vaporCoverageComparison(op, parent, alternatives, simulations, requested) {
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
      metric(c?.covered_potential_heat_flow_w), metric(c?.covered_potential_mass_flow_kg_s)]);
    detail(parent, `${row.key}: vapor coverage regions, source observations and boundary assumptions (display only)`,
      r || { refusal: initial?.vapor_coverage?.refusal || evaluation?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-vapor-coverage-replay.json`, text: r.replay_request_json, mime: "application/json" });
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
    cells(t, [row.key, state(row.vapor), metric(r?.request.area_m2), metric(r?.total_resistance_m2_pa_w),
      metric(r?.potential_heat_flow_w), metric(r?.potential_mass_flow_kg_s),
      r ? (r.mixed_protocols ? "Mixed; explicitly admitted" : "Reported protocols match") : "Unknown"]);
    detail(parent, `${row.key}: vapor layers, source observations and boundary assumptions (display only)`,
      r || { refusal: row.vapor?.refusal });
    if (typeof r?.replay_request_json === "string") {
      const actions = document.createElement("div"); actions.className = "links"; parent.append(actions);
      link(op, actions, { name: `alternative-${i + 1}-vapor-replay.json`, text: r.replay_request_json, mime: "application/json" });
    }
  }
}
function inspectResult(r, selectionJson) {
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
function compressionHistoryResult(parent, r) {
  const t = table(parent, "Compression and heat history — chronological holds", ["Time (s)", "Pressure (Pa)",
    "Gap (m)", "Cell temperatures (°C)", "Cumulative external work (J)", "Cumulative entering heat (J)",
    "Cumulative leaving heat (J)", "Stored energy change (J)", "Heat balance error (J)"]);
  for (const step of r.steps) cells(t, [metric(step.elapsed_s), metric(step.pressure_pa), metric(step.thickness_m),
    step.temperature_c.map(metric).join(", "), metric(step.cumulative_external_work_j),
    metric(step.cumulative_inside_heat_j), metric(step.cumulative_outside_heat_j),
    metric(step.cumulative_stored_change_j), metric(step.cumulative_heat_balance_error_j)]);
  const note = document.createElement("p");
  note.textContent = "External mechanical work is separate from sensible heat; no work-to-heat conversion is inferred. " +
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
  const note = document.createElement("p");
  note.textContent = "Potential steady transfer through the declared series of vapor resistances. " +
    "Boundary vapor pressures, film resistances, area and latent heat are supplied assumptions. " +
    "This does not establish available sweat, actual evaporative cooling, condensation, garment comfort or a coupled dry/wet heat history. " +
    (r.mixed_protocols ? "Different reported source protocols were explicitly admitted. " : "Reported layer protocols match. ") +
    "Source measurement conditions remain attached to each layer; they are not an operating range for this model.";
  parent.append(note);
  const totals = table(parent, "Potential vapor and latent heat exchange", ["Quantity", "Value", "Unit"]);
  for (const [label, value, unit] of [
    ["Total vapor resistance", r.total_resistance_m2_pa_w, "m²·Pa/W"],
    ["Boundary vapor pressure difference", r.pressure_difference_pa, "Pa"],
    ["Potential latent heat flux", r.potential_heat_flux_w_m2, "W/m²"],
    ["Potential latent heat flow", r.potential_heat_flow_w, "W"],
    ["Potential water mass flux", r.potential_mass_flux_kg_m2_s, "kg/(m²·s)"],
    ["Potential water mass flow", r.potential_mass_flow_kg_s, "kg/s"],
    ["Inside film pressure drop", r.inside_film_pressure_drop_pa, "Pa"],
    ["Outside film pressure drop", r.outside_film_pressure_drop_pa, "Pa"]
  ]) cells(totals, [label, metric(value), unit]);
  const layers = table(parent, "Layer resistances and vapor pressure drops — inside to outside",
    ["Layer", "Material", "Ret (m²·Pa/W)", "Inside pressure (Pa)", "Outside pressure (Pa)", "Drop (Pa)", "Evidence"]);
  for (const row of r.layers) {
    cells(layers, [row.key, row.selection.id, metric(row.observation.value), metric(row.inside_pressure_pa),
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
  for (const [label, value, unit] of [
    ["Current cloth area", r.area_m2, "m²"],
    ["Potential latent heat flow", r.potential_heat_flow_w, "W"],
    ["Potential water mass flow", r.potential_mass_flow_kg_s, "kg/s"]
  ]) cells(totals, [label, metric(value), unit]);
  const pieces = table(parent, "Independent piece potentials", ["Piece", "Area (m²)", "Potential heat (W)", "Potential water (kg/s)"]);
  for (const row of r.pieces) cells(pieces, [row.instance, metric(row.area_m2),
    metric(row.potential_heat_flow_w), metric(row.potential_mass_flow_kg_s)]);
  detail(parent, "Original resistance observations, boundaries and source protocols", r.columns);
  return { schema: r.schema, model_sha256: r.model_sha256, request_sha256: r.request_sha256,
    scope: r.scope, physical_validation_claim: r.physical_validation_claim,
    next: "Download the complete report, frozen replay, original conditions and retained drape below." };
}
function sewingResult(parent, r) {
  const plan = r.plan, order = plan?.ordering;
  if (order) {
    const counts = table(parent, "Sewing setup changes — declared needle and thread identities",
      ["Schedule", "Needle changes", "Thread changes", "Total changes"]);
    for (const [label, changes] of [["Original dependency order", order.baseline_changes],
      ["Selected order", order.selected_changes]]) {
      cells(counts, [label, metric(changes.needle_changes), metric(changes.thread_changes), metric(changes.total_changes)]);
    }
    const operations = table(parent, "Selected sewing operation order",
      ["Position", "Operation", "Source seam", "Needle", "Thread", "Required predecessors"]);
    for (const [i, row] of plan.seams.entries()) {
      const pass = row.pass;
      cells(operations, [metric(i + 1), pass.id || pass.seam, pass.seam, pass.needle, pass.thread, pass.after.join(", ") || "None declared"]);
    }
    const note = document.createElement("p");
    note.textContent = (order.optimality_proven ?
      "Minimum setup-change count proven for the declared operations, dependencies and initial setup. " :
      "Best complete feasible schedule found; minimum setup-change count has not been proven. ") +
      (order.budget_exhausted ? "The search reached its declared work limit. " : "") +
      "Needle and thread changes each count once. Handling, rethreading duration, assembly access and total manufacturing time are not inferred. " +
      (plan.options.ordering.initial_setup ? "The supplied initial setup is included in the counts." :
        "No initial setup was supplied, so installing the first setup is not counted.");
    parent.append(note);
    detail(parent, "Baseline order, selected order and bounded search result", order);
  }
  if (plan?.seams.some(row => row.source_span)) {
    const spans = table(parent, "Partial-seam operations — original source correspondence",
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
  return { schema: r.schema, stitches: r.stitches?.length, feed_seconds: plan?.feed_seconds,
    setup_changes: order?.selected_changes, minimum_setup_changes_proven: order?.optimality_proven,
    search_nodes: order?.search_nodes, hardware_execution_authorized: plan?.hardware_execution_authorized };
}
function permanentStretchResult(parent, r) {
  const c = r.calibration, d = c.diagnostics;
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
  const work = table(parent, "Identification work and FIT excitation", ["Diagnostic", "Value"]);
  for (const row of [
    ["Completed / attempted starts", `${metric(d.starts_completed)} / ${metric(d.starts_attempted)}`],
    ["Forward evaluations / limit", `${metric(d.evaluations)} / ${metric(d.evaluation_limit)}`],
    ["Reserved history nodes / limit", `${metric(d.history_steps)} / ${metric(d.history_step_limit)}`],
    ["Refused forward trials", metric(d.trials_refused)],
    ["FIT excitation pivot", d.sensitivity_evaluated ? metric(d.excitation_pivot) : "Not evaluated"]
  ]) cells(work, row);
  const note = document.createElement("p");
  note.textContent = "The shared rate-independent law follows every recorded loading, unloading and reversal node, including nodes with unobserved forces. " +
    "Strains are raw material-frame Green strain; loads are second-Piola sheet resultants in N/m. " +
    "FIT chooses the coefficients before CHECK replay. Search ranges, force scales and local sensitivity are not confidence intervals or a proof of global uniqueness. " +
    "Applicability limits remain supplied source observations; the fitter does not infer them from the largest load. Fitted coefficients remain estimates.";
  parent.append(note);
  detail(parent, "Complete source histories, permanent strain, stored energy, dissipation and residuals", c);
  return { response: "Permanent stretch and directional hardening", status: c.status,
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
function calibrationResult(parent, r) {
  if (r.calibration?.schema === "ftx-strip-calibration-report/1") return stripCalibrationResult(parent, r);
  if (r.calibration?.schema === "ftx-membrane-plasticity-calibration-report/1") return permanentStretchResult(parent, r);
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
  const method = nonlinear ? c.material?.membrane_response?.area_response ? "Nonlinear stretch, shear and area response" :
    "Nonlinear stretch and shear" : c.input.orthotropic ?
    timeSearch ? "Directional relaxation with times identified within declared bounds" : "Directional relaxation at the declared times" :
    "Proportional relaxation ratios and times";
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
  } else {
    if (op === "pressure" || op === "calibrate" || op === "stripFit") {
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
    const summary = ["screen", "study", "scenario"].includes(op) ? comparison(op, r) :
      op === "calibrate" ? calibrationResult(s.comparison, r) :
      op === "stripHistory" ? stripHistoryResult(s.comparison, r) :
      ["stripAcquisition", "stripExperiment", "stripFit"].includes(op) ? stripResult(s.comparison, op, r) :
      op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, maximum_marker_length_m: r.options?.maximum_marker_length_m,
        rolls: r.rolls.map((x, i) => ({ marker_number: i + 1, material: x.material?.authored?.id,
        width_m: x.width_m, length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? procurementResult(s.comparison, r) :
      op === "stock" ? { used_stock_count: r.used_stock_count, used_gross_area_m2: r.used_gross_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, markers: r.markers.map(m => ({ id: m.id, pieces: m.placements.length,
          gross_area_m2: m.gross_area_m2, defect_free_area_m2: m.defect_free_area_m2, utilization: m.utilization,
          stock_mass_kg: m.stock_mass_kg, mass_unavailable: m.mass_unavailable })), limitations: r.limitations } :
      op === "sew" ? sewingResult(s.comparison, r) :
      op === "vapor" ? vaporResult(s.comparison, r) :
      op === "vaporSurface" ? vaporSurfaceResult(s.comparison, r) :
      op === "heat" || op === "coverage" ? { steady: r.steady, comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim } :
      op === "pressure" && ["ftx-compressed-thermal-history-report/1", "ftx-compressed-thermal-history-report/2"].includes(r.schema) ? compressionHistoryResult(s.comparison, r) :
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
  const f = $(id).files[0]; if (!f) throw Error(`Choose ${id} first.`);
  if (f.size > (binary ? 256 : 64) * 1024 * 1024) throw Error(`${id} exceeds its input budget.`);
  const data = binary ? await f.arrayBuffer() : await f.text();
  if (!binary) JSON.parse(data);
  return data;
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
  if (!policy || Array.isArray(policy) || policy.schema !== "ftx-material-study/1") throw Error("Combination mode needs an ftx-material-study/1 policy object.");
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
  if (selectionOperations.includes(op)) { ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); }
  const id = ++sequence; pending = { id, op }; $("error").hidden = true;
  $("status").textContent = `Running ${op}…`; controls();
  try {
    const msg = { type: op, request: id };
    if (op === "catalog") msg.query = $("query").value;
    else if (op === "inspect") msg.selection = $("selection").value;
    else if (op === "bind") {
      if (!interfaceReady) throw Error("Load a valid replacement interface list or choose Keep original interfaces.");
      msg.pattern = sourceText; msg.changes = changes();
    }
    else if (op === "pressure") {
      msg.compressionLayer = $("compressionLayer").value;
      msg.compressionRequest = await read("compressionRequest");
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
      if (op === "heat" || op === "vaporSurface" || op === "coverage") {
        if (!drape) throw Error("Re-drape the current material first."); msg.report = drape;
        msg.conditions = await read(op === "heat" ? "heatConditions" : op === "vaporSurface" ? "vaporSurfaceConditions" : "coverageConditions");
        if (op === "coverage") msg.map = await read("map");
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
  ++selectionRead;
  if (selectionOperations.includes(pending?.op)) start();
  $("selection").value = text({ id: $("catalog").value, allow_estimates: false });
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren(); controls();
};
$("selection").addEventListener("input", () => {
  ++selectionRead;
  preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
});
$("selectionFile").onchange = async () => {
  const rev = ++selectionRead; preparedBy = null; inspected = null; $("detail").textContent = ""; $("capabilities").replaceChildren();
  if (selectionOperations.includes(pending?.op)) start(); controls();
  try { const data = await read("selectionFile"); if (rev !== selectionRead) return;
    $("selection").value = data; controls(); }
  catch (e) { if (rev === selectionRead) error(String(e.message || e)); }
};
function installPattern(raw, label, garments = []) {
  const data = JSON.parse(raw);
  if (!Array.isArray(data.pieces) || data.pieces.length > 4096) throw Error("Pattern needs at most 4096 source pieces.");
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
$("patternFile").onchange = async () => {
  const rev = ++patternRead; source = null; sourceText = null; outfit = null; defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits();
  outfitTargets.clear();
  combinationGroups.clear(); showCombinations(); $("combinationPieces").replaceChildren();
  invalidate(["bind", ...boundOperations, "study", "scenario", "outfit"]); $("patternName").textContent = "No pattern loaded.";
  $("target").replaceChildren(); const base = document.createElement("option"); base.value = "";
  base.textContent = "Pattern default (preserve explicit piece choices)"; $("target").append(base);
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
for (const [id, keys] of Object.entries({ measurements:["exports","fold","heat","vaporSurface","coverage","scenario"], fit:["exports","fold","heat","vaporSurface","coverage"],
  body:["fold","heat","vaporSurface","coverage","scenario"], selfContact:["fold","heat","vaporSurface","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  stockRequest:["stock"], sewOptions:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"], vaporRequest:["vapor"],
  stripAcquisitionRequest:["stripAcquisition"], stripExperimentRequest:["stripExperiment"],
  stripHistoryRequest:["stripHistory"], vaporSurfaceConditions:["vaporSurface"],
  screenRequest:["screen"], studyRequest:["study","scenario"], scenarioRequest:["scenario"], scenarioBody:["scenario"] })) {
  $(id).addEventListener("change", () => invalidate(id === "measurements" && has("scenarioBody") ? keys.filter(k => k !== "scenario") : keys));
}
window.addEventListener("pagehide", () => { ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();

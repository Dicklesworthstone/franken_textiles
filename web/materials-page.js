// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const boundOperations = ["exports", "fold", "heat", "coverage", "cut", "stock", "price", "sew"];
const operations = [...boundOperations, "screen", "study", "scenario", "pressure", "calibrate"];
const selectionOperations = ["inspect", "pressure", "calibrate"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let preparedBy = null;
let sourceText = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
let interfaceChange = null, interfaceRead = 0, interfaceReady = true;
const pieceChanges = new Map(), urls = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = ({ screen: "Source-qualified screening",
    study: "Fixed-pattern manufacturing alternatives", scenario: "Fresh drape and heat alternatives",
    pressure: "Pressure-conditioned material", calibrate: "Identified material behavior" })[op] || op;
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
  $("clearInterfaces").disabled = interfaceChange === null && interfaceReady;
  for (const op of boundOperations) $(op).disabled ||= !bound;
  $("screen").disabled ||= !has("screenRequest");
  $("pressure").disabled ||= !has("compressionRequest");
  $("calibrate").disabled ||= !has("calibrationRequest");
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
function procurementResult(parent, r) {
  for (const row of r.roll_prices || []) {
    const sample = row.sample, scope = sample.scope;
    const note = document.createElement("p");
    note.textContent = `Roll ${row.roll_index + 1}: ${metric(row.required_stock_area_m2)} m² of stock. ` +
      `Evaluated at ${scope.as_of_at || `${scope.as_of} (date only)`} UTC. ` +
      `Market: ${scope.currency}; ${scope.region}; ${scope.quality}; ${scope.terms}. ` +
      "The report retains the latest observed offer and its exclusion; an older price is not substituted.";
    parent.append(note);
    const t = table(parent, `Roll ${row.roll_index + 1} — latest supplier observations`,
      ["Offer", "Observed (UTC)", "Valid through (UTC)", "Admission", "Order quantity", `Merchandise cost (${scope.currency})`]);
    const entries = [...sample.offers.map(offer => ({ offer, quote: offer.quote })),
      ...(sample.excluded_offers || []).map(excluded => ({ excluded, quote: excluded.quote }))];
    for (const { quote: q, offer, excluded } of entries) {
      const expiry = [q.valid_through_at, q.valid_through ? `${q.valid_through} (date limit)` : null].filter(Boolean).join("; ");
      const quantity = offer?.order_quantity ?? excluded?.required_order_quantity;
      cells(t, [q.offer_id, q.observed_at || `${q.observed_on} (date only)`, expiry || "Not reported",
        excluded ? `Excluded: ${excluded.reason.replaceAll("_", " ")}` : "Eligible",
        typeof quantity === "number" ? `${metric(quantity)} ${q.unit}` : "Unknown", metric(offer?.order_cost)]);
    }
    if (!entries.length) cells(t, ["No observation in the requested market at this time", "Unknown", "Unknown", "No eligible offer", "Unknown", "Unknown"]);
    detail(parent, `Roll ${row.roll_index + 1}: selected order, source evidence and stock exclusions`, row);
  }
  return { complete_offer_coverage: r.complete_offer_coverage,
    complete_observed_quantity_coverage: r.complete_observed_quantity_coverage, currency: r.currency,
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
    const plastic = name === "transient_surface" ? h?.plastic_dissipation_heat : null;
    if (plastic) {
      const work = table(parent, `${key}: plastic work supplied to independent-sheet heat`, ["Plastic work since motion start (J)",
        "Supplied sheet heat (J)", "Unconverted work (J)", "Initial plastic work excluded (J)"]);
      cells(work, [metric(plastic.plastic_dissipation_j), metric(plastic.supplied_heat_j),
        metric(plastic.unconverted_dissipation_j), metric(plastic.initial_plastic_dissipation_j)]);
      const scope = document.createElement("p");
      scope.textContent = "The sheet energy residual is stored change minus net boundary energy minus supplied plastic heat. " +
        "Work already accumulated in the initial drape is excluded. " +
        (plastic.distribution === "uniform_reference_thickness" && plastic.temporal_distribution === "uniform_over_accepted_interval" ?
          "Converted heat is distributed uniformly through each sheet's reference thickness and over its accepted motion interval. " :
          "Inspect the retained report for the declared heat distribution. ") +
        "This coupling supplies independent sheets only; coverage keeps its separate heat model, and temperatures do not feed back into mechanics. " +
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
    detail(parent, `${row.key}: motion identities, material history and endpoint refusals (display only)`, {
      path_sha256: result.path_sha256, initial_positions_sha256: result.initial_positions_sha256,
      initial_body_collision_sha256: result.initial_body_collision_sha256, initial_drape_gating: result.initial_drape_gating,
      retained_vertex_samples: result.retained_vertex_samples, pairs_tested: result.pairs_tested,
      membrane_history: result.membrane_history, bending_history: result.bending_history,
      membrane_plasticity_history: result.membrane_plasticity_history,
      endpoints: result.keyframes.map(e => ({ time_s: e.time_s, positions_sha256: e.positions_sha256,
        body_collision_sha256: e.body_collision_sha256, surface_refusal: e.surface?.refusal, coverage_refusal: e.coverage?.refusal })),
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
      refusals: { binding: row.binding.refusal, cutting: row.cutting.refusal, finite_stock: row.finite_stock?.refusal,
        sewing: row.sewing.refusal, purchasing: row.purchasing.refusal } });
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
  assemblyMassComparison(parent, study.alternatives);
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
  return { schema: r.schema, stitches: r.stitches?.length, feed_seconds: plan?.feed_seconds,
    setup_changes: order?.selected_changes, minimum_setup_changes_proven: order?.optimality_proven,
    search_nodes: order?.search_nodes, hardware_execution_authorized: plan?.hardware_execution_authorized };
}
function calibrationResult(parent, r) {
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
  const method = nonlinear ? "Nonlinear stretch and shear" : c.input.orthotropic ?
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
  } else if (op === "bind") {
    bound = r;
    const pre = document.createElement("pre"); pre.textContent = text({ pattern_sha256: r.pattern_sha256, geometry_changed: r.geometry_changed,
      pieces: r.pieces, material_resolution: { without_self_contact: r.drape_materials_without_self_contact, with_self_contact: r.drape_materials_with_self_contact } });
    $("boundSummary").replaceChildren(pre);
    for (const f of msg.files) link(op, $("bindFiles"), f);
  } else {
    if (op === "pressure" || op === "calibrate") {
      ++selectionRead; preparedBy = op;
      if (typeof r.selection_json === "string") {
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
      op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, rolls: r.rolls.map(x => ({ material: x.material?.authored?.id,
        width_m: x.width_m, length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? procurementResult(s.comparison, r) :
      op === "stock" ? { used_stock_count: r.used_stock_count, used_gross_area_m2: r.used_gross_area_m2,
        external_cut_area_m2: r.external_cut_area_m2, markers: r.markers.map(m => ({ id: m.id, pieces: m.placements.length,
          gross_area_m2: m.gross_area_m2, defect_free_area_m2: m.defect_free_area_m2, utilization: m.utilization,
          stock_mass_kg: m.stock_mass_kg, mass_unavailable: m.mass_unavailable })), limitations: r.limitations } :
      op === "sew" ? sewingResult(s.comparison, r) :
      op === "heat" || op === "coverage" ? { steady: r.steady, comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim } :
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
function resetInterfaces() {
  ++interfaceRead; interfaceChange = null; interfaceReady = true;
  $("interfacesFile").value = "";
  $("interfacesSummary").textContent = "Keep the original pattern's interface evidence.";
}
async function execute(op) {
  if (!ready || pending) return;
  if (op === "bind") clear(["bind", ...boundOperations]);
  else if (op === "fold") clear(["fold", "heat", "coverage"]);
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
    else if (op === "pressure") msg.compressionRequest = await read("compressionRequest");
    else if (op === "calibrate") {
      msg.calibrationMode = $("calibrationMode").value;
      msg.allowEstimates = $("calibrationEstimates").checked;
      msg.calibrationRequest = await read("calibrationRequest");
    }
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
$("patternFile").onchange = async () => {
  const rev = ++patternRead; source = null; sourceText = null; defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits();
  invalidate(["bind", ...boundOperations, "study", "scenario"]); $("patternName").textContent = "No pattern loaded.";
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
$("reset").onclick = () => { defaultChange = null; pieceChanges.clear(); resetInterfaces(); edits(); invalidate(["bind", ...boundOperations]); };
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
for (const [id, operation] of [["compressionRequest", "pressure"], ["calibrationRequest", "calibrate"],
  ["calibrationMode", "calibrate"], ["calibrationEstimates", "calibrate"]]) {
  $(id).addEventListener("change", () => {
    if (preparedBy === operation) {
      ++selectionRead; preparedBy = null; inspected = null;
      $("detail").textContent = ""; $("capabilities").replaceChildren();
    }
    invalidate([operation]);
  });
}
for (const [id, keys] of Object.entries({ measurements:["exports","fold","heat","coverage","scenario"], fit:["exports","fold","heat","coverage"],
  body:["fold","heat","coverage","scenario"], selfContact:["fold","heat","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  stockRequest:["stock"], sewOptions:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"],
  screenRequest:["screen"], studyRequest:["study","scenario"], scenarioRequest:["scenario"], scenarioBody:["scenario"] })) {
  $(id).addEventListener("change", () => invalidate(id === "measurements" && has("scenarioBody") ? keys.filter(k => k !== "scenario") : keys));
}
window.addEventListener("pagehide", () => { ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();

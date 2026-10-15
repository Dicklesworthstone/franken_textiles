// State is local to this tab. Epoch/request checks cover worker responses AND delayed file reads.
const $ = id => document.getElementById(id);
const commands = [...document.querySelectorAll("[data-command]")];
const operations = ["exports", "fold", "heat", "coverage", "cut", "price", "sew"];
let worker, epoch = 0, sequence = 0, pending = null, ready = false;
let source = null, inspected = null, bound = null, drape = null;
let sourceText = null;
let defaultChange = null;
let selectionRead = 0, patternRead = 0;
const pieceChanges = new Map(), urls = new Map();
const sections = new Map();
for (const op of operations) {
  const section = document.createElement("div"); section.hidden = true;
  const title = document.createElement("h3"); title.textContent = op;
  const summary = document.createElement("pre"), links = document.createElement("div"); links.className = "links";
  section.append(title, summary, links); $("outputs").append(section);
  sections.set(op, { section, summary, links });
}
const text = value => JSON.stringify(value, null, 2);
const has = id => !!$(id).files.length;
function controls() {
  for (const el of commands) el.disabled = !ready || !!pending;
  $("bind").disabled ||= !source;
  $("assign").disabled = !!pending || !source || !inspected;
  $("reset").disabled = !!pending || !source;
  for (const op of operations) $(op).disabled ||= !bound;
  $("exports").disabled ||= !has("measurements") || !has("fit");
  $("fold").disabled ||= !has("measurements") || !has("fit") || !has("body");
  $("cut").disabled ||= !has("cutOptions");
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
    if (sections.has(key)) { const s = sections.get(key); s.section.hidden = true; s.links.replaceChildren(); s.summary.textContent = ""; }
    if (key === "fold") { drape = null; $("viewer").hidden = true; $("viewer").removeAttribute("src"); }
    if (key === "cut") { $("marker").hidden = true; $("marker").removeAttribute("src"); }
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
    const summary = op === "fold" ? { settled: r.settled, flags: r.flags, material: r.material?.basis } :
      op === "cut" ? { pattern_sha256: r.pattern_sha256, rolls: r.rolls.map(x => ({ material: x.material?.authored?.id,
        width_m: x.width_m, length_m: x.length_m, stock_mass_kg: x.stock_mass_kg, utilization: x.utilization })), limitations: r.limitations } :
      op === "price" ? { complete_offer_coverage: r.complete_offer_coverage, currency: r.currency, least_observed_independent_order_total: r.least_observed_independent_order_total, limits: r.limits } :
      op === "sew" ? { schema: r.schema, stitches: r.stitches?.length, hardware_execution_authorized: r.plan?.hardware_execution_authorized } :
      op === "heat" || op === "coverage" ? { steady: r.steady, comfort_claim: r.comfort_claim, empirical_validation_claim: r.empirical_validation_claim } : r;
    s.summary.textContent = text(summary);
    for (const f of msg.files) {
      const url = link(op, s.links, f);
      if (op === "fold" && f.mime === "text/html") { $("viewer").src = url; $("viewer").hidden = false; }
      if (op === "cut" && f.mime === "image/svg+xml") { $("marker").src = url; $("marker").hidden = false; }
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
  if (op === "bind") clear(["bind", ...operations]);
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
  edits(); invalidate(["bind", ...operations]);
};
$("reset").onclick = () => { defaultChange = null; pieceChanges.clear(); edits(); invalidate(["bind", ...operations]); };
for (const [id, keys] of Object.entries({ measurements:["exports","fold","heat","coverage"], fit:["exports","fold","heat","coverage"],
  body:["fold","heat","coverage"], selfContact:["fold","heat","coverage"], cutOptions:["cut","price"], quotes:["price"], market:["price"],
  sewOptions:["sew"], heatConditions:["heat"], map:["coverage"], coverageConditions:["coverage"] })) {
  $(id).addEventListener("change", () => invalidate(keys));
}
window.addEventListener("pagehide", () => { ++epoch; worker?.terminate(); clear(["bind", ...operations]); });
start();

// Explicit user-selected evidence, bounded before transfer. Request identities and worker epochs
// prevent stale successes after input changes, failures, cancellation or replacement.
const $ = (id) => document.getElementById(id);
const sourceControls = [...document.querySelectorAll("#source input, #source select")];
const editControls = [...document.querySelectorAll("#edits textarea")];
let worker, epoch = 0, request = 0, pending = null;
let ready = false, busy = false, hasSheet = false;
let sheetSource = null, traceOptions = null, restoredEdits = null, draftLoading = false;
let reviewPattern = null, workbenchTransfer = null;
const urls = [], draftUrls = [], editFields = ["names", "targets", "labels"];
const editText = id => restoredEdits?.[id]?.displayed === $(id).value ? restoredEdits[id].text : $(id).value;

function controls() {
  for (const element of [...sourceControls, ...editControls]) element.disabled = busy;
  $("trace").disabled = busy || !ready || !$("image").files.length;
  $("apply").disabled = busy || !ready || !hasSheet;
  $("cancel").disabled = !busy;
  $("saveDraft").disabled = busy || !hasSheet || !sheetSource?.source_sha256;
  $("draftFile").disabled = !hasSheet || !sheetSource?.source_sha256 || (busy && !draftLoading);
  $("openPattern").disabled = busy || !reviewPattern;
}

function clearDraft() {
  $("draftDownloads").replaceChildren();
  for(const url of draftUrls.splice(0)) URL.revokeObjectURL(url);
}
function clearResults() {
  reviewPattern = null; $("openPattern").disabled = true;
  if(workbenchTransfer) workbenchTransfer.pattern = null;
  clearDraft();
  $("results").hidden = true;
  $("files").replaceChildren();
  $("pictures").replaceChildren();
  $("pieces").replaceChildren();
  for (const url of urls.splice(0)) URL.revokeObjectURL(url);
}

function error(message) {
  clearResults();
  $("error").textContent = message;
  $("error").hidden = false;
  $("status").textContent = "No current review was published.";
  busy = false;
  draftLoading = false;
  pending = null;
  controls();
}

function startWorker() {
  if (worker) worker.terminate();
  const generation = ++epoch;
  hasSheet = ready = busy = false;
  sheetSource = traceOptions = restoredEdits = null; draftLoading = false;
  pending = null;
  clearResults();
  $("error").hidden = true;
  $("status").textContent = "Loading local tracing engine…";
  controls();
  try {
    worker = new Worker(new URL("./trace-worker.js", import.meta.url), { type: "module" });
  } catch (e) {
    error(`Could not start the tracing worker: ${e.message || e}`);
    return;
  }
  worker.onmessage = ({ data: msg }) => {
    if (generation !== epoch) return;
    if (msg.type === "loadError") {
      ready = hasSheet = false;
      error(`Could not load web/pkg: ${msg.message}. Rebuild the browser bundle and reload.`);
      return;
    }
    if (msg.type === "ready") {
      ready = true;
      $("status").textContent = "Ready. Choose a PNG, JPEG or WebP and trace it.";
      controls();
      return;
    }
    if (msg.request !== pending) return;
    if (msg.type === "error") { error(msg.message); return; }
    if (msg.type === "result") {
      busy = false;
      pending = null;
      if(!hasSheet) sheetSource = { source_sha256: msg.report.source_sha256, options: traceOptions };
      hasSheet = true;
      show(msg);
      controls();
    }
  };
  worker.onerror = (event) => {
    if (generation !== epoch) return;
    ready = hasSheet = false;
    error(`Tracing engine failed to load or run: ${event.message || "unknown error"}. Rebuild web/pkg and reload this page.`);
  };
}

function begin() {
  draftLoading = false;
  clearResults();
  $("error").hidden = true;
  busy = true;
  pending = ++request;
  $("status").textContent = "Computing locally… Cancel discards this worker and its current sheet.";
  controls();
  return pending;
}

function imageOptions() {
  const crop = $("crop").value.trim();
  const stroke = $("stroke").value.trim();
  const polarity = $("invert").value;
  const options = {
    channel: $("channel").value,
    threshold: $("threshold").value,
    invert: polarity === "auto" ? null : polarity === "yes",
    dashes: $("dashes").checked,
    min_area_frac: Number($("area").value),
  };
  if (crop) {
    options.crop = crop.split(",").map(Number);
    if (options.crop.length !== 4 || options.crop.some((n) => !Number.isSafeInteger(n) || n < 0)) {
      throw new Error("Crop must be four nonnegative integers: x,y,width,height.");
    }
  }
  if (stroke) {
    options.stroke = Number(stroke);
    if (!Number.isFinite(options.stroke)) throw new Error("Stroke width must be finite.");
  }
  if (!Number.isFinite(options.min_area_frac)) throw new Error("Area fraction must be finite.");
  return JSON.stringify(options);
}

$("trace").onclick = async () => {
  const id = begin(), generation = epoch;
  hasSheet = false;
  sheetSource = restoredEdits = null;
  $("names").value = "{}"; $("targets").value = "[]"; $("labels").value = "[]";
  try {
    const file = $("image").files[0];
    if (!file || file.size > 16 * 1024 * 1024) throw new Error("Select a PNG, JPEG or WebP no larger than 16 MiB.");
    const options = imageOptions();
    traceOptions = options;
    const bytes = await file.arrayBuffer();
    if (generation !== epoch || pending !== id) return;
    worker.postMessage({ type: "trace", request: id, bytes, options }, [bytes]);
  } catch (e) {
    if (generation === epoch && pending === id) error(String(e.message || e));
  }
};

$("apply").onclick = () => {
  const id = begin();
  const names = editText("names"), targets = editText("targets"), labels = editText("labels");
  if ([names, targets, labels].some((s) => new TextEncoder().encode(s).length > 256 * 1024)) {
    error("Each JSON input must be at most 256 KiB.");
    return;
  }
  worker.postMessage({ type: "review", request: id, names, targets, labels });
};

$("cancel").onclick = startWorker;
for (const input of sourceControls) input.addEventListener("change", () => {
  // Source or trace-setting changes require a new trace, not an edit of the old sheet.
  $("names").value = "{}"; $("targets").value = "[]"; $("labels").value = "[]";
  startWorker();
});
for (const id of editFields) $(id).addEventListener("input", () => {
  if(restoredEdits) delete restoredEdits[id];
  if(draftLoading) { pending = null; busy = draftLoading = false; controls(); }
  clearResults();
  $("status").textContent = "Edits have changed. Apply them to produce a new review.";
});

function checkDraftEdits(draft) {
  for(const id of editFields) if(typeof draft[id]!=="string" || new Blob([draft[id]]).size>256*1024) {
    throw Error("Each draft editor must be UTF-8 text of at most 256 KiB.");
  }
}
$("saveDraft").onclick = () => {
  if(busy || !hasSheet || !sheetSource?.source_sha256) return;
  clearDraft();
  try {
    const draft={schema:"ftx-trace-edits/1",...sheetSource,...Object.fromEntries(editFields.map(id=>[id,editText(id)]))};
    checkDraftEdits(draft);
    const text=JSON.stringify(draft,null,2);
    if(new Blob([text]).size>8*1024*1024) throw Error("Tracing draft exceeds 8 MiB.");
    const url=URL.createObjectURL(new Blob([text],{type:"application/json"})),a=document.createElement("a");
    draftUrls.push(url);a.href=url;a.download="trace-edits.json";a.textContent="trace-edits.json";$("draftDownloads").append(a);
    $("status").textContent="Edits draft ready to download; apply edits separately for a current review.";
  } catch(e) {error(String(e.message || e));}
};
$("draftFile").addEventListener("change",async()=>{
  const file=$("draftFile").files[0];
  if(!file || !hasSheet || !sheetSource?.source_sha256 || (busy && !draftLoading)) return;
  const id=begin(),generation=epoch;draftLoading=true;$("draftFile").value="";controls();
  try {
    if(file.size>8*1024*1024) throw Error("Tracing draft exceeds 8 MiB; no source was read.");
    const bytes=await file.arrayBuffer();
    if(generation!==epoch || pending!==id) return;
    const draft=JSON.parse(new TextDecoder("utf-8",{fatal:true,ignoreBOM:true}).decode(bytes));
    if(!draft || draft.schema!=="ftx-trace-edits/1" || Object.keys(draft).length!==6 ||
      draft.source_sha256!==sheetSource.source_sha256 || draft.options!==sheetSource.options) {
      throw Error("Draft must match the original traced image and tracing settings.");
    }
    checkDraftEdits(draft);restoredEdits={};
    for(const key of editFields) {$(key).value=draft[key];restoredEdits[key]={text:draft[key],displayed:$(key).value};}
    busy=draftLoading=false;pending=null;controls();
    $("status").textContent="Edits draft loaded. Apply edits and audit to produce a current review.";
  } catch(e) {
    if(generation!==epoch || pending!==id) return;
    error(String(e.message || e));$("status").textContent="Draft not loaded; current edits are unchanged.";
  }
});

function show({ report, files }) {
  $("summary").textContent = `Review only — scale: ${report.scale_status}; audit: ${report.audit.summary.ok ? "no errors" : "errors to resolve"}`;
  $("details").textContent = JSON.stringify(report, null, 2);
  $("limits").textContent = report.no_claim;
  $("scaleWarning").textContent = report.scale_fit?.warning || "";
  for (const piece of report.pieces) {
    const p = document.createElement("p");
    p.textContent = `${piece.id}: edges ${piece.edges.join(", ")}; corners ${piece.vertices.join(", ")}`;
    $("pieces").append(p);
  }
  for (const { name, bytes } of files) {
    const png = name.endsWith(".png");
    const blob = new Blob([bytes], { type: png ? "image/png" : name.endsWith(".zip") ? "application/zip" : "application/json" });
    if(name === "pattern.json") reviewPattern = {pattern:blob,source_sha256:report.source_sha256};
    const url = URL.createObjectURL(blob);
    urls.push(url);
    const a = document.createElement("a");
    a.href = url; a.download = name.split("/").pop(); a.textContent = name;
    $("files").append(a);
    if (png && name !== "mask.png") {
      const figure = document.createElement("figure"), image = document.createElement("img"), caption = document.createElement("figcaption");
      image.src = url; image.alt = name; image.loading = "lazy";
      caption.textContent = name;
      figure.append(image, caption); $("pictures").append(figure);
    }
  }
  $("results").hidden = false;
  $("status").textContent = "Review ready. Inspect the scale residuals and every audit finding.";
}

$("openPattern").onclick = () => {
  if(busy || !reviewPattern) return;
  const token=Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,"0")).join("");
  const url=new URL("index.html",import.meta.url);url.searchParams.set("traceImport",token);
  const target=window.open(url.href,"_blank");
  if(!target) {$("status").textContent="Workbench popup blocked. Download pattern.json and open it in the Workbench instead.";return;}
  workbenchTransfer={target,token,...reviewPattern};
  $("status").textContent="Workbench opened. Choose measurement evidence and run its ordinary checks there.";
};
window.addEventListener("message",event=>{
  const transfer=workbenchTransfer,origin=new URL(import.meta.url).origin,m=event.data;
  if(!transfer || event.origin!==origin || event.source!==transfer.target ||
    m?.type!=="ftx-trace-pattern-ready" || m.token!==transfer.token) return;
  transfer.target.postMessage(transfer.pattern ? {type:"ftx-trace-pattern",token:transfer.token,
    pattern:transfer.pattern,source_sha256:transfer.source_sha256} :
    {type:"ftx-trace-pattern-cancelled",token:transfer.token},origin);
  workbenchTransfer=null;
});

window.addEventListener("beforeunload", () => { worker?.terminate(); for (const url of [...urls,...draftUrls]) URL.revokeObjectURL(url); });
startWorker();

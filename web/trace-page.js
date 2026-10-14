// Explicit user-selected evidence, bounded before transfer. Request identities and worker epochs
// prevent stale successes after input changes, failures, cancellation or replacement.
const $ = (id) => document.getElementById(id);
const sourceControls = [...document.querySelectorAll("#source input, #source select")];
const editControls = [...document.querySelectorAll("#edits textarea")];
let worker, epoch = 0, request = 0, pending = null;
let ready = false, busy = false, hasSheet = false;
const urls = [];

function controls() {
  for (const element of [...sourceControls, ...editControls]) element.disabled = busy;
  $("trace").disabled = busy || !ready || !$("image").files.length;
  $("apply").disabled = busy || !ready || !hasSheet;
  $("cancel").disabled = !busy;
}

function clearResults() {
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
  pending = null;
  controls();
}

function startWorker() {
  if (worker) worker.terminate();
  const generation = ++epoch;
  hasSheet = ready = busy = false;
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
      $("status").textContent = "Ready. Choose a PNG or JPEG and trace it.";
      controls();
      return;
    }
    if (msg.request !== pending) return;
    if (msg.type === "error") { error(msg.message); return; }
    if (msg.type === "result") {
      busy = false;
      pending = null;
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
  $("names").value = "{}"; $("targets").value = "[]"; $("labels").value = "[]";
  try {
    const file = $("image").files[0];
    if (!file || file.size > 16 * 1024 * 1024) throw new Error("Select a PNG or JPEG no larger than 16 MiB.");
    const options = imageOptions();
    const bytes = await file.arrayBuffer();
    if (generation !== epoch || pending !== id) return;
    worker.postMessage({ type: "trace", request: id, bytes, options }, [bytes]);
  } catch (e) {
    if (generation === epoch && pending === id) error(String(e.message || e));
  }
};

$("apply").onclick = () => {
  const id = begin();
  const names = $("names").value, targets = $("targets").value, labels = $("labels").value;
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
for (const input of editControls) input.addEventListener("input", () => {
  clearResults();
  $("status").textContent = "Edits have changed. Apply them to produce a new review.";
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
    const url = URL.createObjectURL(new Blob([bytes], { type: png ? "image/png" : name.endsWith(".zip") ? "application/zip" : "application/json" }));
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

window.addEventListener("beforeunload", () => { worker?.terminate(); for (const url of urls) URL.revokeObjectURL(url); });
startWorker();
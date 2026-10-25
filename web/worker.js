// FrankenTextiles in a worker: drafting and exports take a second or two, a fold-up half a minute or more,
// and neither may freeze the page. It answers the page's messages (init, fields, run, body, fold) as the
// Pyodide worker running webapp.py answered the first three.
let ftx = null;
let body = null; // the parsed body model, kept for every fold-up
let docs = null; // the last round's pattern, measurements and fit spec (JSON text)
let bodyRequest, foldRequest;

const pad = (n) => String(n).padStart(2, "0");
function clock() {
  // the wearer's local date (tape files, staleness) and minute (export stamps), as webapp.py reads them
  const d = new Date();
  const today = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return { today, created: `${today}T${pad(d.getHours())}:${pad(d.getMinutes())}` };
}

// the drape's progress lines, called from inside the module while it works
self.ftxProgress = (line) => postMessage({ type: "foldProgress", request: foldRequest, text: line });

const text = (bytes) => (bytes ? new TextDecoder().decode(bytes) : null);
// Python truthiness, as webapp.py's has_fold_up reads a placement: {} and [] are false
const truthy = (v) => !(v === null || v === undefined || v === false || v === 0 || v === ""
  || (Array.isArray(v) && v.length === 0) || (typeof v === "object" && !Array.isArray(v) && Object.keys(v).length === 0));
let dead = false; // set when the module traps: its memory and handles can no longer be trusted

onmessage = async (ev) => {
  const msg = ev.data;
  if (dead) {
    return postMessage({ type: "error", op: msg.type, request: msg.request, fatal: true, message: "the module stopped after an internal error: reload the page" });
  }
  try {
    if (msg.type === "init") {
      postMessage({ type: "progress", text: "Loading FrankenTextiles…" });
      ftx = await import(msg.pkg + "ftx_wasm.js");
      await ftx.default({ module_or_path: msg.pkg + "ftx_wasm_bg.wasm" });
      const info = JSON.parse(ftx.program_info());
      return postMessage({ type: "ready", info, sheetDictionaries: JSON.parse(ftx.calibration_dictionaries()),
        dxf: "every export built in", version: "FrankenTextiles, WebAssembly" });
    }
    if (msg.type === "fields") {
      const rows = JSON.parse(ftx.form_fields(JSON.stringify(msg.args), clock().today));
      return postMessage({ type: "fields", request: msg.request, rows, program: msg.args.program });
    }
    if (msg.type === "run") {
      docs = null; // a refused replacement must never leave the previous garment available
      const { today, created } = clock();
      const round = ftx.run(JSON.stringify(msg.req), today, created);
      try {
        const res = JSON.parse(round.page());
        const blobs = {};
        if (res.zip) blobs.zip = round.zip();
        for (const f of res.files || []) blobs[f.name] = round.file(f.name);
        const pattern = text(round.file("pattern.json"));
        const data = pattern && !res.errors ? JSON.parse(pattern) : null;
        // a fold-up needs a pattern that validates, is to scale and has a piece with a fold-up placement
        const foldable = !!data && res.status !== "BLOCKED" && (data.meta || {}).scale_status !== "UNSCALED"
          && (data.pieces || []).some((p) => truthy(p.placement) && p.sim !== false);
        if (foldable) {
          docs = { request: msg.request, pattern, measurements: text(round.file("measurements.json")), fit: text(round.file("fit_spec.json")) };
        }
        return postMessage({ type: "result", request: msg.request, res, blobs, foldable }, Object.values(blobs).map((b) => b.buffer));
      } finally { round.free(); }
    }
    if (msg.type === "clearBody") {
      if (body) body.free();
      body = null; bodyRequest = undefined;
      return;
    }
    if (msg.type === "body") {
      if (body) body.free();
      body = null; bodyRequest = msg.request;
      body = new ftx.WebBody(new Uint8Array(msg.bytes));
      return postMessage({ type: "bodyReady", request: msg.request });
    }
    if (msg.type === "fold") {
      if (!body || !docs) throw new Error("load the body model and run a round first");
      if (msg.round !== docs.request || msg.body !== bodyRequest) throw new Error("the garment or body changed: run the current round and load its body first");
      foldRequest = msg.request;
      if (msg.bodyFit != null && (typeof msg.bodyFit !== "string" || msg.bodyFit.length > 16 * 1024 * 1024)) {
        throw new Error("accepted body-fit report must be JSON text of at most 16 MiB");
      }
      const imported = msg.bodyFit != null;
      if (imported && msg.bodyFitConsent !== true) throw new Error("associate the accepted fit with this wearer before folding");
      const fold = imported
        ? ftx.fold_up_with_body_fit(body, docs.pattern, docs.measurements, docs.fit, msg.selfContactMm, msg.bodyFit, msg.armDeg)
        : ftx.fold_up(body, docs.pattern, docs.measurements, docs.fit, msg.selfContactMm);
      try {
        const reportText = fold.report(); // preserve numerical JSON for source-locked thermal replay
        const report = JSON.parse(reportText);
        const viewer = fold.viewer();
        return postMessage({ type: "folded", request: msg.request, report, reportText, viewer,
          bodyFitText: imported ? msg.bodyFit : null });
      } finally { fold.free(); }
    }
  } catch (e) {
    const full = String((e && e.message) || e).trim();
    // a trap (a Rust panic: "unreachable") leaves the module's handles borrowed and its memory suspect
    const fatal = e instanceof WebAssembly.RuntimeError;
    if (fatal) dead = true;
    postMessage({
      type: "error", op: msg.type, request: msg.request, fatal,
      message: fatal ? "the module stopped after an internal error: reload the page" : full.split("\n").pop(),
      detail: full,
    });
  }
};

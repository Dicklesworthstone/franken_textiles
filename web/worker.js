// FrankenTextiles in a worker: drafting and exports take a second or two, a fold-up half a minute or more,
// and neither may freeze the page. It answers the page's messages (init, fields, run, body, fold) as the
// Pyodide worker running webapp.py answered the first three.
let ftx = null;
let body = null; // the parsed body model, kept for every fold-up
let docs = null; // the last round's pattern, measurements and fit spec (JSON text)

const pad = (n) => String(n).padStart(2, "0");
function clock() {
  // the wearer's local date (tape files, staleness) and minute (export stamps), as webapp.py reads them
  const d = new Date();
  const today = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return { today, created: `${today}T${pad(d.getHours())}:${pad(d.getMinutes())}` };
}

// the drape's progress lines, called from inside the module while it works
self.ftxProgress = (line) => postMessage({ type: "foldProgress", text: line });

const text = (bytes) => (bytes ? new TextDecoder().decode(bytes) : null);

onmessage = async (ev) => {
  const msg = ev.data;
  try {
    if (msg.type === "init") {
      postMessage({ type: "progress", text: "Loading FrankenTextiles…" });
      ftx = await import(msg.pkg + "ftx_wasm.js");
      await ftx.default({ module_or_path: msg.pkg + "ftx_wasm_bg.wasm" });
      const info = JSON.parse(ftx.program_info());
      return postMessage({ type: "ready", info, dxf: "every export built in", version: "FrankenTextiles, WebAssembly" });
    }
    if (msg.type === "fields") {
      const rows = JSON.parse(ftx.form_fields(JSON.stringify(msg.args), clock().today));
      return postMessage({ type: "fields", rows, program: msg.args.program });
    }
    if (msg.type === "run") {
      const { today, created } = clock();
      const round = ftx.run(JSON.stringify(msg.req), today, created);
      const res = JSON.parse(round.page());
      const blobs = {};
      if (res.zip) blobs.zip = round.zip();
      for (const f of res.files || []) blobs[f.name] = round.file(f.name);
      docs = null;
      const pattern = text(round.file("pattern.json"));
      const data = pattern && !res.errors ? JSON.parse(pattern) : null;
      // a fold-up needs a pattern that validates, is to scale and has a piece with a fold-up placement
      const foldable = !!data && res.status !== "BLOCKED" && (data.meta || {}).scale_status !== "UNSCALED"
        && (data.pieces || []).some((p) => p.placement && p.sim !== false);
      if (foldable) {
        docs = { pattern, measurements: text(round.file("measurements.json")), fit: text(round.file("fit_spec.json")) };
      }
      round.free();
      return postMessage({ type: "result", res, blobs, foldable }, Object.values(blobs).map((b) => b.buffer));
    }
    if (msg.type === "body") {
      const parsed = new ftx.WebBody(new Uint8Array(msg.bytes)); // a bad file throws and keeps the loaded one
      if (body) body.free();
      body = parsed;
      return postMessage({ type: "bodyReady" });
    }
    if (msg.type === "fold") {
      if (!body || !docs) throw new Error("load the body model and run a round first");
      const fold = ftx.fold_up(body, docs.pattern, docs.measurements, docs.fit, msg.selfContactMm);
      const report = JSON.parse(fold.report());
      const viewer = fold.viewer();
      fold.free();
      return postMessage({ type: "folded", report, viewer });
    }
  } catch (e) {
    const full = String((e && e.message) || e).trim();
    postMessage({ type: "error", message: full.split("\n").pop(), detail: full });
  }
};

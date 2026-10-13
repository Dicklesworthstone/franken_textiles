// FrankenTextiles in a worker: drafting and exports take a second or two and must not freeze the page. It
// answers the page's three messages (init, fields, run) as the Pyodide worker running webapp.py did.
let ftx = null;

const pad = (n) => String(n).padStart(2, "0");
function clock() {
  // the wearer's local date (tape files, staleness) and minute (export stamps), as webapp.py reads them
  const d = new Date();
  const today = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return { today, created: `${today}T${pad(d.getHours())}:${pad(d.getMinutes())}` };
}

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
      round.free();
      return postMessage({ type: "result", res, blobs }, Object.values(blobs).map((b) => b.buffer));
    }
  } catch (e) {
    const full = String((e && e.message) || e).trim();
    postMessage({ type: "error", message: full.split("\n").pop(), detail: full });
  }
};

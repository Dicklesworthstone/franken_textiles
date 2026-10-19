// The only network request loads the local application bundle. User evidence is never uploaded.
// Every operation carries explicit inputs; no old pattern/drape survives a failed replacement.
import init, * as ftx from "./pkg/ftx_wasm.js";
let failure = null, active = null;
const ready = init().then(() => postMessage({ type: "ready" })).catch(e => {
  failure = e; postMessage({ type: "loadError", message: String(e.message || e) });
});
self.ftxProgress = text => postMessage({ type: "progress", request: active, text });
const json = text => JSON.parse(text);
const file = (name, value, mime = "application/json") => ({ name, text: typeof value === "string" ? value : JSON.stringify(value, null, 2), mime });
function budget(msg) {
  let bytes = 0;
  for (const value of Object.values(msg)) {
    if (typeof value === "string") bytes += new TextEncoder().encode(value).byteLength;
  }
  if (bytes > 64 * 1024 * 1024) throw Error("Combined JSON inputs exceed 64 MiB.");
  if (msg.bytes?.byteLength > 256 * 1024 * 1024) throw Error("Body model exceeds 256 MiB.");
}

self.onmessage = async ({ data: msg }) => {
  await ready;
  if (failure) return postMessage({ type: "error", request: msg.request, message: String(failure.message || failure), fatal: true });
  active = msg.request;
  try {
    budget(msg);
    let result, files = [], viewer = null, picture = null, raw, reportText = null;
    switch (msg.type) {
      case "catalog": result = json(ftx.material_catalog(msg.query)); break;
      case "inspect": result = json(ftx.material_inspect(msg.selection)); break;
      case "pressure":
        raw = ftx.material_pressure_condition(msg.compressionRequest); result = json(raw);
        if (typeof result.selection_json !== "string") throw Error("Rebuild web/pkg: missing pressure-conditioned selection bridge.");
        result.inspection = json(ftx.material_inspect(result.selection_json));
        files = [file("pressure-conditioned-material.json", raw),
          file("pressure-conditioned-selection.json", result.selection_json),
          file("compression-request.json", msg.compressionRequest)];
        break;
      case "calibrate": {
        const identify = ({ nonlinear: ftx.material_calibrate_nonlinear,
          relaxation: ftx.material_calibrate_relaxation })[msg.calibrationMode];
        if (!["nonlinear", "relaxation"].includes(msg.calibrationMode)) throw Error("Choose the response to identify.");
        if (typeof identify !== "function") throw Error("Rebuild web/pkg: missing material calibration engine.");
        if (typeof msg.allowEstimates !== "boolean") throw Error("Declare admission of fitted estimates explicitly.");
        raw = identify(msg.calibrationRequest, msg.allowEstimates); result = json(raw);
        if (typeof result.selection_json !== "string" || !result.material?.capabilities) {
          throw Error("Rebuild web/pkg: missing calibrated selection and capability bridge.");
        }
        files = [file("material-calibration.json", raw), file("calibrated-selection.json", result.selection_json),
          file("calibration-input.json", msg.calibrationRequest)];
        break;
      }
      case "screen":
        raw = ftx.material_screen(msg.screenRequest); result = json(raw);
        files = [file("material-screen.json", raw), file("screen-request.json", msg.screenRequest)];
        break;
      case "study":
        raw = ftx.material_study(msg.pattern, msg.studyRequest); result = json(raw);
        files = [file("material-study.json", raw), file("original-pattern.json", msg.pattern),
          file("study-request.json", msg.studyRequest)];
        break;
      case "scenario": {
        // A new handle for every invocation: no previous alternative's body or cloth state.
        const model = new ftx.WebBody(new Uint8Array(msg.bytes));
        try {
          raw = ftx.material_scenario_study(model, msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements);
          result = json(raw);
        } finally { model.free(); }
        // Save original JSON text, not the display objects: number spellings participate in locks.
        // The large local model stays with its owner; the report identifies its exact source hash.
        files = [file("material-scenario-study.json", raw), file("original-pattern.json", msg.pattern),
          file("study-request.json", msg.studyRequest), file("scenario-request.json", msg.scenarioRequest),
          file("body-input.json", msg.measurements)];
        break;
      }
      case "bind":
        raw = ftx.material_bind(msg.pattern, msg.changes); result = json(raw);
        if (typeof result.pattern_json !== "string") throw Error("Rebuild web/pkg: missing lossless material pattern bridge.");
        files = [file("pattern.json", result.pattern_json), file("material-bindings.json", raw),
          file("material-changes.json", msg.changes), file("original-pattern.json", msg.pattern)];
        break;
      case "fold": {
        const model = new ftx.WebBody(new Uint8Array(msg.bytes));
        try {
          const fold = ftx.fold_up(model, msg.pattern, msg.measurements, msg.fit, msg.selfContactMm);
          try { reportText = fold.report(); result = json(reportText); viewer = fold.viewer(); }
          finally { fold.free(); }
        } finally { model.free(); }
        files = [file("sim_report.json", reportText), file("fold_viewer.html", viewer, "text/html")];
        break;
      }
      case "heat":
        raw = ftx.thermal_steady_surface(msg.report, msg.conditions); result = json(raw);
        files = [file("thermal-surface.json", raw)]; break;
      case "coverage":
        // Validate each complete JSON fragment, but preserve its number representation. A JS
        // parse/stringify round trip would invalidate the Rust material/geometry receipt hashes.
        for (const fragment of [msg.report, msg.map, msg.conditions]) json(fragment);
        raw = ftx.thermal_steady_coverage(`{"schema":"ftx-steady-coverage-request/1","report":${msg.report},"map":${msg.map},"conditions":${msg.conditions}}`);
        result = json(raw); files = [file("thermal-coverage.json", raw)]; break;
      case "cut":
        raw = ftx.cut_plan(msg.pattern, msg.options); result = json(raw);
        files = [file("cut-plan.json", raw)];
        // One bounded on-demand preview rather than quadratically regenerating every roll.
        if (result.rolls.length) {
          picture = ftx.cut_svg(msg.pattern, msg.options, 0);
          files.push(file("roll-0.svg", picture, "image/svg+xml"));
        }
        break;
      case "stock": {
        raw = ftx.stock_plan(msg.pattern, msg.stockRequest); result = json(raw);
        const residual = ftx.stock_residual(msg.pattern, msg.stockRequest);
        json(residual);
        files = [file("finite-stock-plan.json", raw), file("residual-stock-request.json", residual)];
        const i = result.markers.findIndex(m => m.placements.length);
        if (i >= 0) {
          picture = ftx.stock_svg(msg.pattern, msg.stockRequest, i);
          files.push(file(`stock-${i}.svg`, picture, "image/svg+xml"));
        }
        break;
      }
      case "price":
        raw = ftx.cut_price(msg.pattern, msg.options, msg.quotes, msg.market); result = json(raw);
        files = [file("cut-price.json", raw)]; break;
      case "sew":
        raw = ftx.sew_stitches(msg.pattern, msg.options); result = json(raw);
        files = [file("sewing-program.json", raw)]; break;
      case "exports": {
        for (const fragment of [msg.pattern, msg.fit, msg.measurements]) json(fragment);
        const round = ftx.run(`{"pattern":${msg.pattern},"fit_spec":${msg.fit},"body":{"mode":"file","measurements":${msg.measurements}}}`, msg.today, msg.created);
        try {
          result = json(round.page());
          if (result.errors || result.status === "BLOCKED") throw Error(JSON.stringify(result.errors || result));
          files = [{ name: round.zip_name(), bytes: round.zip(), mime: "application/zip" }];
        } finally { round.free(); }
        break;
      }
      default: throw Error("Unknown material workbench operation.");
    }
    postMessage({ type: "result", request: msg.request, op: msg.type, result, files, viewer, picture, reportText },
      files.filter(f => f.bytes).map(f => f.bytes.buffer));
  } catch (e) {
    const fatal = e instanceof WebAssembly.RuntimeError;
    if (fatal) failure = e;
    postMessage({ type: "error", request: msg.request, op: msg.type, fatal, message: String(e.message || e) });
  } finally { active = null; }
};

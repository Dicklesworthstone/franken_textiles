// Local-only worker for Knitting & Programmed Continuous Yarn Dyeing (FTX-K18).
// No pattern, image, or yarn data ever leaves the browser.
import init, * as ftx from "./pkg/ftx_wasm.js?v=2026-11-27.3";

const EXPECTED_WASM_STAMP = "ftx-wasm-interface/2026-11-27.3";
let loadFailure = null;

const ready = init({ module_or_path: new URL("./pkg/ftx_wasm_bg.wasm?v=2026-11-27.3", import.meta.url) }).then(() => {
  if ((typeof ftx.initSync === "function" || "wasm_interface_stamp" in ftx) &&
      (typeof ftx.wasm_interface_stamp !== "function" || ftx.wasm_interface_stamp() !== EXPECTED_WASM_STAMP)) {
    throw new Error("Stale web/pkg bundle: rebuild web/pkg via ./tools/build_web.sh");
  }
  if (typeof ftx.knit_swatch_json !== "function" ||
      typeof ftx.knit_panel_preview_json !== "function" ||
      typeof ftx.knit_export_json !== "function" ||
      typeof ftx.dye_optimize_json !== "function") {
    throw new Error("Knitting & dyeing WASM exports missing from web/pkg: rebuild via ./tools/build_web.sh");
  }
  postMessage({ type: "ready" });
}).catch((error) => {
  loadFailure = error;
  postMessage({ type: "loadError", message: String(error?.message || error) });
});

self.onmessage = async ({ data: msg }) => {
  try {
    await ready;
    if (loadFailure) throw loadFailure;
    let raw;
    if (msg.type === "swatch") {
      raw = ftx.knit_swatch_json(JSON.stringify(msg.payload));
    } else if (msg.type === "panel") {
      raw = ftx.knit_panel_preview_json(JSON.stringify(msg.payload));
    } else if (msg.type === "export") {
      raw = ftx.knit_export_json(JSON.stringify(msg.payload));
    } else if (msg.type === "dye") {
      raw = ftx.dye_optimize_json(JSON.stringify(msg.payload));
    } else {
      throw new Error(`Unknown knit worker command: ${msg.type}`);
    }
    const env = JSON.parse(raw);
    if (!env.ok) {
      const code = env.refusal?.code || "refused";
      const message = env.refusal?.message || "operation refused";
      postMessage({ type: "refusal", kind: msg.type, request: msg.request, code, message, refusal: env.refusal });
      return;
    }
    postMessage({ type: "result", request: msg.request, kind: msg.type, result: env.result });
  } catch (error) {
    postMessage({ type: "error", kind: msg?.type, request: msg?.request, message: String(error?.message || error) });
  }
};

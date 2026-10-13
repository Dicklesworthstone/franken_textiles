// Local-only worker: no source image or user JSON is sent to a server. The only fetches load the
// application module. One immutable traced sheet is kept until replacement or worker termination.
import init, { WebTrace } from "./pkg/ftx_wasm.js";

let sheet = null;
let loadFailure = null;
const ready = init().then(() => postMessage({ type: "ready" })).catch((error) => {
  loadFailure = error;
  postMessage({ type: "loadError", message: String(error?.message || error) });
});

function products(review) {
  try {
    const report = JSON.parse(review.report());
    const files = JSON.parse(review.names()).map((name) => ({ name, bytes: review.file(name) }));
    files.push({ name: "trace-review.zip", bytes: review.zip() });
    return { report, files };
  } finally {
    review.free();
  }
}

self.onmessage = async ({ data: msg }) => {
  try {
    await ready;
    if (loadFailure) throw loadFailure;
    let result;
    if (msg.type === "trace") {
      if (sheet) sheet.free();
      sheet = null;
      const candidate = new WebTrace(new Uint8Array(msg.bytes), msg.options);
      try {
        result = products(candidate.review("{}", "[]", "[]"));
        sheet = candidate;
      } catch (error) {
        candidate.free();
        throw error;
      }
    } else if (msg.type === "review") {
      if (!sheet) throw new Error("Trace the selected image first.");
      result = products(sheet.review(msg.names, msg.targets, msg.labels));
    } else {
      throw new Error("Unknown trace workshop command.");
    }
    postMessage({ type: "result", request: msg.request, ...result }, result.files.map((f) => f.bytes.buffer));
  } catch (error) {
    postMessage({ type: "error", request: msg.request, message: String(error?.message || error) });
  }
};
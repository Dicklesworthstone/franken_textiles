import init, * as wasm from "../workbench/pkg/ftx_wasm.js";

let ready;
const moduleUrl = new URL("../workbench/pkg/ftx_wasm_bg.wasm", import.meta.url);

async function load() {
  if (typeof DecompressionStream === "function") {
    try {
      const packed = await fetch(moduleUrl.href + ".gz");
      if (!packed.ok || !packed.body) throw Error("Compressed module unavailable.");
      const bytes = packed.body.pipeThrough(new DecompressionStream("gzip"));
      await init({ module_or_path: new Response(bytes, { headers: { "Content-Type": "application/wasm" } }) });
      return wasm;
    } catch {
      // A failed streaming compile or cache write can still succeed with a fresh buffered response.
    }
  }
  const raw = await fetch(moduleUrl, { cache: "no-store" });
  if (!raw.ok) throw Error("The studio engine could not be downloaded.");
  await init({ module_or_path: await raw.arrayBuffer() });
  return wasm;
}

export function getStudioEngine() {
  ready ??= load().catch(error => {
    ready = undefined;
    throw error;
  });
  return ready;
}

// Evaluate steady 1D layer-stack conduction using the exact native Rust/WASM consumer.
import { getStudioEngine } from "./studio-engine.mjs";

export function solveThermalSteady(ftx, request) {
  const raw = ftx.thermal_steady(JSON.stringify(request));
  return JSON.parse(raw);
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    try {
      const ftx = await getStudioEngine();
      if (latest !== data.id) return;
      const result = solveThermalSteady(ftx, data.request);
      if (latest === data.id) self.postMessage({ id: data.id, result });
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  };
}

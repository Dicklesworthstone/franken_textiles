// The threaded studio engine: the same solver built with the `threads` feature, whose element, energy and
// contact-pair evaluation runs on a pool of Web Workers (results are bit for bit the single-threaded
// engine's). It needs shared memory, which a browser grants only to a cross-origin isolated page (see
// public/_headers), so a worker loads it only when `self.crossOriginIsolated`. If it cannot start, the
// caller gets the single-threaded engine instead. Call from a worker: the pool's threads block while they wait.
import { getStudioEngine as getSingleThreaded } from "./studio-engine.mjs";

let ready;
let threads = 0;
const moduleUrl = new URL("../workbench/pkg-threads/ftx_wasm_bg.wasm", import.meta.url);

async function compressed() {
  if (typeof DecompressionStream !== "function") return moduleUrl;
  const packed = await fetch(moduleUrl.href + ".gz");
  if (!packed.ok || !packed.body) return moduleUrl;
  return new Response(packed.body.pipeThrough(new DecompressionStream("gzip")), { headers: { "Content-Type": "application/wasm" } });
}

async function load() {
  const wasm = await import("../workbench/pkg-threads/ftx_wasm.js");
  await wasm.default({ module_or_path: await compressed() });
  // One core stays with the page; past six the solve gets no faster (measured on an 8-core machine).
  const count = Math.max(1, Math.min(6, (navigator.hardwareConcurrency || 4) - 1));
  await wasm.initThreadPool(count);
  threads = count;
  return wasm;
}

/** The engine, threaded when this context can share memory and the pool starts, else single-threaded. */
export function getStudioEngine() {
  ready ??= (self.crossOriginIsolated ? load() : Promise.reject(Error("not cross-origin isolated")))
    .catch(() => {
      threads = 0;
      return getSingleThreaded();
    });
  return ready;
}

/** Solver threads in use: 0 for the single-threaded engine (known once getStudioEngine has resolved). */
export const solverThreads = () => threads;

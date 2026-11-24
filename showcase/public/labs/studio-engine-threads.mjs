// The threaded studio engine: the same solver built with the `threads` feature, whose element, energy,
// contact-pair, Hessian-assembly and sparse-factorization work runs on a pool of Web Workers (results are bit
// for bit the single-threaded engine's). It needs shared memory, which a browser grants only to a
// cross-origin isolated page (see public/_headers), so a worker loads it only when `self.crossOriginIsolated`
// and the device has cores to spare: a pool of one thread was about 5 % slower than the single-threaded
// engine (Chrome, posing-room sew). If it cannot start, the caller gets the single-threaded engine instead.
// Call from a worker: the pool's threads block while they wait.
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

// One core stays with the page; past six the solve gets no faster (measured on an 8-core machine).
const poolSize = () => Math.min(6, (navigator.hardwareConcurrency || 4) - 1);

async function load() {
  const count = poolSize();
  if (count < 2) throw Error("too few cores for a solver pool");
  const wasm = await import("../workbench/pkg-threads/ftx_wasm.js");
  await wasm.default({ module_or_path: await compressed() });
  await wasm.initThreadPool(count);
  threads = count;
  return wasm;
}

/** The engine, threaded when this context can share memory, has two cores to spare and the pool starts,
 * else single-threaded. */
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

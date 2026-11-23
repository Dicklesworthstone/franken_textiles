// The posing room's solver, off the main thread. It holds one WebDrapeSession: a garment sewn and draped
// once by the production cloth solver on a baked synthetic body, then re-draped by the solver's checked
// moving-body step each time the page asks for a new arm pose.
//
//   page -> worker   { type: "start", body, pattern, fit, settings }     (URLs and a settings object)
//                    { type: "move", id, left, right, seconds }          (degrees from vertical, cloth time)
//   worker -> page   { type: "progress", line }
//                    { type: "ready", summary, seconds, cloth, triangles, flat, display, displayTriangles,
//                      raised, frames }
//                    { type: "moved", id, left, right, samples, display, milliseconds }
//                    { type: "refused", id, message }                    (the session is where it was)
//                    { type: "failed", message }
//
// Positions are xyz in metres in the solver's frame (z up, the wearer facing -y).
import init, { WebDrapeSession } from "./pose/pkg/ftx_wasm.js";

let session = null;
// The wasm module calls this global with each of the drape's progress lines.
self.ftxProgress = line => postMessage({ type: "progress", line });

async function load() {
  const moduleUrl = new URL("./pose/pkg/ftx_wasm_bg.wasm", import.meta.url);
  if (typeof DecompressionStream === "function") {
    try {
      const packed = await fetch(moduleUrl.href + ".gz");
      if (!packed.ok || !packed.body) throw Error("Compressed module unavailable.");
      const bytes = packed.body.pipeThrough(new DecompressionStream("gzip"));
      await init({ module_or_path: new Response(bytes, { headers: { "Content-Type": "application/wasm" } }) });
      return;
    } catch {
      // Fall through to the uncompressed module.
    }
  }
  const raw = await fetch(moduleUrl, { cache: "no-store" });
  if (!raw.ok) throw Error("The solver could not be downloaded.");
  await init({ module_or_path: await raw.arrayBuffer() });
}

async function text(url) {
  const response = await fetch(url);
  if (!response.ok) throw Error(`Could not load ${url}`);
  return response.text();
}

async function start({ body, pattern, fit, settings }) {
  postMessage({ type: "progress", line: "loading the solver" });
  const [header, data, patternText, fitText] = await Promise.all([
    text(body + ".json"),
    fetch(body + ".bin").then(response => {
      if (!response.ok) throw Error("Could not load the body");
      return response.arrayBuffer();
    }),
    text(pattern), text(fit), load(),
  ]);
  postMessage({ type: "progress", line: "sewing" });
  const began = performance.now();
  session = new WebDrapeSession(header, new Uint8Array(data), patternText, fitText, JSON.stringify(settings ?? {}));
  const seconds = (performance.now() - began) / 1000;
  const summary = JSON.parse(session.summary());
  const [low, high] = summary.arm_range_deg, base = summary.base_arm_deg;
  const display = session.display(base, base);
  // The same surface with one arm fully raised, then the other: the page tells the arms apart by which
  // vertices these move.
  const raised = [session.display(high, base), session.display(base, high), session.display(low, low)];
  const cloth = session.cloth(), triangles = session.cloth_triangles(), flat = session.cloth_pattern_mm();
  const displayTriangles = session.display_triangles();
  // The sewing and draping frames of the opening fold-up, for a short replay.
  const all = session.replay(), stride = summary.cloth_vertices * 3;
  const first = summary.frames.fold + summary.frames.zip - 1;
  const frames = all.slice(first * stride);
  const header_ = JSON.parse(header);
  postMessage({ type: "ready", summary, seconds, cloth, triangles, flat, display, displayTriangles, raised, frames,
    shoulders: { left: header_.frames.arm_L.origin, right: header_.frames.arm_R.origin }, floor: header_.floor_mm },
    [cloth.buffer, triangles.buffer, flat.buffer, display.buffer, displayTriangles.buffer, frames.buffer, ...raised.map(a => a.buffer)]);
}

function move({ id, left, right, seconds }) {
  const began = performance.now();
  let samples;
  try {
    samples = session.advance(left, right, seconds);
  } catch (error) {
    postMessage({ type: "refused", id, message: String(error?.message ?? error) });
    return;
  }
  const display = session.display(left, right);
  postMessage({ type: "moved", id, left, right, samples, display, milliseconds: performance.now() - began },
    [samples.buffer, display.buffer]);
}

self.onmessage = ({ data }) => {
  if (data.type === "start") start(data).catch(error => postMessage({ type: "failed", message: String(error?.message ?? error) }));
  else if (data.type === "move" && session) move(data);
};

// The posing room's solver, off the main thread. It holds one WebDrapeSession: a garment sewn and draped
// once by the production cloth solver on a baked synthetic body, then re-draped by the solver's checked
// moving-body step each time the page asks for a new arm pose.
//
//   page -> worker   { type: "start", body, pattern, fit, settings }     (URL stems and a settings object)
//                    { type: "pose", left, right }                       (the body only, no solve)
//                    { type: "move", id, left, right, seconds }          (degrees from vertical, cloth time)
//   worker -> page   { type: "progress", line }
//                    { type: "ready", summary, seconds, cloth, triangles, flat, display, displayTriangles,
//                      raised, frames, shoulders, form }
//                    { type: "posed", left, right, display }
//                    { type: "moved", id, left, right, samples, display, milliseconds }
//                    { type: "refused", id, message }                    (the session is where it was)
//                    { type: "failed", message }
//
// Positions are xyz in metres in the solver's frame (z up, the wearer facing -y). The module is the site's
// one solver build, shared with every other lab and the workbenches.
import { getStudioEngine } from "./studio-engine.mjs";

let session = null;
// The wasm module calls this global with each of the drape's progress lines.
self.ftxProgress = line => postMessage({ type: "progress", line });

async function text(url) {
  const response = await fetch(url);
  if (!response.ok) throw Error(`Could not load ${url}`);
  return response.text();
}

async function gunzipped(url) {
  const response = await fetch(url);
  if (!response.ok || !response.body) throw Error(`Could not load ${url}`);
  return new Uint8Array(await new Response(response.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer());
}

async function start({ body, pattern, fit, settings }) {
  postMessage({ type: "progress", line: "loading the solver and the body" });
  const [wasm, header, data, patternText, fitText] = await Promise.all([
    getStudioEngine(), text(body + ".json"), gunzipped(body + ".bin.gz"), text(pattern), text(fit),
  ]);
  const began = performance.now();
  session = new wasm.WebDrapeSession(header, data, patternText, fitText, JSON.stringify(settings ?? {}));
  const seconds = (performance.now() - began) / 1000;
  const summary = JSON.parse(session.summary());
  const [low, high] = summary.arm_range_deg, base = summary.base_arm_deg;
  const display = session.display(base, base);
  // The same surface with the left arm raised, the right arm raised, and both lowered: the page tells the
  // arms apart by which vertices these move.
  const raised = [session.display(high, base), session.display(base, high), session.display(low, low)];
  const cloth = session.cloth(), triangles = session.cloth_triangles(), flat = session.cloth_pattern_mm();
  const displayTriangles = session.display_triangles();
  // The sewing and draping frames of the opening fold-up, for a short replay.
  const all = session.replay(), stride = summary.cloth_vertices * 3;
  const frames = all.slice((summary.frames.fold + summary.frames.zip - 1) * stride);
  const baked = JSON.parse(header);
  postMessage({ type: "ready", summary, seconds, cloth, triangles, flat, display, displayTriangles, raised, frames,
    shoulders: { left: baked.frames.arm_L.origin, right: baked.frames.arm_R.origin },
    form: { floorMm: baked.floor_mm, cutAboveFloorMm: baked.trunk_cut_above_floor_mm ?? null } },
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
  const [reachedLeft, reachedRight] = [left, right];
  const display = session.display(reachedLeft, reachedRight);
  postMessage({ type: "moved", id, left: reachedLeft, right: reachedRight, samples, display, milliseconds: performance.now() - began },
    [samples.buffer, display.buffer]);
}

self.onmessage = ({ data }) => {
  if (data.type === "start") {
    start(data).catch(error => postMessage({ type: "failed", message: String(error?.message ?? error) }));
  } else if (session && data.type === "pose") {
    const display = session.display(data.left, data.right);
    postMessage({ type: "posed", left: data.left, right: data.right, display }, [display.buffer]);
  } else if (session && data.type === "move") {
    move(data);
  }
};

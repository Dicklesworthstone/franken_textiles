// The posing room's solver, off the main thread: it sews and drapes the bolero onto the baked form with
// the arms in the pose the page asks for, using the production cloth solver compiled to WebAssembly. The
// page poses the form itself (it reads the same baked body); this worker only does the cloth. After a sew
// the worker keeps that session, so the page can move the arms under the sewn jacket with the solver's
// checked moving-body step instead of sewing it on again.
//
//   page -> worker   { type: "sew", body, pattern, fit, settings, left, right }   (URL stems, settings, degrees)
//                    { type: "move", left, right, seconds }   (from the session's present pose, over `seconds`)
//   worker -> page   { type: "progress", line }
//                    { type: "sewn", left, right, seconds, summary, cloth, triangles, flat, frames }
//                    { type: "failed", left, right, message }
//                    { type: "moved", left, right, seconds, samples }   (samples: [t, xyz...] per accepted substep)
//                    { type: "move-failed", left, right, message }   (the session is unchanged)
//
// A page that wants another pose while a sew is running terminates this worker and starts a new one.
// Positions are xyz in metres in the solver's frame (z up, the wearer facing -y).
// The threaded engine where the page is cross-origin isolated (studio-engine-threads.mjs falls back to the
// single-threaded one by itself).
import { getStudioEngine, solverThreads } from "./studio-engine-threads.mjs";

self.ftxProgress = line => postMessage({ type: "progress", line });

const cache = new Map();
async function load(url, read) {
  if (!cache.has(url)) {
    cache.set(url, fetch(url).then(async response => {
      if (!response.ok || !response.body) throw Error(`Could not load ${url}`);
      return read(response);
    }));
  }
  return cache.get(url);
}
const text = url => load(url, response => response.text());
const gunzipped = url => load(url, async response =>
  new Uint8Array(await new Response(response.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer()));

// The last sewn session, kept for moves.
let session = null;

async function sew({ body, pattern, fit, settings, left, right }) {
  postMessage({ type: "progress", line: "loading the solver" });
  const [wasm, header, data, patternText, fitText] = await Promise.all([
    getStudioEngine(), text(body + ".json"), gunzipped(body + ".bin.gz"), text(pattern), text(fit),
  ]);
  const threads = solverThreads();
  postMessage({ type: "progress", line: threads > 1 ? `solver ready on ${threads} threads` : "solver ready" });
  postMessage({ type: "progress", line: `placing the pieces round the arms at ${left}° and ${right}°` });
  const began = performance.now();
  const sewn = wasm.WebDrapeSession.posed(header, data, patternText, fitText, JSON.stringify(settings ?? {}), left, right);
  const seconds = (performance.now() - began) / 1000;
  const summary = JSON.parse(sewn.summary());
  const cloth = sewn.cloth(), triangles = sewn.cloth_triangles(), flat = sewn.cloth_pattern_mm();
  // The whole fold-up, from the flat pieces on the board to the hung jacket.
  const frames = sewn.replay();
  session?.free();
  session = sewn;
  postMessage({ type: "sewn", left, right, seconds, summary, cloth, triangles, flat, frames },
    [cloth.buffer, triangles.buffer, flat.buffer, frames.buffer]);
}

function move({ left, right, seconds }) {
  if (!session) {
    postMessage({ type: "move-failed", left, right, message: "nothing is sewn yet" });
    return;
  }
  const began = performance.now();
  try {
    const samples = session.advance(left, right, seconds);
    postMessage({ type: "moved", left, right, seconds: (performance.now() - began) / 1000, samples }, [samples.buffer]);
  } catch (error) {
    postMessage({ type: "move-failed", left, right, message: String(error?.message ?? error) });
  }
}

self.onmessage = ({ data }) => {
  if (data.type === "sew") {
    sew(data).catch(error => postMessage({ type: "failed", left: data.left, right: data.right, message: String(error?.message ?? error) }));
  } else if (data.type === "move") {
    move(data);
  }
};

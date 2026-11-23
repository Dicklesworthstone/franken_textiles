// The posing room's solver, off the main thread: it sews and drapes the bolero onto the baked form with
// the arms in the pose the page asks for, using the production cloth solver compiled to WebAssembly. The
// page poses the form itself (it reads the same baked body); this worker only does the cloth.
//
//   page -> worker   { type: "sew", body, pattern, fit, settings, left, right }   (URL stems, settings, degrees)
//   worker -> page   { type: "progress", line }
//                    { type: "sewn", left, right, seconds, summary, cloth, triangles, flat, frames }
//                    { type: "failed", left, right, message }
//
// A page that wants another pose while a sew is running terminates this worker and starts a new one.
// Positions are xyz in metres in the solver's frame (z up, the wearer facing -y).
import { getStudioEngine } from "./studio-engine.mjs";

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

async function sew({ body, pattern, fit, settings, left, right }) {
  postMessage({ type: "progress", line: "loading the solver" });
  const [wasm, header, data, patternText, fitText] = await Promise.all([
    getStudioEngine(), text(body + ".json"), gunzipped(body + ".bin.gz"), text(pattern), text(fit),
  ]);
  postMessage({ type: "progress", line: `placing the pieces round the arms at ${left}° and ${right}°` });
  const began = performance.now();
  const session = wasm.WebDrapeSession.posed(header, data, patternText, fitText, JSON.stringify(settings ?? {}), left, right);
  const seconds = (performance.now() - began) / 1000;
  const summary = JSON.parse(session.summary());
  const cloth = session.cloth(), triangles = session.cloth_triangles(), flat = session.cloth_pattern_mm();
  // The whole fold-up, from the flat pieces on the board to the hung jacket.
  const frames = session.replay();
  session.free();
  postMessage({ type: "sewn", left, right, seconds, summary, cloth, triangles, flat, frames },
    [cloth.buffer, triangles.buffer, flat.buffer, frames.buffer]);
}

self.onmessage = ({ data }) => {
  if (data.type === "sew") {
    sew(data).catch(error => postMessage({ type: "failed", left: data.left, right: data.right, message: String(error?.message ?? error) }));
  }
};

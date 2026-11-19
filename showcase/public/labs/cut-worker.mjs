// The small initial marker and every live edit use this same browser-portable pipeline.
import { getStudioEngine } from "./studio-engine.mjs";

export const initialCut = { garment: "zouave_bolero", material: "dharma/cotton-muslin-3.5oz", copies: 2 };
const patterns = new Map();

export function planCut(ftx, { garment, material, copies, pattern: suppliedPattern }) {
  let pattern = suppliedPattern ?? patterns.get(garment);
  if (suppliedPattern !== undefined && (typeof suppliedPattern !== "string" || !suppliedPattern.length)) {
    throw Error("The transferred draft is empty.");
  }
  if (!pattern) {
    const request = { body: { mode: "chart", stature_cm: 92 }, program: garment, style: {} };
    const round = ftx.run(JSON.stringify(request), "2026-10-12", "2026-10-12T00:00:00Z");
    try {
      pattern = new TextDecoder().decode(round.file("pattern.json"));
      if (!pattern) throw Error("The size-chart draft produced no pattern.");
      patterns.set(garment, pattern);
    } finally {
      round.free();
    }
  }
  const changes = { schema: "ftx-material-bindings/1", default: { id: material }, pieces: {} };
  const { pattern_json } = JSON.parse(ftx.material_bind(pattern, JSON.stringify(changes)));
  const options = { copies, gap_mm: 4, edge_margin_mm: 10, end_margin_mm: 10,
    nesting: { passes: 2, work_limit: 2500000 } };
  const job = JSON.parse(ftx.roll_job(pattern_json, JSON.stringify(options), 0));
  if (!job.preview_svg) throw Error(job.preview_refusal?.message || "The marker preview was refused.");
  const plan = JSON.parse(job.plan_json);
  if (plan.rolls.length !== 1) throw Error("This studio displays a single-material marker.");
  const roll = plan.rolls[0];
  const record = roll.material.authored;
  return {
    garment, material, copies, width: roll.width_m * 1000, length: roll.length_m * 1000,
    yield: roll.utilization, area: roll.external_cut_area_m2,
    source: record.sources[record.properties.width.source].reference,
    bounds: roll.placements.map(p => p.bounds_mm),
    svg: job.preview_svg,
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    try {
      const ftx = await getStudioEngine();
      if (latest === data.id) self.postMessage({ id: data.id, result: planCut(ftx, data) });
    } catch (error) {
      self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
    }
  };
}

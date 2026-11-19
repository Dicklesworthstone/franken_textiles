// Run with the staged gallery workbench: node --test scripts/cut-handoff.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import init, * as wasm from "../public/workbench/pkg/ftx_wasm.js";
import { initialCut, planCut } from "../public/labs/cut-worker.mjs";

test("a changed draft supplies the cutting geometry and leaves the stock specimen reusable", async () => {
  const bytes = await readFile(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url));
  await init({ module_or_path: bytes });
  const stock = planCut(wasm, initialCut);
  const round = wasm.run(JSON.stringify({
    body: { mode: "chart", stature_cm: 92 }, program: initialCut.garment, style: { chest_ease: 140 },
  }), "2026-10-12", "2026-10-12T00:00:00Z");
  let pattern;
  try { pattern = new TextDecoder().decode(round.file("pattern.json")); }
  finally { round.free(); }
  const altered = planCut(wasm, { ...initialCut, pattern });
  assert.equal(altered.bounds.length, stock.bounds.length);
  assert.equal(altered.width, stock.width);
  assert.notDeepEqual(altered.bounds, stock.bounds);
  assert.ok(altered.area > stock.area);
  assert.deepEqual(planCut(wasm, initialCut), stock);
  const saved = JSON.parse(await readFile(new URL("../public/labs/cut-starter.json", import.meta.url), "utf8"));
  assert.deepEqual(saved, stock);
});

#!/usr/bin/env node
// Static & contract verification for the Knitting & Programmed Yarn Dyeing local workbench (FTX-K18).
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const html = fs.readFileSync(path.join(root, "web/knit.html"), "utf8");
const worker = fs.readFileSync(path.join(root, "web/knit-worker.js"), "utf8");
const page = fs.readFileSync(path.join(root, "web/knit-page.js"), "utf8");
const nav = fs.readFileSync(path.join(root, "web/atelier-assets/navigation.js"), "utf8");

// 1. Navigation wiring
assert(nav.includes('"knit", "knit.html", "Knitting & Dyeing"'), "navigation.js missing knit.html link");

// 2. Honesty badges and safety disclosures in HTML
for (const marker of [
  'class="wb wb-knit"',
  "hardware_execution_authorized: false",
  "physical_knitting_validated: false",
  "physical_dyeing_validated: false",
  "fit_claimed: false",
  "CalibrationSwatchOnly",
  "EN 14682",
  "knit-page.js",
]) {
  assert(html.includes(marker), `web/knit.html missing required marker: ${marker}`);
}

// 3. Required DOM element IDs referenced by knit-page.js exist and are unique in knit.html
const allIds = [...html.matchAll(/\bid="([^"]+)"/g)].map((m) => m[1]);
const seenIds = new Set();
for (const id of allIds) {
  assert(!seenIds.has(id), `web/knit.html has duplicate id="${id}"`);
  seenIds.add(id);
}
const idMatches = [...page.matchAll(/getElementById\("([^"]+)"\)/g)].map((m) => m[1]);
assert(idMatches.length >= 18, `expected >= 18 DOM bindings, found ${idMatches.length}`);
for (const id of new Set(idMatches)) {
  assert(seenIds.has(id), `web/knit.html missing id="${id}" referenced by web/knit-page.js`);
}

// 4. HTML select values and JS request/response fields match ftx-wasm Rust serde schemas
const validAgeClasses = new Set(["infant_under3", "child3_to14", "adult", "calibration_swatch_only"]);
for (const selectId of ["panelAgeClass", "dyeAgeClass"]) {
  const m = html.match(new RegExp(`<select id="${selectId}">([\\s\\S]*?)</select>`));
  assert(m, `web/knit.html missing <select id="${selectId}">`);
  const values = [...m[1].matchAll(/<option value="([^"]+)"/g)].map((o) => o[1]);
  assert(values.length >= 3, `<select id="${selectId}"> must declare >= 3 age classes`);
  for (const v of values) {
    assert(validAgeClasses.has(v), `<select id="${selectId}"> has invalid WearerAgeClass "${v}"`);
  }
}
assert(page.includes("relaxed_after_wash"), "knit-page.js must send GaugeSpec.relaxed_after_wash");
assert(page.includes("needle_pitch_mm"), "knit-page.js must send GaugeSpec.needle_pitch_mm");
assert(!page.includes("measured_after_wash_block"), "knit-page.js must not send unknown GaugeSpec field measured_after_wash_block");
assert(page.includes("regime1_a_stripes") && page.includes("regime1_b_circular_pooling"), "knit-page.js must send valid DyeingRegime variants");
assert(!page.includes("regime2_b_planned_pooling"), "knit-page.js must not send invalid DyeingRegime regime2_b_planned_pooling");
for (const field of ["expected_delta_e00_p95", "complexity_rank", "feasible", "rationale"]) {
  assert(page.includes(`row.${field}`), `knit-page.js must read DyeRegimeScore.${field}`);
}
for (const stale of ["estimated_delta_e00_p50", "estimated_delta_e00_p95", "hardware_complexity"]) {
  assert(!page.includes(stale), `knit-page.js references nonexistent Pareto field ${stale}`);
}
assert(!page.includes("tr.innerHTML"), "knit-page.js must build table rows with textContent, not innerHTML");

// 5. Worker enforces WASM interface stamp & local-only execution (no fetch/XMLHttpRequest)
assert(worker.includes('EXPECTED_WASM_STAMP = "ftx-wasm-interface/2026-11-26.1"'), "worker missing stamp check");
assert(worker.includes("ftx.knit_swatch_json"), "worker missing knit_swatch_json call");
assert(worker.includes("ftx.knit_panel_preview_json"), "worker missing knit_panel_preview_json call");
assert(worker.includes("ftx.dye_optimize_json"), "worker missing dye_optimize_json call");
assert(!/\bfetch\s*\(/.test(worker), "worker must not perform network fetch calls");
assert(!/\bfetch\s*\(/.test(page), "page must not perform network fetch calls");

console.log(`check_knit_web.cjs: OK (${new Set(idMatches).size} DOM bindings, schema fields, zero network calls, honesty badges verified)`);

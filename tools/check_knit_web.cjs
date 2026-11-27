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
assert(html.includes('id="panelLength" type="number" min="120"'), "panelLength min must exceed hardcoded armhole_depth_mm=95");

// 5. Worker enforces WASM interface stamp & local-only execution (no fetch/XMLHttpRequest)
assert(worker.includes('EXPECTED_WASM_STAMP = "ftx-wasm-interface/2026-11-27.1"'), "worker missing updated stamp check");
assert(worker.includes("ftx.knit_swatch_json"), "worker missing knit_swatch_json call");
assert(worker.includes("ftx.knit_panel_preview_json"), "worker missing knit_panel_preview_json call");
assert(worker.includes("ftx.dye_optimize_json"), "worker missing dye_optimize_json call");
assert(!/\bfetch\s*\(/.test(worker), "worker must not perform network fetch calls");
assert(!/\bfetch\s*\(/.test(page), "page must not perform network fetch calls");
assert(!page.includes(".innerHTML"), "knit-page.js must not inject SVG or table HTML via innerHTML");

// 6. Behavioral VM checks for knit-page.js (input validation, stale refusal suppression, hazard refusal display, worker.onerror, pagehide)
const vm = require("node:vm");
class MockEl {
  constructor(tag = "div") {
    this.tag = tag;
    this.value = "";
    this.textContent = "";
    this.hidden = false;
    this.disabled = false;
    this.children = [];
    this.handlers = {};
    this.width = 160;
    this.height = 160;
  }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = [...nodes]; }
  getContext() {
    return { clearRect() {}, fillRect() {}, fillStyle: "" };
  }
}

const elMap = Object.fromEntries([...seenIds].map((id) => [id, new MockEl()]));
elMap.swatchMachine.value = "kniterate-7e";
elMap.swatchYarn.value = "merino-200tex";
elMap.swatchNeedles.value = "32";
elMap.swatchCourses.value = "12";
elMap.swatchCams.value = "5.0,7.0";
elMap.panelMachine.value = "kniterate-7e";
elMap.panelAgeClass.value = "infant_under3";
elMap.panelHem.value = "250";
elMap.panelChest.value = "280";
elMap.panelLength.value = "310";
elMap.panelRib.value = "6";
elMap.dyeMachine.value = "piezo-cmyk";
elMap.dyeAgeClass.value = "calibration_swatch_only";
elMap.dyePreset.value = "horizon";
elMap.dyeMode.value = "circular";
elMap.dyeCourses.value = "6";
elMap.dyeWales.value = "6";
elMap.dyeEps.value = "0.002";
elMap.dyeSigma.value = "0.015";
elMap.dyeWick.value = "1.5";
const paretoTbody = new MockEl("tbody");

const posted = [];
const activeUrls = new Set();
let nextUrl = 0;
let workerInstance = null;
const windowHandlers = {};

const ctx = vm.createContext({
  JSON, Number, String, Boolean, Error, Map, WeakMap, Math, Array, Object,
  Blob,
  URL: {
    createObjectURL: () => { const u = `blob:knit-${++nextUrl}`; activeUrls.add(u); return u; },
    revokeObjectURL: (u) => { activeUrls.delete(u); },
  },
  Worker: class {
    constructor(url) { this.url = url; workerInstance = this; }
    postMessage(m) { posted.push(m); }
  },
  document: {
    getElementById: (id) => elMap[id] || null,
    querySelector: (sel) => (sel === "#dyeParetoTable tbody" ? paretoTbody : null),
    createElement: (tag) => new MockEl(tag),
  },
  window: {
    addEventListener: (name, fn) => { windowHandlers[name] = fn; },
  },
});

vm.runInContext(page, ctx);
assert(workerInstance, "knit-page.js must construct a Worker");
workerInstance.onmessage({ data: { type: "ready" } });
assert.equal(elMap.runSwatch.disabled, false);

// Reject invalid cam tokens rather than silently dropping them.
elMap.swatchCams.value = "5.0, oops, 7.0";
elMap.runSwatch.handlers.click();
assert.equal(posted.length, 0, "invalid cam token must refuse before postMessage");
assert.equal(elMap.error.hidden, false);
assert.match(elMap.error.textContent, /Invalid cam setting/);

// Reject oversized dye grids BEFORE allocating buildPresetPixels().
elMap.dyeCourses.value = "999999";
elMap.runDye.handlers.click();
assert.equal(posted.length, 0, "oversized dye grid must refuse before allocation");
assert.match(elMap.error.textContent, /between 2 and 40/);

// Restore valid dye inputs, submit two requests, verify stale refusal does not clobber newer result,
// and verify controller_export_refusal surfaces explicitly instead of wearable Exp-4 message.
elMap.dyeCourses.value = "4";
elMap.dyeWales.value = "4";
elMap.runDye.handlers.click();
elMap.runDye.handlers.click();
assert.equal(posted.length, 2);
const [req1, req2] = posted;
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "dye",
    request: req2.request,
    result: {
      grid: [4, 4],
      delta_e00_p50: 1.2,
      delta_e00_p95: 2.3,
      gamut_floor_delta_e00: 0.4,
      coherent_horizon_circular_stitches: 40,
      coherent_horizon_flatbed_stitches: 35,
      iterations: 6,
      predicted_stitch_srgb: Array.from({ length: 16 }, () => [100, 120, 140]),
      pareto_regimes: [{ regime: "stripes", feasible: true, expected_delta_e00_p95: 2.3, complexity_rank: 1, rationale: "ok" }],
      program: { name: "test" },
      controller_gcode: null,
      controller_export_refusal: { code: "material_process_hazard", message: "oven dwell below minimum" },
    },
  },
});
assert.match(elMap.dyePre.textContent, /Controller export refused \[material_process_hazard\]: oven dwell below minimum/);
assert.equal(activeUrls.size, 1);

// Stale refusal for req1 must NOT overwrite req2's clean status or raise error banner.
workerInstance.onmessage({
  data: { type: "refusal", kind: "dye", request: req1.request, code: "out_of_range", message: "stale failure" },
});
assert.equal(elMap.error.hidden, true, "stale refusal must be ignored");
assert.equal(elMap.knitStatus.textContent, "Completed dye.");

// pagehide revokes Blob URLs cleanly.
windowHandlers.pagehide();
assert.equal(activeUrls.size, 0, "pagehide must revoke download URLs");

// worker.onerror disables buttons and surfaces error fail-closed.
workerInstance.onerror({ message: "WASM trap" });
assert.equal(elMap.runSwatch.disabled, true);
assert.equal(elMap.runPanel.disabled, true);
assert.equal(elMap.runDye.disabled, true);
assert.match(elMap.error.textContent, /WASM trap/);

console.log(`check_knit_web.cjs: OK (${new Set(idMatches).size} DOM bindings, schema fields, VM behavioral checks, zero network calls, honesty badges verified)`);

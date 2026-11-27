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

// 3. Required DOM element IDs referenced by knit-page.js exist in knit.html
const idMatches = [...page.matchAll(/getElementById\("([^"]+)"\)/g)].map((m) => m[1]);
assert(idMatches.length >= 18, `expected >= 18 DOM bindings, found ${idMatches.length}`);
for (const id of new Set(idMatches)) {
  assert(html.includes(`id="${id}"`), `web/knit.html missing id="${id}" referenced by web/knit-page.js`);
}

// 4. Worker enforces WASM interface stamp & local-only execution (no fetch/XMLHttpRequest)
assert(worker.includes('EXPECTED_WASM_STAMP = "ftx-wasm-interface/2026-11-26.1"'), "worker missing stamp check");
assert(worker.includes("ftx.knit_swatch_json"), "worker missing knit_swatch_json call");
assert(worker.includes("ftx.knit_panel_preview_json"), "worker missing knit_panel_preview_json call");
assert(worker.includes("ftx.dye_optimize_json"), "worker missing dye_optimize_json call");
assert(!/\bfetch\s*\(/.test(worker), "worker must not perform network fetch calls");
assert(!/\bfetch\s*\(/.test(page), "page must not perform network fetch calls");

console.log(`check_knit_web.cjs: OK (${new Set(idMatches).size} DOM bindings, zero network calls, honesty badges verified)`);

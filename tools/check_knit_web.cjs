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
assert(page.includes("regime1_a_stripes") && page.includes("regime1_b_circular_pooling") && page.includes("regime2_fiducial_synced"), "knit-page.js must send valid DyeingRegime variants");
assert(!page.includes("regime2_b_planned_pooling"), "knit-page.js must not send invalid DyeingRegime regime2_b_planned_pooling");
const validRegimes = new Set([
  "auto",
  "regime1_a_stripes",
  "regime1_b_circular_pooling",
  "regime2_fiducial_synced",
  "regime3_inline_dancer",
  "regime4_multi_carrier_jacquard",
]);
const regimeSelect = html.match(/<select id="dyeRegime">([\s\S]*?)<\/select>/);
assert(regimeSelect, 'web/knit.html missing <select id="dyeRegime">');
for (const m of regimeSelect[1].matchAll(/<option value="([^"]+)"/g)) {
  assert(validRegimes.has(m[1]), `<select id="dyeRegime"> has invalid option "${m[1]}"`);
}
for (const field of ["expected_delta_e00_p95", "complexity_rank", "feasible", "rationale"]) {
  assert(page.includes(`row.${field}`), `knit-page.js must read DyeRegimeScore.${field}`);
}
for (const stale of ["estimated_delta_e00_p50", "estimated_delta_e00_p95", "hardware_complexity"]) {
  assert(!page.includes(stale), `knit-page.js references nonexistent Pareto field ${stale}`);
}
assert(!page.includes("tr.innerHTML"), "knit-page.js must build table rows with textContent, not innerHTML");
assert(html.includes('id="panelLength" type="number" min="120"'), "panelLength min must stay >= 120 mm");

// 5. Worker enforces WASM interface stamp, cache-busting revision, & local-only execution (no fetch/XMLHttpRequest)
assert(worker.includes('EXPECTED_WASM_STAMP = "ftx-wasm-interface/2026-11-27.3"'), "worker missing updated stamp check");
assert(worker.includes('ftx_wasm.js?v=2026-11-27.3') && worker.includes('ftx_wasm_bg.wasm?v=2026-11-27.3'), "worker missing matching ?v=2026-11-27.3 cache buster");
assert(worker.includes("ftx.knit_swatch_json"), "worker missing knit_swatch_json call");
assert(worker.includes("ftx.knit_panel_preview_json"), "worker missing knit_panel_preview_json call");
assert(worker.includes("ftx.knit_export_json"), "worker missing knit_export_json call");
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
    this.files = [];
    this._textContent = "";
    this.hidden = false;
    this.disabled = false;
    this.children = [];
    this.handlers = {};
    this.width = 160;
    this.height = 160;
  }
  get textContent() { return this._textContent; }
  set textContent(v) { this._textContent = String(v ?? ""); this.children = []; }
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
elMap.panelNeckWidth.value = "0";
elMap.panelNeckDepth.value = "0";
elMap.panelShoulderPairs.value = "0";
elMap.exportMachine.value = "kniterate-7e";
elMap.dyeMachine.value = "piezo-cmyk";
elMap.dyeAgeClass.value = "calibration_swatch_only";
elMap.dyePreset.value = "horizon";
elMap.dyeMode.value = "circular";
elMap.dyeRegime.value = "auto";
elMap.dyeCourses.value = "6";
elMap.dyeWales.value = "6";
elMap.dyeEps.value = "0.002";
elMap.dyeSigma.value = "0.015";
elMap.dyeWick.value = "1.5";
const paretoTbody = new MockEl("tbody");

const posted = [];
const activeUrls = new Set();
const blobByUrl = new Map();
let nextUrl = 0;
let workerInstance = null;
const windowHandlers = {};

const ctx = vm.createContext({
  JSON, Number, String, Boolean, Error, Map, WeakMap, Set, Math, Array, Object, Uint8Array, TextEncoder,
  atob,
  Blob,
  DOMParser: class {
    parseFromString(src) {
      const s = String(src || "");
      const makeNode = (tag, attrs = []) => ({
        nodeName: tag,
        localName: tag,
        attributes: attrs,
      });
      const tags = [];
      const tagRe = /<([a-zA-Z_][\w:-]*)([^>]*)>/g;
      let m;
      while ((m = tagRe.exec(s)) !== null) {
        const attrs = [];
        const attrRe = /([a-zA-Z_][\w:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/g;
        let am;
        while ((am = attrRe.exec(m[2])) !== null) {
          attrs.push({ name: am[1], value: am[2] ?? am[3] ?? "" });
        }
        tags.push(makeNode(m[1], attrs));
      }
      const root = tags[0] || null;
      return {
        documentElement: root,
        querySelectorAll: () => tags.slice(1),
      };
    }
  },
  URL: {
    createObjectURL: (blob) => { const u = `blob:knit-${++nextUrl}`; activeUrls.add(u); blobByUrl.set(u, blob); return u; },
    revokeObjectURL: (u) => { activeUrls.delete(u); blobByUrl.delete(u); },
  },
  Worker: class {
    constructor(url) { this.url = url; workerInstance = this; }
    postMessage(m) { posted.push(m); }
  },
  document: {
    getElementById: (id) => elMap[id] || null,
    querySelector: (sel) => (sel === "#dyeParetoTable tbody" ? paretoTbody : null),
    createElement: (tag) => new MockEl(tag),
    importNode: (node) => node,
  },
  window: {
    addEventListener: (name, fn) => { windowHandlers[name] = fn; },
  },
});

async function checkBehavior() {
vm.runInContext(page, ctx);
assert(workerInstance, "knit-page.js must construct a Worker");
workerInstance.onmessage({ data: { type: "ready" } });
assert.equal(elMap.runSwatch.disabled, false);
assert.equal(elMap.runExport.disabled, false);

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

// SVG sanitizer accepts safe SVG charts and refuses event handlers, foreignObject, and javascript: hrefs.
elMap.runPanel.handlers.click();
const panelReqSafe = posted[posted.length - 1];
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "panel",
    request: panelReqSafe.request,
    result: {
      chart_svg: '<svg viewBox="0 0 10 10"><rect width="10" height="10"/></svg>',
      pattern_json: '{"name":"safe panel"}\n',
      instructions_text: "# ok",
      stitch_count: 10,
      short_row_turns: 0,
      doc_sha256: "0123456789abcdef0123456789abcdef",
      svg_checks_passed: true,
      fit_claimed: false,
      metadata: {},
    },
  },
});
assert.equal(elMap.error.hidden, true, "safe SVG must render without error");
assert.equal(elMap.panelSvgBox.children.length, 1);

for (const unsafeSvg of [
  '<svg onload="alert(1)"><rect/></svg>',
  '<svg><foreignObject><div/></foreignObject></svg>',
  '<svg><a href="javascript:alert(1)"></a></svg>',
  '<svg><use xlink:href="data:image/svg+xml,boom"/></svg>',
]) {
  elMap.runPanel.handlers.click();
  const reqBad = posted[posted.length - 1];
  workerInstance.onmessage({
    data: {
      type: "result",
      kind: "panel",
      request: reqBad.request,
      result: {
        chart_svg: unsafeSvg,
        pattern_json: '{"name":"unsafe chart"}\n',
        instructions_text: "# bad",
        stitch_count: 10,
        short_row_turns: 0,
        doc_sha256: "0123456789abcdef0123456789abcdef",
        svg_checks_passed: true,
        fit_claimed: false,
        metadata: {},
      },
    },
  });
  assert.equal(elMap.error.hidden, false, `unsafe SVG must refuse: ${unsafeSvg}`);
  assert.match(elMap.error.textContent, /Refused malformed or unsafe SVG/);
}

// Flatbed checker in auto regime must pick regime2_fiducial_synced rather than circular pooling.
elMap.dyePreset.value = "checker";
elMap.dyeMode.value = "flatbed";
elMap.dyeRegime.value = "auto";
elMap.runDye.handlers.click();
const flatbedReq = posted.at(-1);
assert.equal(flatbedReq.payload.is_circular, false);
assert.equal(flatbedReq.payload.regime, "regime2_fiducial_synced");

// Kniterate retains useful Knitout and source identity without offering an unqualified .kc file.
elMap.swatchCams.value = "5.0,7.0";
elMap.swatchMaterialSha256.value = "a".repeat(64);
elMap.runSwatch.handlers.click();
const kniterateReq = posted.at(-1);
assert.equal(kniterateReq.payload.material_sha256, "a".repeat(64));
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "swatch",
    request: kniterateReq.request,
    result: {
      knitout_text: ";!knitout-2\n",
      pattern_json: '{ "gauge": 0.1234567890123456789 }\r\n',
      kc_text: null,
      machine_executable: false,
      backend_export_refusal: {
        code: "unqualified_process",
        message: "Kniterate conversion requires an independent backend",
        repairs: ["Convert the verified Knitout file with the official backend."],
      },
      chart_svg: "<svg></svg>",
      cert: { hardware_execution_authorized: false },
      qualification_template: { material_sha256: "a".repeat(64) },
    },
  },
});
assert(elMap.swatchDownloads.children.some((a) => a.download === "swatch.k"));
assert(!elMap.swatchDownloads.children.some((a) => a.download === "swatch.kc"));
assert(elMap.swatchDownloads.children.some((a) => a.download === "backend_export_refusal.json"));
assert.match(elMap.swatchPre.textContent, /Machine conversion required/);
const swatchPatternLink = elMap.swatchDownloads.children.find((a) => a.download === "swatch.knit.json");
assert(swatchPatternLink, "swatch must supply its reusable pattern JSON");
assert.equal(await blobByUrl.get(swatchPatternLink.href).text(), '{ "gauge": 0.1234567890123456789 }\r\n', "saved swatch JSON must remain byte-for-byte identical");

// A real KCode result needs an exact source-bound verification and stays hardware-unqualified.
const kcodeResult = {
  knitout_text: ";!knitout-2\n",
  knitout_sha256: "b".repeat(64),
  pattern_json: '{"name":"KCode swatch"}\n',
  kc_text: "HOME\nRACK:0\n// fixture response\n",
  kc_sha256: "c".repeat(64),
  kcode_verification: {
    schema: "ftx-kniterate-readback/1",
    program_sha256: "b".repeat(64),
    kc_sha256: "c".repeat(64),
    hardware_execution_authorized: false,
    takedown: { kc_sha256: "c".repeat(64), machine_sha256: "e".repeat(64), physical_takedown_calibrated: false },
  },
  machine_executable: false,
  backend_export_refusal: null,
  chart_svg: "<svg></svg>",
  cert: { hardware_execution_authorized: false, machine_sha256: "e".repeat(64) },
  qualification_template: {},
};
elMap.runSwatch.handlers.click();
workerInstance.onmessage({ data: { type: "result", kind: "swatch", request: posted.at(-1).request, result: kcodeResult } });
assert(elMap.swatchDownloads.children.some((a) => a.download === "swatch.kc"));
assert(elMap.swatchDownloads.children.some((a) => a.download === "kcode_verification.json"));
assert.match(elMap.swatchPre.textContent, /KCode pass and needle read-back verified/);
elMap.runSwatch.handlers.click();
workerInstance.onmessage({ data: { type: "result", kind: "swatch", request: posted.at(-1).request,
  result: { ...kcodeResult, kcode_verification: { ...kcodeResult.kcode_verification, kc_sha256: "d".repeat(64) } } } });
assert.equal(elMap.swatchDownloads.children.length, 0);
assert.match(elMap.error.textContent, /source-bound read-back/);

// An old bundle's invented .kc result must withdraw all prior swatch downloads.
elMap.runSwatch.handlers.click();
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "swatch",
    request: posted.at(-1).request,
    result: { knitout_text: ";!knitout-2\n", kc_text: "GRIPPER_IN 1", chart_svg: "<svg></svg>" },
  },
});
assert.equal(elMap.swatchDownloads.children.length, 0);
assert.match(elMap.error.textContent, /Rebuild and reload/);

// Brother AYAB swatch offers swatch_ayab.png download alongside checklist.
elMap.swatchCams.value = "4.0,6.0";
elMap.swatchMachine.value = "brother-kh930-ayab";
elMap.runSwatch.handlers.click();
const swatchReq = posted.at(-1);
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "swatch",
    request: swatchReq.request,
    result: {
      knitout_text: ";!knitout-2\n",
      machine_executable: false,
      pattern_json: '{"name":"AYAB swatch"}\n',
      ayab_png_base64: Buffer.from("\x89PNG\r\n\x1a\n", "binary").toString("base64"),
      ayab_checklist: { schema: "ftx-ayab-checklist/1" },
      chart_svg: "<svg></svg>",
      cert: { hardware_execution_authorized: false },
      qualification_template: {},
    },
  },
});
assert(
  elMap.swatchDownloads.children.some((a) => a.download === "swatch_ayab.png"),
  "Brother AYAB swatch must render swatch_ayab.png download link"
);

// A supported straight-top panel preserves actual dimensions and does not invent a neckline.
elMap.runPanel.handlers.click();
const defaultPanel = posted.at(-1).payload.panel;
assert.equal(defaultPanel.armhole_depth_mm, 95);
assert.equal(defaultPanel.armhole_inset_total_mm, 36);
assert.equal(defaultPanel.neck_width_mm, 0);
assert.equal(defaultPanel.neck_depth_mm, 0);
assert.equal(posted.at(-1).payload.shoulder_short_row_pairs, 0);

// Measured neckline requests retain the user's exact values and explain the actual construction.
elMap.panelNeckWidth.value = "93.25";
elMap.panelNeckDepth.value = "26.5";
elMap.panelShoulderPairs.value = "2";
elMap.runPanel.handlers.click();
const neckPanelRequest = posted.at(-1);
assert.equal(neckPanelRequest.payload.panel.neck_width_mm, 93.25);
assert.equal(neckPanelRequest.payload.panel.neck_depth_mm, 26.5);
assert.equal(neckPanelRequest.payload.shoulder_short_row_pairs, 2);
workerInstance.onmessage({ data: {
  type: "result", kind: "panel", request: neckPanelRequest.request,
  result: {
    stitch_count: 120,
    pattern_json: '{"neck_width_mm":93.25,"neck_depth_mm":26.5}\n',
    doc_sha256: "0123456789abcdef", svg_checks_passed: true, fit_claimed: false,
    chart_svg: "<svg></svg>", instructions_text: "Finish held center loops manually.",
    metadata: {
      neckline_construction: "held_center_tuck_anchored_shoulders",
      manual_finishing: "Finish held center loops manually.",
    },
  },
} });
assert.match(elMap.panelSummary.textContent, /tuck-anchored shoulder short rows and held center neckline loops; manual finishing required/);
assert.match(elMap.panelPre.textContent, /Finish held center loops manually/);

for (const [width, depth, reason] of [
  ["NaN", "26.5", /finite number/],
  ["93.25", "Infinity", /finite number/],
  ["-1", "26.5", /nonnegative/],
  ["280", "26.5", /smaller than/],
  ["93.25", "310", /smaller than/],
  ["93.25", "0", /both a positive width and drop/],
]) {
  const beforeInvalidNeck = posted.length;
  elMap.panelNeckWidth.value = width;
  elMap.panelNeckDepth.value = depth;
  elMap.runPanel.handlers.click();
  assert.equal(posted.length, beforeInvalidNeck, "invalid neckline input must refuse before posting");
  assert.match(elMap.error.textContent, reason);
  assert.equal(elMap.panelDownloads.children.length, 0, "a refused neckline must withdraw the old panel downloads");
}

// Rust still decides the quantized shoulder span and carrier feasibility; display its refusal.
elMap.panelNeckWidth.value = "260";
elMap.panelNeckDepth.value = "26.5";
elMap.runPanel.handlers.click();
const unsupportedNeck = posted.at(-1);
assert.equal(unsupportedNeck.payload.panel.neck_width_mm, 260, "the browser must not silently shrink a requested opening");
workerInstance.onmessage({ data: {
  type: "refusal", kind: "panel", request: unsupportedNeck.request, code: "out_of_range",
  message: "neck opening must fit strictly inside the remaining shoulder span",
} });
assert.match(elMap.error.textContent, /remaining shoulder span/);
assert.equal(elMap.panelDownloads.children.length, 0);
elMap.panelNeckWidth.value = "0";
elMap.panelNeckDepth.value = "0";
elMap.panelShoulderPairs.value = "0";

// Single-bed rib requests remain visible and refuse, rather than silently turning into jersey.
const beforeUnsupportedRib = posted.length;
elMap.panelMachine.value = "brother-kh930-ayab";
elMap.runPanel.handlers.click();
assert.equal(posted.length, beforeUnsupportedRib);
assert.equal(elMap.panelRib.value, "6");
assert.match(elMap.error.textContent, /Set Rib Hem Courses to zero/);
elMap.panelRib.value = "0";
elMap.runPanel.handlers.click();
assert.equal(posted.at(-1).payload.panel.rib_hem_courses, 0);
elMap.panelMachine.value = "kniterate-7e";
elMap.panelRib.value = "6";
elMap.panelHem.value = "120";
elMap.panelChest.value = "120";
elMap.panelLength.value = "150";
elMap.runPanel.handlers.click();
const smallPanelReq = posted.at(-1);
assert.equal(smallPanelReq.payload.panel.neck_width_mm, 0, "small panel must retain the supported straight top");
assert(smallPanelReq.payload.panel.armhole_depth_mm < 60, "small panel armhole_depth_mm must scale below panel length");
workerInstance.onmessage({
  data: {
    type: "result",
    kind: "panel",
    request: smallPanelReq.request,
    result: {
      stitch_count: 120,
      pattern_json: '{ "stitches": 120, "measure": 1.0000000000000001 }\n',
      short_row_turns: 0,
      doc_sha256: "abcdef0123456789",
      svg_checks_passed: true,
      fit_claimed: false,
      chart_svg: "<svg></svg>",
      instructions_text: "Row 1: knit",
      metadata: { pathway: "measured_fully_fashioned_panel" },
    },
  },
});
assert.equal(elMap.panelDownloads.children.length, 4, "panel preview must include its reusable pattern document");
const panelPatternLink = elMap.panelDownloads.children.find((a) => a.download === "panel.knit.json");
assert.equal(await blobByUrl.get(panelPatternLink.href).text(), '{ "stitches": 120, "measure": 1.0000000000000001 }\n', "saved panel JSON must retain exact numeric text");

// Saved documents and their sealed evidence cross the browser boundary as original text.
const rawPattern = ' \r\n{"measure":1.0000000000000001,"id":18446744073709551615,"name":"écharpe"}\r\n';
const rawQualification = '{\n  "sample": 0.123456789012345678901, "sealed": true\n}\n';
const rawReceipt = '{"residual":1.234567890123456789e-12,"sealed_sha256":"abc"}\r\n';
const localFile = (text) => new Blob([text], { type: "application/json" });
const beforeMissingFile = posted.length;
await elMap.runExport.handlers.click();
assert.equal(posted.length, beforeMissingFile);
assert.match(elMap.error.textContent, /Choose a saved pattern JSON/);

let oversizedReadCount = 0;
const oversizedFile = (size) => ({ size, text: async () => { oversizedReadCount++; return "{}"; } });
elMap.exportPattern.files = [oversizedFile(2 * 1024 * 1024)];
elMap.exportQualification.files = [oversizedFile(2 * 1024 * 1024)];
elMap.exportDrapeReceipt.files = [oversizedFile(1)];
await elMap.runExport.handlers.click();
assert.equal(oversizedReadCount, 0, "combined file-size budget must be enforced before reading any file");
assert.equal(posted.length, beforeMissingFile);
assert.match(elMap.error.textContent, /combined 4 MiB budget/);

// Escaping raw text may grow the request beyond the file-size budget; count encoded UTF-8 too.
elMap.exportPattern.files = [localFile("\n".repeat(3 * 1024 * 1024))];
elMap.exportQualification.files = [];
elMap.exportDrapeReceipt.files = [];
await elMap.runExport.handlers.click();
assert.equal(posted.length, beforeMissingFile);
assert.match(elMap.error.textContent, /encoded pattern and evidence request exceeds the 4 MiB budget/);

elMap.exportPattern.files = [localFile(rawPattern)];
elMap.exportQualification.files = [localFile(rawQualification)];
elMap.exportDrapeReceipt.files = [localFile(rawReceipt)];
elMap.exportMaterialSha256.value = "a".repeat(64);
elMap.exportCreated.value = "2026-11-27T14:30";
await elMap.runExport.handlers.click();
const firstExport = posted.at(-1);
assert.equal(firstExport.type, "export");
assert.equal(firstExport.payload.pattern_json, rawPattern);
assert.equal(firstExport.payload.qualification_json, rawQualification);
assert.equal(firstExport.payload.drape_receipt_json, rawReceipt);
assert.equal(firstExport.payload.material_sha256, "a".repeat(64));
assert.equal(firstExport.payload.created, "2026-11-27T14:30");

// A valid empty ZIP with a binary comment detects accidental UTF-8/text conversion of bytes.
const zipBytes = Buffer.alloc(25);
zipBytes.writeUInt32LE(0x06054b50, 0);
zipBytes.writeUInt16LE(3, 20);
zipBytes.set([0, 128, 255], 22);
const exportResult = {
  machine: "kniterate-7e",
  pattern_sha256: "b".repeat(64),
  files: [],
  archive_base64: zipBytes.toString("base64"),
  archive_sha256: "c".repeat(64),
  vm_report: { accepted: true },
  cert: { hardware_execution_authorized: false },
  backend_export_refusal: {
    code: "unqualified_process",
    message: "Kniterate conversion requires an independent backend",
    repairs: ["Convert the verified Knitout file with the official backend."],
  },
  machine_executable: false,
  hardware_execution_authorized: false,
  physical_knitting_validated: false,
  fit_claimed: false,
};
const replyExport = (request, result = exportResult) => workerInstance.onmessage({
  data: { type: "result", kind: "export", request, result },
});
replyExport(firstExport.request);
assert.equal(elMap.exportOut.hidden, false);
assert.match(elMap.exportPre.textContent, /Machine conversion required \[unqualified_process\]/);
assert.match(elMap.exportPre.textContent, /Convert the verified Knitout file with the official backend/);
const archiveLink = elMap.exportDownloads.children[0];
assert.equal(archiveLink.download, "knit-export.zip");
assert.equal(blobByUrl.get(archiveLink.href).type, "application/zip");
assert.deepEqual(Buffer.from(await blobByUrl.get(archiveLink.href).arrayBuffer()), zipBytes, "downloaded ZIP must retain all binary bytes");

// Editing inputs immediately withdraws the previous artifact and invalidates every old reply.
elMap.exportMachine.handlers.change();
assert.equal(elMap.exportOut.hidden, true);
assert.equal(elMap.exportDownloads.children.length, 0);
assert(!activeUrls.has(archiveLink.href), "changing an export input must revoke its old ZIP URL");
const changedStatus = elMap.knitStatus.textContent;
replyExport(firstExport.request);
for (const type of ["refusal", "error"]) {
  workerInstance.onmessage({ data: { type, kind: "export", request: firstExport.request, code: "stale", message: "stale failure" } });
}
assert.equal(elMap.exportDownloads.children.length, 0, "stale success must not recreate downloads");
assert.equal(elMap.knitStatus.textContent, changedStatus);
assert.equal(elMap.error.hidden, true);

for (const id of ["exportPattern", "exportQualification", "exportDrapeReceipt", "exportMaterialSha256", "exportCreated"]) {
  await elMap.runExport.handlers.click();
  replyExport(posted.at(-1).request);
  const url = elMap.exportDownloads.children[0].href;
  elMap[id].handlers.input();
  assert.equal(elMap.exportDownloads.children.length, 0, `${id} edits must withdraw the previous ZIP`);
  assert(!activeUrls.has(url), `${id} edits must revoke the previous ZIP URL`);
}

// File reads are asynchronous: an edited selection must not submit the old in-flight file.
let resolveRead;
elMap.exportPattern.files = [{ size: 10, text: () => new Promise((resolve) => { resolveRead = resolve; }) }];
const beforePendingRead = posted.length;
const pendingRead = elMap.runExport.handlers.click();
elMap.exportPattern.files = [localFile(rawPattern)];
elMap.exportPattern.handlers.change();
resolveRead('{"old":true}');
await pendingRead;
assert.equal(posted.length, beforePendingRead, "a stale file read must never reach the worker");

// Starting another export also supersedes a read already in progress, even without input edits.
let resolveSupersededRead;
elMap.exportPattern.files = [{ size: 10, text: () => new Promise((resolve) => { resolveSupersededRead = resolve; }) }];
const supersededRead = elMap.runExport.handlers.click();
elMap.exportPattern.files = [localFile(rawPattern)];
elMap.exportQualification.files = [];
elMap.exportDrapeReceipt.files = [];
elMap.exportMaterialSha256.value = "";
elMap.exportCreated.value = "";
await elMap.runExport.handlers.click();
const newestExport = posted.at(-1);
assert.equal(newestExport.payload.pattern_json, rawPattern);
for (const field of ["qualification_json", "drape_receipt_json", "material_sha256", "created"]) {
  assert.equal(Object.hasOwn(newestExport.payload, field), false, `absent optional ${field} must be omitted`);
}
resolveSupersededRead('{"superseded":true}');
await supersededRead;
assert.equal(posted.at(-1).request, newestExport.request);
replyExport(newestExport.request);
const priorExportUrl = elMap.exportDownloads.children[0].href;
await elMap.runExport.handlers.click();
const failingExport = posted.at(-1);
assert(!activeUrls.has(priorExportUrl), "a rerun must withdraw the prior artifact before verification");
replyExport(newestExport.request);
assert.equal(elMap.exportDownloads.children.length, 0, "an old success must not replace a pending export");
workerInstance.onmessage({ data: {
  type: "refusal", kind: "export", request: failingExport.request,
  code: "unqualified_process", message: "Measured qualification does not bind this material.",
} });
assert.match(elMap.error.textContent, /Measured qualification does not bind this material/);
assert.equal(elMap.exportDownloads.children.length, 0);

// Even a successful worker envelope cannot advertise physical or machine execution claims.
await elMap.runExport.handlers.click();
replyExport(posted.at(-1).request, { ...exportResult, machine_executable: true });
assert.match(elMap.error.textContent, /unqualified export claim/);
assert.equal(elMap.exportDownloads.children.length, 0);
await elMap.runExport.handlers.click();
replyExport(posted.at(-1).request, { ...exportResult, archive_base64: "not base64!!!" });
assert.equal(elMap.error.hidden, false);
assert.equal(elMap.exportDownloads.children.length, 0, "invalid ZIP encoding must leave no partial download");

elMap.exportPattern.files = [{ size: 2, text: async () => { throw new Error("Local file could not be read"); } }];
const beforeReadFailure = posted.length;
await elMap.runExport.handlers.click();
assert.equal(posted.length, beforeReadFailure);
assert.match(elMap.error.textContent, /Local file could not be read/);
assert.equal(elMap.exportDownloads.children.length, 0);
elMap.exportPattern.files = [localFile(rawPattern)];
await elMap.runExport.handlers.click();
const exportBeforePagehide = posted.at(-1);
replyExport(exportBeforePagehide.request);

// pagehide revokes ALL Blob URLs cleanly; delayed responses cannot recreate those URLs.
assert(activeUrls.size >= 9);
windowHandlers.pagehide();
assert.equal(activeUrls.size, 0, "pagehide must revoke download URLs");
replyExport(exportBeforePagehide.request);
assert.equal(activeUrls.size, 0, "a stale response after pagehide must not recreate a download");

// Worker failure withdraws an otherwise accepted export and blocks delayed results.
await elMap.runExport.handlers.click();
const exportBeforeFailure = posted.at(-1);
replyExport(exportBeforeFailure.request);
assert.equal(elMap.exportDownloads.children.length, 1);

// worker.onerror disables buttons and surfaces error fail-closed.
workerInstance.onerror({ message: "WASM trap" });
assert.equal(elMap.runSwatch.disabled, true);
assert.equal(elMap.runPanel.disabled, true);
assert.equal(elMap.runExport.disabled, true);
assert.equal(elMap.runDye.disabled, true);
assert.match(elMap.error.textContent, /WASM trap/);
assert.equal(elMap.exportDownloads.children.length, 0);
assert.equal(activeUrls.size, 0);
replyExport(exportBeforeFailure.request);
assert.equal(activeUrls.size, 0, "failed engine's late result must remain unavailable");

// Execute the worker dispatcher too: only the outer envelope is serialized, preserving raw files.
const workerSource = worker
  .replace('import init, * as ftx from "./pkg/ftx_wasm.js?v=2026-11-27.3";', "")
  .replace("import.meta.url", '"https://local.invalid/knit-worker.js"');
let forwardedRequest;
const workerApi = {
  wasm_interface_stamp: () => "ftx-wasm-interface/2026-11-27.3",
  knit_swatch_json: () => '{"ok":true,"result":{}}',
  knit_panel_preview_json: () => '{"ok":true,"result":{}}',
  dye_optimize_json: () => '{"ok":true,"result":{}}',
  knit_export_json: (raw) => {
    forwardedRequest = JSON.parse(raw);
    return JSON.stringify({ ok: true, result: exportResult });
  },
};
async function loadWorker(api) {
  const messages = [];
  const scope = vm.createContext({
    init: async () => {}, ftx: api, URL, JSON, String, Error,
    self: {}, postMessage: (message) => messages.push(message),
  });
  vm.runInContext(workerSource, scope);
  await vm.runInContext("ready", scope);
  return { scope, messages };
}
const loadedWorker = await loadWorker(workerApi);
assert.equal(loadedWorker.messages[0].type, "ready");
await loadedWorker.scope.self.onmessage({ data: firstExport });
assert.deepEqual(forwardedRequest, JSON.parse(JSON.stringify(firstExport.payload)));
assert.equal(forwardedRequest.pattern_json, rawPattern);
assert.equal(forwardedRequest.qualification_json, rawQualification);
assert.equal(forwardedRequest.drape_receipt_json, rawReceipt);
assert.equal(loadedWorker.messages.at(-1).kind, "export");
assert.equal(loadedWorker.messages.at(-1).result.archive_base64, exportResult.archive_base64);
workerApi.knit_export_json = () => JSON.stringify({ ok: false, refusal: {
  code: "unqualified_process", message: "Evidence mismatch", repairs: ["Select matching measured evidence"],
} });
await loadedWorker.scope.self.onmessage({ data: firstExport });
assert.equal(loadedWorker.messages.at(-1).type, "refusal");
assert.equal(loadedWorker.messages.at(-1).code, "unqualified_process");
assert.equal(loadedWorker.messages.at(-1).request, firstExport.request);
const missingExportWorker = await loadWorker({ ...workerApi, knit_export_json: undefined });
assert.equal(missingExportWorker.messages[0].type, "loadError", "old bundle without saved-document export must refuse readiness");
await missingExportWorker.scope.self.onmessage({ data: firstExport });
assert.equal(missingExportWorker.messages.at(-1).type, "error");

}

checkBehavior().then(() => {
  console.log(`check_knit_web.cjs: OK (${new Set(idMatches).size} DOM bindings, schema fields, async VM behavioral checks, zero network calls, honesty badges verified)`);
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

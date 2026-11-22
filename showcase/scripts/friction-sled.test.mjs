// Verify native friction-sled trajectories, stick-to-slip threshold, motion direction, units,
// live WASM & native CLI calibration parity, and catalog unavailability (ftx-13b).
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/friction-sled.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import * as ftx from "../public/workbench/pkg/ftx_wasm.js";
import { calibrateFrictionState, verifySledHistory } from "../public/labs/friction-sled-worker.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const NATIVE_MATERIALS = path.join(REPO_ROOT, "target/release/ftx-materials");

let browser;
before(async () => {
  ftx.initSync({ module: readFileSync(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url)) });
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});

after(async () => {
  await browser?.close();
});

test("Native friction-sled report verifies stick-to-slip thresholds, motion directions, units, catalog unavailability, and CLI calibration parity", () => {
  const report = JSON.parse(readFileSync(new URL("../public/materials/friction-sled.json", import.meta.url), "utf8"));
  assert.equal(report.schema, "ftx-friction-sled-exhibit/1");
  assert.equal(report.states.length, 2);
  assert.equal(report.histories.length, 4);

  // Verify every recorded sled history satisfies exact units, positive contact gap, normal balance, direction alignment & Coulomb plateau.
  const summaries = report.histories.map(verifySledHistory);
  assert.equal(summaries.length, 4);

  // Higher-grip synthetic state (mu=0.75) holds significantly longer than mu=0.40 at the same 2.0 N normal load.
  const a2n = report.histories.find(h => h.id === "state_a_2n_pos_x");
  const b2n = report.histories.find(h => h.id === "state_b_2n_pos_x");
  assert.ok(a2n && b2n);
  assert.ok(b2n.coulomb_threshold_n > a2n.coulomb_threshold_n * 1.8);
  assert.ok(b2n.first_slip_pull_n > a2n.first_slip_pull_n * 1.7);

  // Verify real catalog textiles remain honestly unavailable (0 qualified friction coefficients).
  assert.ok(report.catalog_audit.total_materials >= 400);
  assert.equal(report.catalog_audit.materials_with_source_qualified_friction, 0);
  assert.ok(report.catalog_audit.unavailable_pairs.every(p => p.available === false));

  // Verify live WASM calibration matches every state's mu & refuses when allowEstimates = false.
  for (const state of report.states) {
    const okCal = calibrateFrictionState(ftx, state.calibration_input, true);
    assert.ok(Math.abs(okCal.mu - state.mu) < 1e-9, `WASM mu ${okCal.mu} vs report ${state.mu}`);
    assert.equal(okCal.inputSha256, state.calibration_input_sha256);
    assert.equal(okCal.readiness.ready, true);

    const refusedCal = calibrateFrictionState(ftx, state.calibration_input, false);
    assert.equal(refusedCal.readiness.ready, false);
    assert.match(refusedCal.readiness.refusal?.message ?? "", /explicit consent/i);
  }

  // Cross-check state A against native `ftx-materials calibrate-friction` CLI directly.
  if (existsSync(NATIVE_MATERIALS)) {
    const cliRaw = execFileSync(
      NATIVE_MATERIALS,
      ["calibrate-friction", path.join(REPO_ROOT, "tests/fixtures/materials/synthetic-friction-sliding.json"), "--allow-estimates"],
      { encoding: "utf8" },
    );
    const cli = JSON.parse(cliRaw);
    const stateA = report.states[0];
    assert.ok(Math.abs(cli.calibration.coefficient.value - stateA.mu) < 1e-12);
    assert.equal(cli.calibration.input_sha256, stateA.calibration_input_sha256);
    assert.equal(cli.readiness.ready, true);
  }
});

test("Friction sled page steps across stick-to-slip, switches histories & directions, refuses uncalibrated catalog pairs, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/experiments/friction/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-metric="wasm-mu"]').waitFor({ timeout: 20000 });

    // Initial state A (mu = 0.40) verifies live WASM mu = 0.4000 and READY admission.
    assert.equal(await page.locator('[data-metric="wasm-mu"]').innerText(), "0.4000 (1)");
    assert.match(await page.locator('[data-metric="wasm-ready"]').innerText(), /READY/);
    assert.equal(await page.locator('[data-metric="threshold"]').innerText(), "0.800 N");

    // Step 0 is STICK; advancing to final step is SLIP with positive displacement along +x.
    const scrubber = page.getByRole("slider", { name: /Recorded friction sled step cursor/i });
    await scrubber.fill("0");
    assert.match(await page.locator('[data-metric="regime"]').innerText(), /STICK/i);
    await scrubber.fill("44");
    assert.match(await page.locator('[data-metric="regime"]').innerText(), /SLIP/i);
    assert.match(await page.locator('[data-metric="normal"]').innerText(), /^2\.00\d N$/);

    // Unchecking allow_estimates immediately surfaces the real Rust/WASM consent refusal.
    await page.getByLabel(/Admit synthetic estimate/i).uncheck();
    await page.waitForFunction(() => /REFUSED/i.test(document.querySelector('[data-metric="wasm-ready"]')?.textContent ?? ""));
    await page.getByLabel(/Admit synthetic estimate/i).check();
    await page.waitForFunction(() => /READY/i.test(document.querySelector('[data-metric="wasm-ready"]')?.textContent ?? ""));

    // Switch to higher-grip synthetic state (mu = 0.75) and confirm WASM re-identifies 0.7500 & threshold 1.500 N.
    await page.locator('button[data-surface="synthetic_pair_075"]').click();
    await page.waitForFunction(() => document.querySelector('[data-metric="wasm-mu"]')?.textContent === "0.7500 (1)");
    assert.equal(await page.locator('[data-metric="threshold"]').innerText(), "1.500 N");

    // Switch to unavailable catalog textiles: sled curve is removed and unavailability refusal is displayed.
    await page.locator('button[data-surface="catalog_unavailable"]').click();
    await page.locator('[data-unavailable="true"]').waitFor({ timeout: 10000 });
    assert.match(await page.locator('[data-unavailable="true"]').innerText(), /missing source-qualified friction/i);
    assert.equal(await page.locator(".fs-svg").count(), 0);

    // Download link returns valid JSON report.
    const res = await page.request.get(new URL("/materials/friction-sled.json", site).href);
    assert.equal(res.status(), 200);
    const json = await res.json();
    assert.equal(json.schema, "ftx-friction-sled-exhibit/1");

    // Desktop & mobile overflow + single aria-current check.
    await page.locator('button[data-surface="synthetic_pair_040"]').click();
    await page.locator(".fs-svg").waitFor({ timeout: 10000 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

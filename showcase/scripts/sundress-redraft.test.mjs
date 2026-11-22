// Verify sundress ease redrafting, pinned silhouette comparison, native CLI parity, and stale-download locking (ftx-ahm).
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/sundress-redraft.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import * as ftx from "../public/workbench/pkg/ftx_wasm.js";
import { runSundressRedraft } from "../public/labs/sundress-redraft.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const NATIVE_FTX = path.join(REPO_ROOT, "target/release/ftx");

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

test("Sundress redraft matches unchanged outlines, recalculates seams on bust-ease increase, and matches native ftx validate", () => {
  const basePatternJson = readFileSync(new URL("../public/exhibits/sundress/dress-pattern.json", import.meta.url), "utf8");

  // 1. Unchanged request on Reference (90/72, +40/+60 mm ease) yields matching outlines and 0.0 mm seam deltas.
  const unchanged = runSundressRedraft(ftx, {
    basePatternJson,
    cut: "dress",
    chestCm: 90.0,
    waistCm: 72.0,
    reqBustEaseMm: 40,
    reqWaistEaseMm: 60,
  });
  assert.equal(unchanged.unchanged, true);
  assert.equal(unchanged.outlinesIdentical, true);
  assert.equal(unchanged.maxPanelShiftMm, 0);
  assert.equal(unchanged.maxSeamDeltaMm, 0);
  assert.equal(unchanged.first.status, "CONVERGED");
  assert.equal(unchanged.second.status, "CONVERGED");
  assert.equal(unchanged.first.files.pdf.name, "GARDEN_SUNDRESS_A4.pdf");
  assert.equal(unchanged.second.files.pdf.name, "SUNDRESS_REDRAFT_A4.pdf");

  // 2. Increasing required bust ease on Reference (40 -> 70 mm) triggers LOW on first draft, alters bodice, recalculates seams & changes exports.
  const bustBoost = runSundressRedraft(ftx, {
    basePatternJson,
    cut: "dress",
    chestCm: 90.0,
    waistCm: 72.0,
    reqBustEaseMm: 70,
    reqWaistEaseMm: 60,
  });
  assert.equal(bustBoost.unchanged, false);
  assert.equal(bustBoost.outlinesIdentical, false);
  assert.equal(bustBoost.first.status, "NOT_CONVERGED");
  assert.equal(bustBoost.second.status, "CONVERGED");
  assert.equal(bustBoost.second.passedSeams, 8);
  assert.equal(bustBoost.deltaBustMm, 30);
  assert.equal(bustBoost.deltaWaistMm, 0);
  const sideR = bustBoost.seamDeltas.find(s => s.seam === "bodice_side_R");
  assert.ok(sideR && Math.abs(sideR.deltaAMm) >= 1.0, `expected bodice_side_R length to change, got ${sideR?.deltaAMm}`);
  assert.notEqual(
    bustBoost.second.files.pdf.fingerprint,
    unchanged.second.files.pdf.fingerprint,
    "increasing bust ease must change exported PDF bytes",
  );
  assert.notEqual(
    bustBoost.second.files.svg.fingerprint,
    unchanged.second.files.svg.fingerprint,
    "increasing bust ease must change exported SVG bytes",
  );

  // 3. Default Close wearer (93.5 / 77.0 cm): NOT_CONVERGED -> CONVERGED, all 8 seams pass, native CLI agrees.
  const closeRun = runSundressRedraft(ftx, {
    basePatternJson,
    cut: "dress",
    chestCm: 93.5,
    waistCm: 77.0,
    reqBustEaseMm: 40,
    reqWaistEaseMm: 60,
  });
  assert.equal(closeRun.first.status, "NOT_CONVERGED");
  assert.equal(closeRun.second.status, "CONVERGED");
  assert.equal(closeRun.second.passedSeams, 8);
  assert.equal(closeRun.first.files.pdf.name, "GARDEN_SUNDRESS_A4.pdf");
  assert.equal(closeRun.second.files.pdf.name, "SUNDRESS_REDRAFT_A4.pdf");
  assert.equal(closeRun.first.files.pdf.fingerprint, unchanged.first.files.pdf.fingerprint, "first draft PDF must remain preserved");

  // 4. Lightly eased cut (1.05x) also preserves exact gather ratio and converges for Beyond garment (96.5 / 80.5 cm).
  const lightPatternJson = readFileSync(new URL("../public/exhibits/sundress/light-pattern.json", import.meta.url), "utf8");
  const lightRun = runSundressRedraft(ftx, {
    basePatternJson: lightPatternJson,
    cut: "light",
    chestCm: 96.5,
    waistCm: 80.5,
    reqBustEaseMm: 40,
    reqWaistEaseMm: 60,
  });
  assert.equal(lightRun.gatherRatio, 1.05);
  assert.equal(lightRun.first.status, "NOT_CONVERGED");
  assert.equal(lightRun.second.status, "CONVERGED");
  assert.equal(lightRun.second.passedSeams, 8);

  if (existsSync(NATIVE_FTX)) {
    const dir = mkdtempSync(path.join(tmpdir(), "ftx-sundress-redraft-"));
    const patPath = path.join(dir, "redrafted.json");
    const fitPath = path.join(dir, "fit.json");
    const measPath = path.join(dir, "meas.json");
    writeFileSync(patPath, closeRun.second.patternJson);
    writeFileSync(fitPath, JSON.stringify(closeRun.fitSpec));
    const baseMeas = JSON.parse(readFileSync(new URL("../public/exhibits/sundress/dress-measures.json", import.meta.url), "utf8"));
    baseMeas.fused.chest.value_mm = 935;
    baseMeas.fused.waist.value_mm = 770;
    writeFileSync(measPath, JSON.stringify(baseMeas));
    const env = JSON.parse(
      execFileSync(NATIVE_FTX, ["validate", patPath, "--measurements", measPath, "--fit", fitPath, "--json"], {
        encoding: "utf8",
      }),
    );
    assert.equal(env.ok, true);
    assert.equal(env.result.status, "CONVERGED");
    const cliBust = env.result.points.find(p => p.id === "bust");
    const cliWaist = env.result.points.find(p => p.id === "waist");
    assert.equal(cliBust.finished_mm, 975);
    assert.equal(cliWaist.finished_mm, 830);
    for (const s of closeRun.seamDeltas) {
      const cliSeam = env.result.seams.find(x => x.seam === s.seam);
      assert.ok(cliSeam, `missing native seam ${s.seam}`);
      assert.ok(Math.abs(cliSeam.len_a_mm - s.secondLenAMm) < 0.05, `${s.seam} native=${cliSeam.len_a_mm} wasm=${s.secondLenAMm}`);
    }
  }
});

test("Sundress redraft page preserves first draft, prevents stale downloads, updates outlines/seams, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1180, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/wardrobe/sundress/redraft/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-download="second-pdf"]').waitFor({ timeout: 25000 });

    // Initial Close (93.5 / 77.0 cm): First draft NOT_CONVERGED, Second cut CONVERGED.
    assert.equal(await page.locator('[data-metric="first-status"]').innerText(), "NOT_CONVERGED");
    assert.equal(await page.locator('[data-metric="second-status"]').innerText(), "CONVERGED");
    assert.equal(await page.locator('[data-download="first-pdf"]').getAttribute("download"), "GARDEN_SUNDRESS_A4.pdf");
    assert.equal(await page.locator('[data-download="second-pdf"]').getAttribute("download"), "SUNDRESS_REDRAFT_A4.pdf");
    const initialFp = await page.locator('[data-download="second-pdf"]').getAttribute("data-fingerprint");

    // Switching to Reference immediately marks the previous second cut STALE and locks/removes stale second downloads!
    await page.locator('button[data-wearer="reference"]').click();
    await page.locator('[data-download-locked="true"]').waitFor({ timeout: 5000 });
    assert.equal(await page.locator('[data-download="second-pdf"]').count(), 0, "stale second PDF link must be unavailable");
    assert.equal(await page.locator('[data-download="first-pdf"]').count(), 1, "preserved first draft remains available");

    // Request checked second cut on Reference (unchanged request -> 0.00 mm shift, matching outlines).
    await page.getByRole("button", { name: "Request checked second cut" }).click();
    await page.locator('[data-download="second-pdf"]').waitFor({ timeout: 25000 });
    assert.equal(await page.locator('[data-metric="max-panel-shift"]').innerText(), "0.00 mm");
    const refFp = await page.locator('[data-download="second-pdf"]').getAttribute("data-fingerprint");
    assert.notEqual(refFp, initialFp);

    // Increasing required bust ease immediately re-locks stale downloads, then redrafting changes export & seam length.
    const bustRange = page.getByRole("slider", { name: /Required bust ease/i });
    await bustRange.fill("70");
    await page.locator('[data-download-locked="true"]').waitFor({ timeout: 5000 });
    assert.equal(await page.locator('[data-download="second-pdf"]').count(), 0);
    await page.getByRole("button", { name: "Request checked second cut" }).click();
    await page.locator('[data-download="second-pdf"]').waitFor({ timeout: 25000 });
    const boostedFp = await page.locator('[data-download="second-pdf"]').getAttribute("data-fingerprint");
    assert.notEqual(boostedFp, refFp, "increasing bust ease must yield distinct second PDF");
    assert.equal(await page.locator('[data-metric="first-status"]').innerText(), "NOT_CONVERGED");
    assert.equal(await page.locator('[data-metric="second-status"]').innerText(), "CONVERGED");
    assert.match(await page.locator('tr[data-seam-row="bodice_side_R"]').innerText(), /\+1\.\d mm/);

    // Toggle pin checkbox removes/restores first-draft silhouette path.
    assert.equal(await page.locator(".sr-first-path").count(), 1);
    await page.getByLabel("Pin first draft silhouette").uncheck();
    assert.equal(await page.locator(".sr-first-path").count(), 0);
    await page.getByLabel("Pin first draft silhouette").check();
    assert.equal(await page.locator(".sr-first-path").count(), 1);

    // Desktop & mobile overflow + single aria-current check.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

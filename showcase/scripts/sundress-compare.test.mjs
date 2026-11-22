// Verify side-by-side native drape comparison of one fixed sundress (ftx-9zm):
// identical fixed pattern & solver configuration across two independently accepted synthetic adult forms,
// visible hem-elevation and waist-clearance differences, swap-sides preserving each report & common world scale,
// same-state exact match (0.0 deltas), refusal never appearing as a completed garment, and phone layout.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/sundress-compare.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { COMMON_WORLD_SCALE_M_PER_UNIT, buildCompareMetrics } from "../public/labs/sundress-compare.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";

let browser;
before(async () => {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});

after(async () => {
  await browser?.close();
});

test("Compare manifest proves identical fixed pattern & solver config, swap symmetry, same-state match, and refusal blocking", () => {
  const manifest = JSON.parse(readFileSync(new URL("../public/exhibits/sundress-compare/compare.json", import.meta.url), "utf8"));
  assert.equal(manifest.schema, "ftx-sundress-compare/1");
  assert.equal(manifest.fixedPattern.spacingMm, 20);
  assert.equal(manifest.fixedPattern.vertices, 3458);

  const ref = manifest.states.find(s => s.id === "reference");
  const slender = manifest.states.find(s => s.id === "slender");
  const tight = manifest.states.find(s => s.id === "tight_refused");
  assert.ok(ref?.accepted && slender?.accepted);
  assert.equal(tight?.accepted, false);
  assert.equal(tight?.sourceBase, null);
  assert.equal(tight?.refusal.code, "intersecting_start");

  // Two distinct forms produce clearly visible hem & clearance shifts
  const lr = buildCompareMetrics(ref, slender);
  assert.equal(lr.worldScale, COMMON_WORLD_SCALE_M_PER_UNIT);
  assert.ok(Math.abs(lr.deltas.hemMeanMm) > 40, `expected >40 mm hem shift, got ${lr.deltas.hemMeanMm}`);
  assert.ok(Math.abs(lr.deltas.backGapMm) > 10, `expected >10 mm back-gap shift, got ${lr.deltas.backGapMm}`);

  // Swapping sides preserves common world scale and negates signed deltas exactly
  const rl = buildCompareMetrics(slender, ref);
  assert.equal(rl.worldScale, lr.worldScale);
  assert.equal(rl.deltas.hemFrontMm, -lr.deltas.hemFrontMm);
  assert.equal(rl.deltas.hemBackMm, -lr.deltas.hemBackMm);
  assert.equal(rl.deltas.frontGapMm, -lr.deltas.frontGapMm);
  assert.equal(rl.deltas.backGapMm, -lr.deltas.backGapMm);

  // Same state on both sides matches with exact zero deltas
  const same = buildCompareMetrics(ref, ref);
  assert.equal(same.sameState, true);
  assert.equal(same.deltas.hemFrontMm, 0);
  assert.equal(same.deltas.hemBackMm, 0);
  assert.equal(same.deltas.frontGapMm, 0);
  assert.equal(same.deltas.backGapMm, 0);
  assert.equal(same.deltas.strainP95Pct, 0);

  // Refused solve cannot render as a completed garment
  const withRefusal = buildCompareMetrics(ref, tight);
  assert.equal(withRefusal.canRenderLeftGarment, true);
  assert.equal(withRefusal.canRenderRightGarment, false);
  assert.equal(withRefusal.deltas, null);
});

test("Compare page loads both native drapes under common world scale, swaps cleanly, matches same state, blocks refused garment, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/wardrobe/sundress/compare/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-side="left"] [data-drape-loaded="true"]').waitFor({ timeout: 25000 });
    await page.locator('[data-side="right"] [data-drape-loaded="true"]').waitFor({ timeout: 25000 });

    const leftShaBefore = await page.locator('[data-metric="sha-left"]').innerText();
    const rightShaBefore = await page.locator('[data-metric="sha-right"]').innerText();
    const leftHemBefore = await page.locator('[data-metric="hem-left"]').innerText();
    const rightHemBefore = await page.locator('[data-metric="hem-right"]').innerText();
    assert.notEqual(leftShaBefore, rightShaBefore);
    assert.equal(await page.locator('[data-side="left"]').getAttribute("data-world-scale"), "1");
    assert.equal(await page.locator('[data-side="right"]').getAttribute("data-world-scale"), "1");

    // Swap sides -> preserves each side's report SHA-256, hem metrics, and common world scale
    await page.locator('button[data-action="swap"]').click();
    assert.equal(await page.locator('[data-metric="sha-left"]').innerText(), rightShaBefore);
    assert.equal(await page.locator('[data-metric="sha-right"]').innerText(), leftShaBefore);
    assert.equal(await page.locator('[data-metric="hem-left"]').innerText(), rightHemBefore);
    assert.equal(await page.locator('[data-metric="hem-right"]').innerText(), leftHemBefore);
    assert.equal(await page.locator('[data-side="left"]').getAttribute("data-world-scale"), "1");
    assert.equal(await page.locator('[data-side="right"]').getAttribute("data-world-scale"), "1");

    // Match same state on both sides -> exact 0.0 mm & 0.00% deltas
    await page.locator('button[data-action="match-same"]').click();
    assert.equal(await page.locator(".sc-viewports").getAttribute("data-same-state"), "true");
    assert.equal(await page.locator('[data-metric="delta-hem-front"]').innerText(), "0.0 mm");
    assert.equal(await page.locator('[data-metric="delta-back-gap"]').innerText(), "0.0 mm");
    assert.equal(await page.locator('[data-metric="delta-strain"]').innerText(), "0.00%");

    // Selecting refused form on right side displays native refusal card and ZERO garment canvas
    await page.locator('button[data-pick-right="tight_refused"]').click();
    await page.locator('[data-side="right"] [data-refused-garment="true"]').waitFor({ timeout: 10000 });
    assert.equal(await page.locator('[data-side="right"] canvas').count(), 0, "Refused solve must never render a garment canvas");
    assert.match(await page.locator('[data-side="right"]').innerText(), /intersecting_start/);
    assert.equal(await page.locator('[data-side="left"] canvas').count(), 1, "Accepted left side remains rendered");

    // Restore accepted pair and check desktop + 390px phone overflow & single aria-current
    await page.locator('button[data-pick-left="reference"]').click();
    await page.locator('button[data-pick-right="slender"]').click();
    await page.locator('[data-side="right"] [data-drape-loaded="true"]').waitFor({ timeout: 20000 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

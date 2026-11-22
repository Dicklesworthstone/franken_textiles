// Verify inside-out layered garment explorer (ftx-z55):
// toggling shell reveals correct lining, turning layers apart preserves every native seam index (0..35),
// selecting construction edges matches actual panel & seam IDs, and missing linings / unpaired seams
// are surfaced honestly without inventing one.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/inside-out.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { buildInsideOutState } from "../public/labs/waistcoat.mjs";

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

test("Layer toggles and radial separation preserve exact 0-based native seam indices and surface missing linings honestly", () => {
  const man = JSON.parse(readFileSync(new URL("../public/exhibits/waistcoat/waistcoat-manifest.json", import.meta.url), "utf8"));

  const assembled = buildInsideOutState(man, { showShell: true, explodeMm: 0, selectedSeamIndex: 13 });
  const separated = buildInsideOutState(man, { showShell: true, explodeMm: 72, selectedSeamIndex: 13 });
  const liningOnly = buildInsideOutState(man, { showShell: false, showFacing: true, showLining: true, explodeMm: 40, selectedSeamIndex: 13 });

  // Exact seam index invariance across all 36 native seams
  assert.equal(assembled.seams.length, 36);
  assert.deepEqual(
    assembled.seams.map(s => [s.index, s.id, s.a, s.b]),
    separated.seams.map(s => [s.index, s.id, s.a, s.b]),
  );
  assert.deepEqual(
    assembled.seams.map(s => [s.index, s.id, s.a, s.b]),
    liningOnly.seams.map(s => [s.index, s.id, s.a, s.b]),
  );

  // Turning layers apart increases shell-to-lining radial separation monotonically
  assert.ok(separated.radialOffsetMm.shell - separated.radialOffsetMm.lining > assembled.radialOffsetMm.shell - assembled.radialOffsetMm.lining + 100);

  // Toggling shell hides shell while keeping facing and lining visible
  assert.equal(liningOnly.layerVisible.shell, false);
  assert.equal(liningOnly.layerVisible.facing, true);
  assert.equal(liningOnly.layerVisible.lining, true);

  // Missing collar lining & unpaired back vent are surfaced explicitly
  assert.ok(assembled.missingLiningCount >= 7, "must surface undrafted collar lining & unpaired inner necklines");
  assert.ok(assembled.unpairedEdgeCount >= 8, "must surface open centre-back vent edges");
  assert.ok(assembled.unpairedAndMissing.some(u => u.ref === "wc_collar/top" && u.kind === "MISSING_LINING"));
  assert.ok(assembled.unpairedAndMissing.some(u => u.ref === "wc_back.R/cb_vent_edge" && u.kind === "UNPAIRED_EDGE"));
});

test("Inside-out page toggles shell to reveal lining, turns layers apart with invariant seam indices, audits missing linings, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/wardrobe/waistcoat/inside-out/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-metric="selection-readout"]').waitFor({ timeout: 20000 });

    const stage = page.locator(".io-stage-card");
    assert.equal(await stage.getAttribute("data-visible-layers"), "shell,facing,lining,pocket");

    // Capture full seam index list before separating layers
    const beforeIndices = await page.locator("button[data-seam-index]").evaluateAll(els =>
      els.map(el => `${el.getAttribute("data-seam-index")}:${el.getAttribute("data-seam-id")}`),
    );
    assert.equal(beforeIndices.length, 36);

    // Toggle shell off -> reveals facing, lining, pocket while shell is hidden
    await page.locator('button[data-toggle-layer="shell"]').click();
    assert.equal(await stage.getAttribute("data-visible-layers"), "facing,lining,pocket");

    // Turn layers apart to 68 mm -> seam indices stay 100% invariant
    const sepSlider = page.getByRole("slider", { name: /Turn garment layers apart/i });
    await sepSlider.fill("68");
    assert.equal(await stage.getAttribute("data-explode-mm"), "68");
    const afterIndices = await page.locator("button[data-seam-index]").evaluateAll(els =>
      els.map(el => `${el.getAttribute("data-seam-index")}:${el.getAttribute("data-seam-id")}`),
    );
    assert.deepEqual(afterIndices, beforeIndices);

    // Select seam #20 (bag_hem_front_R) and verify panel/edge pairing readout
    await page.locator('button[data-seam-id="bag_hem_front_R"]').click();
    assert.equal(await stage.getAttribute("data-selected-seam-index"), "20");
    assert.match(await page.locator('[data-metric="selection-readout"]').innerText(), /wc_front\.R\/hem_lining/);
    assert.match(await page.locator('[data-metric="selection-readout"]').innerText(), /wc_front_lining\.R\/hem/);

    // Switch to Missing lining & unpaired audit tab and select missing collar lining
    await page.locator('button[data-tab="audit"]').click();
    await page.locator('[data-audit-panel="true"]').waitFor({ timeout: 10000 });
    await page.locator('button[data-unpaired-ref="wc_collar/top"]').click();
    assert.equal(await stage.getAttribute("data-selected-unpaired"), "wc_collar/top");
    assert.match(await page.locator('[data-metric="selection-readout"]').innerText(), /MISSING_LINING/);

    // Desktop & mobile overflow + single aria-current check
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

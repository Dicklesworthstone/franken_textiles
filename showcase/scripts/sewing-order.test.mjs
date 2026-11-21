// Verify native sewing-order playback on the Zouave bolero (ftx-jkf) against ftx-materials and in Chromium.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/sewing-order.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import init, * as ftx from "../public/workbench/pkg/ftx_wasm.js";
import { buildBoleroSewingBundle } from "../public/labs/sewing-order.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const repoRoot = path.resolve(import.meta.dirname, "../..");
const ftxMaterialsBin = process.env.FTX_MATERIALS_BIN || path.join(repoRoot, "target/release/ftx-materials");

let browser;
before(async () => {
  await init({ module_or_path: await readFile(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url)) });
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});

after(async () => {
  await browser?.close();
});

test("sewing-order bundle matches native ftx-materials sew-plan and sew-stitches exactly", () => {
  const bundle = buildBoleroSewingBundle(ftx);
  const starter = JSON.parse(readFileSync(new URL("../public/labs/sewing-starter.json", import.meta.url), "utf8"));
  const publishedPlan = JSON.parse(readFileSync(new URL("../public/labs/bolero-sew-plan.json", import.meta.url), "utf8"));
  const publishedProgram = JSON.parse(readFileSync(new URL("../public/labs/bolero-sew-program.json", import.meta.url), "utf8"));

  assert.deepEqual(starter, bundle.starter);
  assert.equal(starter.totalOperations, 10);
  assert.equal(starter.hardwareExecutionAuthorized, false);
  assert.equal(starter.physicalSeamQualityValidated, false);

  const dir = mkdtempSync(path.join(tmpdir(), "ftx-sew-test-"));
  const patPath = path.join(dir, "bolero.json");
  const optPath = path.join(dir, "options.json");
  writeFileSync(patPath, bundle.patternJson, { flag: "wx" });
  writeFileSync(optPath, bundle.optionsJson, { flag: "wx" });

  const nativePlanProc = spawnSync(ftxMaterialsBin, ["sew-plan", patPath, optPath], { encoding: "utf8" });
  assert.equal(nativePlanProc.status, 0, nativePlanProc.stderr || nativePlanProc.stdout);
  const nativePlan = JSON.parse(nativePlanProc.stdout);
  assert.deepEqual(publishedPlan, nativePlan);

  const nativeProgProc = spawnSync(ftxMaterialsBin, ["sew-stitches", patPath, optPath], { encoding: "utf8" });
  assert.equal(nativeProgProc.status, 0, nativeProgProc.stderr || nativeProgProc.stdout);
  const nativeProgram = JSON.parse(nativeProgProc.stdout);
  assert.deepEqual(publishedProgram, nativeProgram);

  // Verify operation count and exact ordering match native `seams`.
  assert.deepEqual(
    starter.operations.map(op => op.id),
    nativePlan.seams.map(s => s.pass.id),
  );
  assert.equal(starter.totalStitchIntervals, nativePlan.stitch_intervals);
  assert.equal(starter.totalDirectedStitches, nativeProgram.stitches.length);
});

test("sewing-order player navigates with keyboard, lights flat & 3D seams, serves machine files, and fits phone", async () => {
  const starter = JSON.parse(readFileSync(new URL("../public/labs/sewing-starter.json", import.meta.url), "utf8"));
  const page = await browser.newPage({
    viewport: { width: 1120, height: 840 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/studies/sewing/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".so-flat-svg").waitFor({ timeout: 15000 });

    // Verify all 10 operations are listed in exact native order.
    const renderedIds = await page.locator(".so-op-btn").evaluateAll(els => els.map(e => e.getAttribute("data-op-id")));
    assert.deepEqual(renderedIds, starter.operations.map(o => o.id));
    assert.equal(await page.locator('.so-active-seam').getAttribute("data-active-op"), starter.operations[0].id);

    // Keyboard ArrowRight steps to operation 2, ArrowLeft returns to operation 1.
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), starter.operations[1].id);
    assert.match(await page.locator(".so-step-status").innerText(), /Step 2 \/ 10/i);
    await page.keyboard.press("ArrowLeft");
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), starter.operations[0].id);

    // Click an operation with prerequisites & unlocked next ops (`armhole-front-l`) and inspect endpoints + chips.
    await page.locator('.so-op-btn[data-op-id="armhole-front-l"]').click();
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), "armhole-front-l");
    const afterChips = await page.locator('[data-deps="after"]').innerText();
    assert.match(afterChips, /shoulder-l/);
    assert.match(afterChips, /side-l/);
    assert.match(afterChips, /underarm-l/);
    const unlocksChips = await page.locator('[data-deps="unlocks"]').innerText();
    assert.match(unlocksChips, /armhole-back-l/);

    // Clicking the unlocked next chip jumps straight to `armhole-back-l`.
    await page.locator('[data-deps="unlocks"] button').first().click();
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), "armhole-back-l");
    const tableText = await page.locator(".so-table").innerText();
    assert.match(tableText, /sleeve\.L\/cap_b/);
    assert.match(tableText, /back\/armhole\.L/);

    // Verify desktop overflow and single aria-current in header.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);

    // Switch to phone viewport (390x844): keyboard next/previous, touch buttons, and machine-file downloads all work.
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.getByRole("button", { name: "Previous sewing operation" }).click();
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), "armhole-front-l");
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator(".so-active-seam").getAttribute("data-active-op"), "armhole-back-l");

    for (const selector of [
      'a[href="/labs/bolero-sew-program.json"]',
      'a[href="/labs/bolero-sew-plan.json"]',
      'a[href="/labs/stitch-back.dst"]',
      'a[href="/patterns/chart-zouave_bolero.pdf"]',
    ]) {
      const href = await page.locator(selector).getAttribute("href");
      const res = await page.request.get(new URL(href, site).href);
      assert.equal(res.status(), 200, `download failed for ${href}`);
      assert.ok((await res.body()).length > 1000, `empty file for ${href}`);
    }
  } finally {
    await page.close();
  }
});

// Verify Wash, shrink & recut (ftx-8mf) against native `ftx materials treatment-pattern` and in Chromium.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/wash-shrink.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import init, * as wasm from "../public/workbench/pkg/ftx_wasm.js";
import { WASH_PRESETS, runWashShrink } from "../public/labs/wash-worker.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const repoRoot = path.resolve(import.meta.dirname, "../..");
const ftxBin = process.env.FTX_BIN || path.join(repoRoot, "target/release/ftx");

let browser;
let treatments;
before(async () => {
  const bytes = await readFile(new URL("../public/workbench/pkg/ftx_wasm_bg.wasm", import.meta.url));
  await init({ module_or_path: bytes });
  treatments = JSON.parse(await readFile(new URL("../public/labs/wash-treatments.json", import.meta.url), "utf8"));
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});

after(async () => {
  await browser?.close();
});

test("wash shrinkage & compensation match native CLI and refuse mismatched materials", () => {
  const treat = runWashShrink(wasm, treatments, WASH_PRESETS.denim1_treat);
  assert.equal(treat.mode, "treat");
  assert.equal(treat.transition.warpShrinkPct, 4.83);
  assert.equal(treat.transition.weftShrinkPct, 5.5);
  assert.ok(Math.abs(treat.pieces[0].scale_warp_weft[0] - 0.9517) < 1e-12);
  assert.ok(Math.abs(treat.pieces[0].scale_warp_weft[1] - 0.945) < 1e-12);
  assert.equal(treat.seamsBefore[0].len_a_mm, 103.5);
  assert.equal(treat.seamsAfter[0].len_a_mm, 98.5);
  assert.equal(treat.seamsAfter[0].residual_mm, 0);
  assert.match(treat.revisedSvg, /<svg\b/);
  assert.ok(treat.files.pdf?.bytes.length > 5000);

  const comp = runWashShrink(wasm, treatments, WASH_PRESETS.denim1_compensate);
  assert.equal(comp.mode, "compensate");
  assert.ok(Math.abs(comp.pieces[0].scale_warp_weft[0] - 1 / 0.9517) < 1e-12);
  assert.ok(Math.abs(comp.pieces[0].scale_warp_weft[1] - 1 / 0.945) < 1e-12);
  assert.equal(comp.seamsAfter[0].len_a_mm, 108.8);

  const d5 = runWashShrink(wasm, treatments, WASH_PRESETS.denim5_treat);
  assert.equal(d5.seamsAfter[0].len_a_mm, 101.9);

  // Unsupported material/treatment pair MUST refuse cleanly.
  assert.throws(
    () => runWashShrink(wasm, treatments, WASH_PRESETS.muslin_refusal),
    /does not match the transition BEFORE snapshot/i,
  );

  // Native CLI parity via `ftx materials treatment-pattern` + `ftx export svg`.
  const dir = mkdtempSync(path.join(tmpdir(), "ftx-wash-test-"));
  const round = wasm.run(
    JSON.stringify({ body: { mode: "chart", stature_cm: 92 }, program: "zouave_bolero", style: {} }),
    "2026-10-12",
    "2026-10-12T00:00:00Z",
  );
  let pat;
  try {
    pat = new TextDecoder().decode(round.file("pattern.json"));
  } finally {
    round.free();
  }
  const bound = JSON.parse(
    wasm.material_bind(
      pat,
      JSON.stringify({ schema: "ftx-material-bindings/1", default: treatments.denim1.before, pieces: {} }),
    ),
  ).pattern_json;
  const patPath = path.join(dir, "bolero-denim1.json");
  const reqPath = path.join(dir, "req.json");
  const outPatPath = path.join(dir, "treated.json");
  const outExportDir = path.join(dir, "exports");
  writeFileSync(patPath, bound);
  writeFileSync(reqPath, treat.frozenRequestJson);

  const cli = spawnSync(ftxBin, ["materials", "treatment-pattern", patPath, reqPath, outPatPath, "--json"], {
    encoding: "utf8",
  });
  assert.equal(cli.status, 0, `native treatment-pattern failed: ${cli.stderr || cli.stdout}`);
  const cliEnv = JSON.parse(cli.stdout);
  const cliJson = cliEnv.result ?? cliEnv;
  assert.equal(cliJson.pattern_sha256, treat.patternSha256);
  assert.equal(cliJson.request_sha256, treat.requestSha256);
  assert.deepEqual(cliJson.seams_after, treat.seamsAfter);

  const cliExport = spawnSync(
    ftxBin,
    ["export", "all", outPatPath, "--out", outExportDir, "--created", "2026-10-12T00:00:00Z"],
    { encoding: "utf8" },
  );
  assert.equal(cliExport.status, 0, `native export all failed: ${cliExport.stderr || cliExport.stdout}`);
  const nativeSvg = readFileSync(path.join(outExportDir, "ZOUAVE_BOLERO_master_mm.svg"), "utf8");
  const nativePdf = readFileSync(path.join(outExportDir, "ZOUAVE_BOLERO_A4.pdf"));
  assert.equal(new TextDecoder().decode(treat.files.svg.bytes), nativeSvg, "revised master SVG must match native export byte-for-byte");
  assert.deepEqual(Buffer.from(treat.files.pdf.bytes), nativePdf, "revised A4 PDF must match native export byte-for-byte");

  // Verify starter JSON matches live WASM output.
  const starter = JSON.parse(readFileSync(new URL("../public/labs/wash-starter.json", import.meta.url), "utf8"));
  assert.equal(starter.patternSha256, treat.patternSha256);
  assert.equal(starter.requestSha256, treat.requestSha256);
});

test("wash, shrink & recut page renders overlay, switches to compensate, refuses muslin, and fits mobile", async () => {
  const page = await browser.newPage({
    viewport: { width: 1120, height: 820 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/experiments/wash/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".ws-svg").waitFor({ timeout: 20000 });
    let text = await page.locator(".ws-kpis").innerText();
    assert.match(text, /-4\.83%/);
    assert.match(text, /-5\.50%/);
    assert.match(text, /103\.5 → 98\.5 mm/);

    // Switch to pre-wash compensation recut.
    await page.getByRole("button", { name: "Denim 1 · Pre-wash recut (compensate)" }).click();
    await page.waitForFunction(() => document.querySelector(".ws-kpis")?.innerText.includes("108.8 mm"));
    text = await page.locator(".ws-kpis").innerText();
    assert.match(text, /\+5\.08%/);
    assert.match(text, /\+5\.82%/);

    // Switch to unsupported material refusal.
    await page.getByRole("button", { name: "Cotton muslin + Denim 1 wash (refusal)" }).click();
    await page.locator(".ws-refusal-box").waitFor({ timeout: 20000 });
    assert.match(await page.locator(".ws-refusal-box").innerText(), /does not match the transition BEFORE snapshot/i);
    assert.equal(await page.locator(".ws-svg").count(), 0);
    assert.match(await page.locator(".ws-kpis").innerText(), /REFUSED/);

    // Recover to Denim 5.
    await page.getByRole("button", { name: "Denim 5 (0.5% elastane) · Post-wash" }).click();
    await page.waitForFunction(() => document.querySelector(".ws-kpis")?.innerText.includes("101.9 mm"));
    assert.equal(await page.locator(".ws-refusal-box").count(), 0);

    // Overflow checks desktop & mobile.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

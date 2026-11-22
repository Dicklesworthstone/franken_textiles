// Verify V&A 256-1880 1780s waistcoat reconstruction (ftx-n4x):
// collection provenance, selecting both a front and a pocket linking to exact cut-piece outlines
// & sewing landmarks, native ftx validate convergence, SVG opening at published dimensions, and phone layout.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/waistcoat.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { buildPieceOutline, getSeamsForInstance } from "../public/labs/waistcoat.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const NATIVE_FTX = path.join(REPO_ROOT, "target/release/ftx");

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

test("Waistcoat pattern converges natively, links fronts & pockets to landmarks, and exports SVG at published dimensions", () => {
  const pat = JSON.parse(readFileSync(new URL("../public/exhibits/waistcoat/waistcoat-pattern.json", import.meta.url), "utf8"));
  const man = JSON.parse(readFileSync(new URL("../public/exhibits/waistcoat/waistcoat-manifest.json", import.meta.url), "utf8"));
  const svg = readFileSync(new URL("../public/exhibits/waistcoat/waistcoat-master.svg", import.meta.url), "utf8");

  assert.equal(man.schema, "ftx-waistcoat-exhibit/1");
  assert.equal(man.provenance.accessionNumber, "256-1880");
  assert.equal(man.provenance.systemNumber, "O112019");
  assert.equal(man.provenance.publishedDimensionsMm.chestCircumference, 1075);
  assert.equal(man.provenance.publishedDimensionsMm.frontLength, 690);
  assert.equal(man.provenance.publishedDimensionsMm.backLength, 670);
  assert.equal(man.validation.status, "CONVERGED");
  assert.equal(man.validation.seamsPassing, 36);
  assert.equal(man.validation.truingPassing, man.validation.truingTotal);

  // Verify front and pocket outlines & landmarks
  const front = buildPieceOutline(pat, "wc_front", "R");
  assert.equal(front.widthMm, 275);
  assert.equal(front.heightMm, 690);
  assert.ok(front.internals.some(m => /136 mm/i.test(m.label)), "front must carry 136 mm pocket welt slash mark");
  assert.ok(front.notches.some(n => n.id === "pocket_level"));

  const welt = buildPieceOutline(pat, "wc_welt", "R");
  assert.equal(welt.widthMm, 136);
  assert.equal(welt.heightMm, 48);
  assert.ok(welt.internals.some(m => m.kind === "fold"), "welt must carry lengthwise fold line");
  assert.ok(getSeamsForInstance(man, "wc_welt.R").some(s => s.id === "welt_to_bag_R"));

  // SVG opens at exact published mm dimensions
  assert.match(svg, new RegExp(`width="${man.svgDimensions.widthAttr}"`));
  assert.match(svg, new RegExp(`height="${man.svgDimensions.heightAttr}"`));
  assert.match(svg, new RegExp(`viewBox="${man.svgDimensions.viewBox}"`));

  // Native CLI re-validation agrees 100%
  if (existsSync(NATIVE_FTX)) {
    const base = path.join(REPO_ROOT, "showcase/public/exhibits/waistcoat");
    const env = JSON.parse(
      execFileSync(
        NATIVE_FTX,
        [
          "validate",
          path.join(base, "waistcoat-pattern.json"),
          "--measurements",
          path.join(base, "waistcoat-measures.json"),
          "--fit",
          path.join(base, "waistcoat-fit.json"),
          "--json",
        ],
        { encoding: "utf8" },
      ),
    );
    assert.equal(env.ok, true);
    assert.equal(env.result.status, "CONVERGED");
    assert.equal(env.result.seams.filter(s => s.status === "PASS").length, 36);
  }
});

test("Waistcoat page selects both front and pocket pieces, steps welt pocket stages, serves SVG at published dimensions, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/wardrobe/waistcoat/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-metric="selected-instance"]').waitFor({ timeout: 15000 });

    // Front selected initially: verify outline dims (275 x 690 mm) and pocket slash landmark
    assert.match(await page.locator('[data-metric="selected-instance"]').innerText(), /wc_front\.R/i);
    assert.equal(await page.locator('[data-metric="selected-dims"]').innerText(), "275 × 690 MM");
    assert.match(await page.locator(".wc-landmarks").innerText(), /Pocket welt slash/i);

    // Select pocket welt piece and confirm cut-piece outline & lengthwise fold landmark update
    await page.locator('button[data-piece="wc_welt"]').click();
    assert.match(await page.locator('[data-metric="selected-instance"]').innerText(), /wc_welt\.R/i);
    assert.equal(await page.locator('[data-metric="selected-dims"]').innerText(), "136 × 48 MM");
    assert.match(await page.locator(".wc-landmarks").innerText(), /Welt lengthwise fold/i);

    // Select pocket bag and confirm dimensions & linked pocket seams
    await page.locator('button[data-piece="wc_pocket_outer"]').click();
    assert.equal(await page.locator('[data-metric="selected-dims"]').innerText(), "136 × 156 MM");

    // Step through welt pocket transformation to stage 4 (visible stitched welt)
    await page.locator('button[data-welt-stage="finish"]').click();
    assert.match(await page.locator('[data-metric="welt-stage-title"]').innerText(), /Stage 4/i);

    // SVG download matches published dimensions
    const svgRes = await page.request.get(new URL("/exhibits/waistcoat/waistcoat-master.svg", site).href);
    assert.equal(svgRes.status(), 200);
    const svgText = await svgRes.text();
    assert.match(svgText, /width="1139\.12mm"/);
    assert.match(svgText, /height="1570\.648mm"/);

    // Desktop & mobile overflow + single aria-current check
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

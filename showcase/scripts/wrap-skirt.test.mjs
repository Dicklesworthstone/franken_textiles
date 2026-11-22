// Verify adjustable-overlap wrap skirt flat pattern lengths, tie placements, neutral vs seated
// native Anny slices, printable A4/SVG exports, and phone layout (ftx-kod).
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/wrap-skirt.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { evaluateWrapSetting } from "../public/labs/wrap-skirt.mjs";

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

test("Wrap skirt pattern passes all 7 native seams, bounds tie placements, and differentiates neutral vs seated Anny slices", () => {
  const exhibit = JSON.parse(readFileSync(new URL("../public/exhibits/wrap-skirt/forms.json", import.meta.url), "utf8"));
  assert.equal(exhibit.schema, "ftx-wrap-skirt-exhibit/1");
  assert.equal(exhibit.forms.length, 3);

  // Every form has all 7 seams passing with 0.0 mm residual.
  for (const f of exhibit.forms) {
    assert.equal(f.flat_validation.seams.length, 7);
    assert.ok(f.flat_validation.seams.every(s => s.status === "PASS" && s.residual_mm === 0));
  }

  // Reference adult in neutral rest pose with firm tie (+20 mm): secure overlap & generous bow tail.
  const refRest = evaluateWrapSetting(exhibit, { formId: "reference", posture: "rest", easeMm: 20, slotShiftMm: 0 });
  assert.equal(refRest.slotInBounds, true);
  assert.ok(refRest.waistOverlapMm > 270 && refRest.waistOverlapMm < 290, `got ${refRest.waistOverlapMm}`);
  assert.ok(refRest.minTieTailMm > 350, `got ${refRest.minTieTailMm}`);
  assert.equal(refRest.status, "SECURE");

  // Comparing a second looser tie setting (+65 mm ease) reduces front waist overlap by exactly 45 mm.
  const refLoose = evaluateWrapSetting(exhibit, { formId: "reference", posture: "rest", easeMm: 65, slotShiftMm: 0 });
  assert.ok(Math.abs((refRest.waistOverlapMm - refLoose.waistOverlapMm) - 45) < 0.1);

  // Seated pose increases the horizontal hip/thigh perimeter significantly vs rest and surfaces explicit limits.
  const refSeated = evaluateWrapSetting(exhibit, { formId: "reference", posture: "seated", easeMm: 20, slotShiftMm: 0 });
  assert.ok(
    refSeated.hipSlicePerimMm > refRest.hipSlicePerimMm + 200 || refSeated.lapSlicePerimMm > refRest.lapSlicePerimMm + 50,
    "seated Anny slice must reflect forward thigh projection",
  );
  assert.ok(refSeated.limits.length >= 1, "seated pose must surface explicit coverage limits on the fixed wrap skirt");

  // Out-of-bounds tie slot placement is explicitly flagged as LIMIT_EXCEEDED.
  const badSlot = evaluateWrapSetting(exhibit, { formId: "reference", posture: "rest", easeMm: 20, slotShiftMm: 320 });
  assert.equal(badSlot.slotInBounds, false);
  assert.equal(badSlot.status, "LIMIT_EXCEEDED");

  // Native CLI re-validation of the committed wrap-skirt-pattern.json agrees with 7/7 PASS seams.
  if (existsSync(NATIVE_FTX)) {
    const patPath = path.join(REPO_ROOT, "showcase/public/exhibits/wrap-skirt/wrap-skirt-pattern.json");
    const fitPath = path.join(REPO_ROOT, "showcase/public/exhibits/wrap-skirt/wrap-skirt-fit.json");
    const measPath = path.join(REPO_ROOT, "showcase/public/exhibits/sundress/dress-measures.json");
    const env = JSON.parse(
      execFileSync(NATIVE_FTX, ["validate", patPath, "--measurements", measPath, "--fit", fitPath, "--json"], {
        encoding: "utf8",
      }),
    );
    assert.equal(env.ok, true);
    assert.equal(env.result.status, "CONVERGED");
    assert.equal(env.result.seams.filter(s => s.status === "PASS").length, 7);
  }
});

test("Wrap skirt page compares two tie settings, neutral vs seated poses, serves A4 PDF/SVG, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1200, height: 860 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/studies/wrap-skirt/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-metric="waist-overlap-a"]').waitFor({ timeout: 15000 });

    const ovA = await page.locator('[data-metric="waist-overlap-a"]').innerText();
    const ovB = await page.locator('[data-metric="waist-overlap-b"]').innerText();
    assert.notEqual(ovA, ovB, "Setting A and Setting B must display distinct front overlaps");
    assert.equal(await page.locator('[data-metric="status-a"]').innerText(), "SECURE");

    // Switching to Seated posture explicitly surfaces limits.
    await page.locator('button[data-posture="seated"]').click();
    await page.waitForFunction(() => Number(document.querySelector(".ws-limits")?.getAttribute("data-limits-count") ?? "0") >= 1);

    // Switching wearer form to Full (82/106 cm) reduces front overlap while pattern stays fixed.
    await page.locator('button[data-form="full"]').click();
    const fullOv = await page.locator('[data-metric="waist-overlap-a"]').innerText();
    assert.notEqual(fullOv, ovA);

    // Printable A4 PDF and master SVG return HTTP 200 with valid headers.
    const pdfRes = await page.request.get(new URL("/exhibits/wrap-skirt/wrap-skirt-A4.pdf", site).href);
    assert.equal(pdfRes.status(), 200);
    assert.equal((await pdfRes.body()).subarray(0, 5).toString(), "%PDF-");
    const svgRes = await page.request.get(new URL("/exhibits/wrap-skirt/wrap-skirt-master.svg", site).href);
    assert.equal(svgRes.status(), 200);
    assert.match(await svgRes.text(), /<svg/);

    // Desktop & mobile overflow + single aria-current check.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

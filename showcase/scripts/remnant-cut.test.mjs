// Verify two-job finite-stock remnant cutting (ftx-83qx) against the native CLI and in Chromium.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/remnant-cut.test.mjs
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { REMNANT_PRESETS, buildSheetRequest, initialRemnant, planRemnant } from "../public/labs/remnant-worker.mjs";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const repoRoot = path.resolve(import.meta.dirname, "../..");
const ftxMaterialsBin = process.env.FTX_MATERIALS_BIN || path.join(repoRoot, "target/release/ftx-materials");
const wasmPkg = process.env.FTX_WASM_PKG || path.resolve(import.meta.dirname, "../public/workbench/pkg");

function pointInRing(pt, ring) {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    const cross = (yi > pt[1]) !== (yj > pt[1]) && pt[0] < ((xj - xi) * (pt[1] - yi)) / (yj - yi) + xi;
    if (cross) inside = !inside;
  }
  return inside;
}

function runJson(bin, args) {
  const proc = spawnSync(bin, args, { encoding: "utf8" });
  return { status: proc.status, stdout: proc.stdout, stderr: proc.stderr };
}

let browser, wasm;
before(async () => {
  wasm = await import(pathToFileURL(path.join(wasmPkg, "ftx_wasm.js")));
  await wasm.default({ module_or_path: await readFile(path.join(wasmPkg, "ftx_wasm_bg.wasm")) });
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});

after(async () => {
  await browser?.close();
});

test("remnant planner matches native ftx-materials stock-plan/stock-residual and blocks overlaps", () => {
  const res = planRemnant(wasm, initialRemnant);
  assert.equal(res.stock.id, "muslin-sheet-01");
  assert.equal(res.job1.occupiedCount, 5);
  assert.ok(res.job2, `expected Job 2 fez to fit in bolero remnant, refusal=${res.job2Refusal}`);
  assert.equal(res.job2.stockId, "muslin-sheet-01");
  assert.equal(res.job2.occupiedPolygons.length, 5);
  assert.equal(res.job2.placements.length, 2);
  assert.match(res.job1.occupiedBasis, /^ftx-finite-stock-plan\/1:[0-9a-f]{64}$/);
  assert.match(res.job2.occupiedBasisOut, /^ftx-finite-stock-plan\/1:[0-9a-f]{64} -> ftx-finite-stock-plan\/1:[0-9a-f]{64}$/);
  assert.ok(res.job2.combinedUtilization > res.job1.utilization);
  assert.ok(res.freshRollJob2.stockAreaM2 > 0.15);

  // Verify no Job 2 vertex falls inside any Job 1 occupied polygon.
  for (const p2 of res.job2.placements) {
    for (const pt of p2.cut_mm) {
      for (const occ of res.job2.occupiedPolygons) {
        assert.equal(pointInRing(pt, occ), false, `Job 2 piece ${p2.instance} overlapped Job 1 occupied polygon`);
      }
    }
  }

  // Verify native CLI parity against `ftx-materials stock-plan` & `stock-residual`.
  const dir = mkdtempSync(path.join(tmpdir(), "ftx-remnant-test-"));
  const draftAndBind = (program) => {
    const round = wasm.run(
      JSON.stringify({ body: { mode: "chart", stature_cm: 92 }, program, style: {} }),
      "2026-10-12",
      "2026-10-12T00:00:00Z",
    );
    let pat;
    try {
      pat = new TextDecoder().decode(round.file("pattern.json"));
    } finally {
      round.free();
    }
    return JSON.parse(
      wasm.material_bind(
        pat,
        JSON.stringify({ schema: "ftx-material-bindings/1", default: { id: "dharma/cotton-muslin-3.5oz" }, pieces: {} }),
      ),
    ).pattern_json;
  };
  const boleroPath = path.join(dir, "bolero.json");
  const fezPath = path.join(dir, "fez.json");
  writeFileSync(boleroPath, draftAndBind("zouave_bolero"), { flag: "wx" });
  writeFileSync(fezPath, draftAndBind("fez"), { flag: "wx" });
  const runNative = (op, pattern, request) => {
    const result = runJson(ftxMaterialsBin, [op, pattern, request]);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    return JSON.parse(result.stdout);
  };
  for (const [id, count] of Object.entries({ bolero_fez: 2, bolero_fez_2x: 4, bolero_bolero_refusal: 0, bolero_bolero_large: 5 })) {
    const preset = REMNANT_PRESETS[id];
    const plan = id === "bolero_fez" ? res : planRemnant(wasm, preset);
    const req1Path = path.join(dir, `${id}-job1.json`);
    const req2Path = path.join(dir, `${id}-job2.json`);
    writeFileSync(req1Path, JSON.stringify(buildSheetRequest(preset)), { flag: "wx" });
    const native1 = runNative("stock-plan", boleroPath, req1Path);
    assert.equal(native1.request_sha256, plan.job1.requestSha256);
    assert.equal(native1.markers[0].id, plan.stock.id);
    const residual1 = runNative("stock-residual", boleroPath, req1Path);
    assert.deepEqual(residual1, JSON.parse(plan.job1.residualJson));
    assert.equal(residual1.stocks[0].occupied_mm.length, 5);
    residual1.options.copies = preset.job2Copies;
    assert.deepEqual(residual1, JSON.parse(plan.job2RequestJson));
    writeFileSync(req2Path, plan.job2RequestJson, { flag: "wx" });

    const job2Pattern = preset.job2Garment === "fez" ? fezPath : boleroPath;
    assert.equal(plan.job2PatternJson, readFileSync(job2Pattern, "utf8"));
    if (!count) {
      assert.equal(plan.job2, null);
      assert.match(plan.job2Refusal, /no checked finite-stock placement/i);
      const native = runJson(ftxMaterialsBin, ["stock-plan", job2Pattern, req2Path]);
      assert.notEqual(native.status, 0);
      assert.match(JSON.parse(native.stdout).refusal.message, /no checked finite-stock placement/i);
      continue;
    }
    assert.equal(plan.job2Refusal, null);
    assert.equal(plan.job2.placements.length, count);
    const native2 = runNative("stock-plan", job2Pattern, req2Path);
    assert.equal(native2.request_sha256, plan.job2.requestSha256);
    assert.equal(native2.markers[0].id, plan.stock.id);
    assert.equal(native2.markers[0].placements.length, count);
    assert.deepEqual(native2.markers[0].occupied_mm, residual1.stocks[0].occupied_mm);
    const residual2 = runNative("stock-residual", job2Pattern, req2Path);
    assert.deepEqual(residual2, JSON.parse(plan.job2.residualJson));
    assert.equal(residual2.stocks[0].occupied_mm.length, 5 + count);
  }

  // Starter JSON matches live WASM output.
  const starter = JSON.parse(readFileSync(new URL("../public/labs/remnant-starter.json", import.meta.url), "utf8"));
  assert.deepEqual(starter, res);
});

test("CutLab remnant mode renders real occupied polygons, handles refusal, and fits desktop and mobile", async () => {
  const page = await browser.newPage({
    viewport: { width: 1120, height: 820 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/sections/cut/?mode=remnant", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".remnant-lab").waitFor();
    await page.locator(".remnant-new").first().waitFor({ timeout: 20000 });
    assert.equal(await page.locator(".remnant-occupied").count(), 5);
    assert.equal(await page.locator(".remnant-new").count(), 2);
    const bodyText = await page.locator(".remnant-lab").innerText();
    assert.match(bodyText, /muslin-sheet-01/);
    assert.match(bodyText, /occupied_basis/);

    const requestFrom = async suffix => {
      const link = page.locator(`a[download$="${suffix}"]`);
      await link.waitFor();
      return link.evaluate(async a => (await fetch(a.href)).json());
    };
    const afterFez = await requestFrom("-residual-request.json");
    assert.equal(afterFez.schema, "ftx-finite-stock-request/1");
    assert.equal(afterFez.stocks[0].id, "muslin-sheet-01");
    assert.equal(afterFez.stocks[0].occupied_mm.length, 7);
    assert.match(bodyText, /Search grid 30 mm/);
    assert.equal((await requestFrom("fez-pattern.json")).schema, "fabric-pattern/1");
    for (const garment of ["Bolero", "Fez"]) {
      const href = await page.getByRole("link", { name: garment, exact: true }).getAttribute("href");
      const pdf = await page.request.get(new URL(href, site).href);
      assert.equal(pdf.status(), 200);
      assert.equal((await pdf.body()).subarray(0, 5).toString(), "%PDF-");
    }

    // Switch to refusal scenario: Job 2 must show 0 new pieces and an explicit refusal alert.
    await page.getByRole("button", { name: "Bolero → 2nd Bolero (refusal)" }).click();
    await page.locator(".remnant-refusal").waitFor({ timeout: 25000 });
    assert.equal(await page.locator(".remnant-occupied").count(), 5);
    assert.equal(await page.locator(".remnant-new").count(), 0);
    assert.match(await page.locator(".remnant-refusal").innerText(), /no checked finite-stock placement/i);
    assert.equal((await requestFrom("-residual-request.json")).stocks[0].occupied_mm.length, 5);

    // Recover onto a larger sheet or 2x Fez caps.
    await page.getByRole("button", { name: "Bolero → 2× Fez caps" }).click();
    await page.locator(".remnant-new").nth(3).waitFor({ timeout: 25000 });
    assert.equal(await page.locator(".remnant-occupied").count(), 5);
    assert.equal(await page.locator(".remnant-new").count(), 4);
    assert.equal(await page.locator(".remnant-refusal").count(), 0);
    const replay = await requestFrom("-job2-request.json");
    assert.equal(replay.options.copies, 2);
    assert.equal(replay.stocks[0].occupied_mm.length, 5);
    const afterTwoFezes = await requestFrom("-residual-request.json");
    assert.equal(afterTwoFezes.stocks[0].occupied_mm.length, 9);
    assert.deepEqual(afterTwoFezes.stocks[0].occupied_mm.slice(0, 5), replay.stocks[0].occupied_mm);

    await page.getByRole("button", { name: "Bolero → 2nd Bolero (1100 mm sheet)" }).click();
    await page.locator(".remnant-new").nth(4).waitFor({ timeout: 25000 });
    const afterBolero = await requestFrom("-residual-request.json");
    assert.equal(afterBolero.stocks[0].id, "muslin-sheet-02");
    assert.equal(afterBolero.stocks[0].occupied_mm.length, 10);

    // Desktop & mobile overflow checks.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

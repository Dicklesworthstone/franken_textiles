// Verify the Thermal Sandwich exhibit agrees with native `ftx materials thermal-steady`
// for 1 layer, 2 layers, reversed boundary gradients, and missing-interface refusal.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const repoRoot = resolve(import.meta.dirname, "../..");
const ftxBin = process.env.FTX_BIN || join(repoRoot, "target/release/ftx");

function runNativeSteady(request, expectedExit = 0) {
  const dir = mkdtempSync(join(tmpdir(), "ftx-thermal-test-"));
  const file = join(dir, "req.json");
  writeFileSync(file, JSON.stringify(request), { flag: "wx" });
  const run = spawnSync(ftxBin, ["materials", "thermal-steady", file, "--json"], {
    encoding: "utf8",
    cwd: repoRoot,
  });
  assert.equal(run.status, expectedExit, run.stderr || run.stdout);
  const reply = JSON.parse(run.stdout);
  return expectedExit === 0 ? reply.result : reply.refusal;
}

let browser;
before(async () => {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"],
  });
});
after(async () => { await browser?.close(); });

const opts = { userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0", viewport: { width: 1120, height: 820 } };

test("thermal sandwich matches native steady conduction for 1 layer, 2 layers, reversed gradient, and refuses missing interface", async () => {
  const oneLayerNative = runNativeSteady({
    schema: "ftx-thermal-steady/1",
    layers: [{ id: "zagreb/neoprene-2025/m3nob5", allow_estimates: false }],
    interface_resistance_m2_k_w: [],
    area_m2: 0.6,
    inside_c: 35.0,
    outside_c: 20.0,
    inside_resistance_m2_k_w: 0.03,
    outside_resistance_m2_k_w: 0.05,
  });
  const twoLayerRequest = {
    schema: "ftx-thermal-steady/1",
    layers: [
      { id: "zagreb/neoprene-2025/m3nob5", allow_estimates: false },
      { id: "zagreb/neoprene-2025/m1no5", allow_estimates: false },
    ],
    interface_resistance_m2_k_w: [0.02],
    area_m2: 0.6,
    inside_c: 35.0,
    outside_c: 20.0,
    inside_resistance_m2_k_w: 0.03,
    outside_resistance_m2_k_w: 0.05,
  };
  const twoLayerNative = runNativeSteady(twoLayerRequest);
  const reversedNative = runNativeSteady({
    ...twoLayerRequest,
    inside_c: 15.0,
    outside_c: 35.0,
  });
  const missingInterface = runNativeSteady({ ...twoLayerRequest, interface_resistance_m2_k_w: [] }, 3);

  const page = await browser.newPage({ ...opts, reducedMotion: "reduce" });
  const errors = [];
  page.on("pageerror", e => errors.push(String(e)));
  let releaseWorker;
  const workerGate = new Promise(resolve => { releaseWorker = resolve; });
  await page.route("**/labs/thermal-worker.mjs", async route => {
    await workerGate;
    await route.continue();
  });

  try {
    await page.goto(new URL("/experiments/thermal/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator('[data-metric="flux"]').filter({ hasText: `${oneLayerNative.state.flux_w_m2.toFixed(2)} W/m²` }).waitFor({ timeout: 30000 });
    assert.ok((await page.locator('[data-metric="layer-0-faces"]').innerText()).includes(
      `${oneLayerNative.state.layer_faces_c[0][0].toFixed(2)} → ${oneLayerNative.state.layer_faces_c[0][1].toFixed(2)} °C`,
    ));
    assert.ok((await page.locator(".th-sha").innerText()).includes(oneLayerNative.request_sha256));

    // Check aria-current uniqueness in header and mobile menu sheet
    const currents = await page.evaluate(() => ({
      header: document.querySelectorAll("header a[aria-current]").length,
      sheet: document.querySelectorAll("dialog.ft-sheet a[aria-current]").length,
    }));
    assert.equal(currents.header, 1);
    assert.equal(currents.sheet, 1);

    // 2-layer sandwich live via Rust WASM
    await page.locator(".th-downloads a").first().waitFor();
    const loadingWorker = page.waitForRequest("**/labs/thermal-worker.mjs");
    await page.getByRole("button", { name: "Two-sheet sandwich" }).click();
    await loadingWorker;
    assert.doesNotMatch(await page.locator('[data-metric="flux"]').innerText(), /W\/m²/);
    assert.equal(await page.locator(".th-downloads a").count(), 0);
    releaseWorker();
    await page.locator('[data-metric="flux"]').filter({ hasText: `${twoLayerNative.state.flux_w_m2.toFixed(2)} W/m²` }).waitFor({ timeout: 60000 });
    assert.ok((await page.locator('[data-metric="layer-0-faces"]').innerText()).includes(
      `${twoLayerNative.state.layer_faces_c[0][0].toFixed(2)} → ${twoLayerNative.state.layer_faces_c[0][1].toFixed(2)} °C`,
    ));
    assert.ok((await page.locator('[data-metric="layer-1-faces"]').innerText()).includes(
      `${twoLayerNative.state.layer_faces_c[1][0].toFixed(2)} → ${twoLayerNative.state.layer_faces_c[1][1].toFixed(2)} °C`,
    ));
    assert.ok((await page.locator(".th-sha").innerText()).includes(twoLayerNative.request_sha256));
    const frozen = await page.locator('a[download="thermal-sandwich-request.json"]').evaluate(async link =>
      (await fetch(link.href)).json());
    const replayed = runNativeSteady(frozen);
    assert.equal(replayed.request_sha256, twoLayerNative.request_sha256);
    assert.equal(replayed.state.flux_w_m2, twoLayerNative.state.flux_w_m2);

    // Reversed gradient (inside 15 °C < outside 35 °C) -> signed negative flux matches CLI
    await page.getByLabel("Inside reservoir temperature in degrees Celsius").fill("15");
    await page.getByLabel("Outside reservoir temperature in degrees Celsius").fill("35");
    await page.locator('[data-metric="flux"]').filter({ hasText: `${reversedNative.state.flux_w_m2.toFixed(2)} W/m²` }).waitFor({ timeout: 30000 });
    assert.ok(reversedNative.state.flux_w_m2 < 0);

    // Omit explicit interface resistance -> blocked by native Rust refusal, no stale numeric KPIs
    await page.getByRole("button", { name: "Omit interface (refusal)" }).click();
    const alert = page.locator(".th-refusal-banner[role='alert']");
    await alert.waitFor({ timeout: 30000 });
    assert.ok((await alert.innerText()).includes(missingInterface.message));
    assert.equal(await page.locator('[data-metric="flux"]').count(), 0);

    // Re-enabling explicit interface resistance restores the valid 2-layer solve
    await page.getByLabel("Supply explicit interface resistance (required for 2 layers)").check();
    await page.locator('[data-metric="flux"]').filter({ hasText: `${twoLayerNative.state.flux_w_m2.toFixed(2)} W/m²` }).waitFor({ timeout: 30000 });

    // Responsive overflow check
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    assert.deepEqual(errors, []);
  } finally {
    releaseWorker();
    await page.close();
  }
});

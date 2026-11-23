// Verify the Seam Repair studio agrees with native `ftx draft` / `ftx alter move` / `ftx alter true-seam` / `ftx validate`
// and verifies original, broken, and repaired reports plus full-size PDF/SVG exports.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const repoRoot = resolve(import.meta.dirname, "../..");
const ftxBin = process.env.FTX_BIN || join(repoRoot, "target/release/ftx");

function runNativeRepairSequence(mismatchMm) {
  const dir = mkdtempSync(join(tmpdir(), "ftx-repair-test-"));
  const m = join(dir, "m.json");
  const bolero = join(dir, "bolero");
  const broken = join(dir, "broken.json");
  const repaired = join(dir, "repaired.json");
  execFileSync(ftxBin, ["measure", "reference", "--stature-mm", "920", "--age-months", "30", "--out", m], { cwd: repoRoot });
  execFileSync(ftxBin, ["draft", "zouave_bolero", "--measurements", m, "--out", bolero], { cwd: repoRoot });
  const pat = join(bolero, "pattern.json");
  const fit = join(bolero, "fit_spec.json");
  const validate = (path, expectedExit, expectedStatus) => {
    const checked = spawnSync(ftxBin, ["validate", path, "--measurements", m, "--fit", fit, "--json"], { encoding: "utf8", cwd: repoRoot });
    assert.equal(checked.status, expectedExit, checked.stderr);
    const { result } = JSON.parse(checked.stdout);
    assert.equal(result.status, expectedStatus);
    return result;
  };
  const orig = validate(pat, 0, "CONVERGED");
  execFileSync(ftxBin, ["alter", "move", pat, "--piece", "back", "--vertex", "side_hem", "--dx", "0", "--dy", String(-mismatchMm), "--out", broken], { cwd: repoRoot });
  const brk = validate(broken, 2, "NOT_CONVERGED");
  execFileSync(ftxBin, ["alter", "true-seam", broken, "--seam", "side_R", "--end", "start", "--out", repaired], { cwd: repoRoot });
  const rep = validate(repaired, 0, "CONVERGED");
  return { orig, brk, rep, dir, repaired };
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

test("seam repair studio matches native CLI for original, broken, and repaired stages and produces valid full-size PDF/SVG", async () => {
  const cli20 = runNativeRepairSequence(20);
  const cli30 = runNativeRepairSequence(30);
  const sideOf = r => r.seams.find(s => s.seam === "side_R");

  const page = await browser.newPage({ ...opts, reducedMotion: "reduce" });
  const flatStatus = () => page.locator('[data-metric="flat-status"]').evaluate(el => el.firstChild?.textContent?.trim());
  const errors = [];
  page.on("pageerror", e => errors.push(String(e)));

  try {
    await page.goto(new URL("/sections/repair/", site).href, { waitUntil: "domcontentloaded" });
    // Default stage is "broken" at +20 mm
    await page.locator('[data-metric="flat-status"]').filter({ hasText: cli20.brk.status }).waitFor({ timeout: 60000 });
    assert.equal(await flatStatus(), cli20.brk.status);
    assert.ok((await page.locator('[data-metric="len-a"]').innerText()).includes(`${sideOf(cli20.brk).len_a_mm.toFixed(1)} mm`));
    assert.ok((await page.locator('[data-metric="len-b"]').innerText()).includes(`${sideOf(cli20.brk).len_b_mm.toFixed(1)} mm`));
    assert.ok((await page.locator('[data-metric="residual"]').innerText()).includes(`${sideOf(cli20.brk).residual_mm.toFixed(1)} mm`));

    // Verify SVG distinguishes cut/seam-allowance contours from walked sewing lines and active seam
    assert.equal(await page.locator(".rl-svg path.rl-sa-fill").count(), 2);
    assert.equal(await page.locator(".rl-svg path.rl-sew-fill").count(), 2);
    assert.equal(await page.locator(".rl-svg path.rl-seam-active[data-state='fail']").count(), 2);

    // Stage 01: Original
    await page.getByRole("button", { name: /Original drafted cut/ }).click();
    assert.equal(await flatStatus(), cli20.orig.status);
    assert.ok((await page.locator('[data-metric="len-b"]').innerText()).includes(`${sideOf(cli20.orig).len_b_mm.toFixed(1)} mm`));

    // Stage 03: Repaired via true-seam
    await page.getByRole("button", { name: /Apply true-seam repair/ }).click();
    assert.equal(await flatStatus(), cli20.rep.status);
    assert.ok((await page.locator('[data-metric="len-b"]').innerText()).includes(`${sideOf(cli20.rep).len_b_mm.toFixed(1)} mm`));
    assert.ok((await page.locator('[data-metric="residual"]').innerText()).includes("+0.0 mm"));
    assert.equal(await page.locator(".rl-svg path.rl-seam-active[data-state='pass']").count(), 2);

    const nativeExports = join(cli20.dir, "native-exports");
    execFileSync(ftxBin, ["export", "all", cli20.repaired, "--out", nativeExports, "--created", "2026-10-12T00:00:00Z"], { cwd: repoRoot });
    const [pdf, svg] = await page.evaluate(async () => {
      const links = Array.from(document.querySelectorAll(".rl-downloads a"));
      const pdfLink = links.find(a => a.textContent.includes("PDF"));
      const svgLink = links.find(a => a.textContent.includes("SVG"));
      const pdfBytes = new Uint8Array(await (await fetch(pdfLink.href)).arrayBuffer());
      const svg = await (await fetch(svgLink.href)).text();
      return [Array.from(pdfBytes), svg];
    });
    assert.deepEqual(Buffer.from(pdf), readFileSync(join(nativeExports, "ZOUAVE_BOLERO_A4.pdf")));
    assert.equal(svg, readFileSync(join(nativeExports, "ZOUAVE_BOLERO_master_mm.svg"), "utf8"));

    // Change mismatch to +30 mm and check live broken & repaired recomputation
    await page.getByRole("button", { name: "+30 mm", exact: true }).click();
    await page.getByRole("button", { name: /Break the side seam/ }).click();
    await page.locator('[data-metric="len-b"]').filter({ hasText: `${sideOf(cli30.brk).len_b_mm.toFixed(1)} mm` }).waitFor({ timeout: 30000 });
    await page.getByRole("button", { name: /Apply true-seam repair/ }).click();
    await page.locator('[data-metric="flat-status"]').filter({ hasText: /^CONVERGED/ }).waitFor({ timeout: 30000 });
    assert.equal(await flatStatus(), cli30.rep.status);

    // Responsive check
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);

    // The two visual-fitting cases are told on the how-it-works page, each under its own anchor.
    await page.setViewportSize({ width: 1120, height: 820 });
    await page.goto(new URL("/how-it-works/", site).href, { waitUntil: "domcontentloaded" });
    for (const anchor of ["visual-fitting", "full-hand-bolero", "raised-arm-start"])
      assert.equal(await page.locator(`#${anchor}`).count(), 1, anchor);
    assert.deepEqual(errors, []);
  } finally {
    await page.close();
  }
});

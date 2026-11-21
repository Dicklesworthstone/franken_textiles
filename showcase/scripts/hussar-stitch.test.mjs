// Verify Tajima DST panel-to-surface registration onto the 1761 hussar dolman (ftx-f9t).
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/hussar-stitch.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import {
  RENDER_TOLERANCE_MM,
  decodeDst,
  registerDstOnDolmanPanel,
  unpackDolmanBinary,
} from "../public/labs/dolman-stitch.mjs";

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

test("DST placement and couching stitches register onto dolman_front R & L within 0.25 mm while leaving jumps and flat sampler unmatched", async () => {
  const pack = JSON.parse(await readFile(new URL("../public/exhibits/hussar/dolman.json", import.meta.url), "utf8"));
  const unpacked = unpackDolmanBinary(
    gunzipSync(await readFile(new URL("../public/exhibits/hussar/dolman.gz", import.meta.url))),
    pack,
  );

  const evalAnchor3d = (triIdx, w0, w1) => {
    const a = pack.triangles[triIdx * 3], b = pack.triangles[triIdx * 3 + 1], c = pack.triangles[triIdx * 3 + 2];
    const w2 = 1 - w0 - w1;
    const xyz = unpacked.clothUpright;
    return [
      w0 * xyz[a * 3] + w1 * xyz[b * 3] + w2 * xyz[c * 3],
      w0 * xyz[a * 3 + 1] + w1 * xyz[b * 3 + 1] + w2 * xyz[c * 3 + 1],
      w0 * xyz[a * 3 + 2] + w1 * xyz[b * 3 + 2] + w2 * xyz[c * 3 + 2],
    ];
  };
  const ptSegDist = (p, a, b) => {
    const vx = b[0] - a[0], vy = b[1] - a[1], vz = b[2] - a[2];
    const l2 = vx * vx + vy * vy + vz * vz;
    const t = l2 > 0 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * vx + (p[1] - a[1]) * vy + (p[2] - a[2]) * vz) / l2)) : 0;
    return Math.hypot(p[0] - (a[0] + t * vx), p[1] - (a[1] + t * vy), p[2] - (a[2] + t * vz));
  };

  for (const [panel, rel] of [
    ["dolman_front.R", "../public/exhibits/hussar/dolman-front-R.dst"],
    ["dolman_front.L", "../public/exhibits/hussar/dolman-front-L.dst"],
  ]) {
    const dst = decodeDst(await readFile(new URL(rel, import.meta.url)));
    assert.equal(dst.stopIndex, 295);
    const reg = registerDstOnDolmanPanel(dst, pack, unpacked, panel, RENDER_TOLERANCE_MM);
    assert.equal(reg.registered, true);
    assert.equal(reg.registeredStitches, 2172);
    assert.equal(reg.unmatchedStitches, 0);
    assert.equal(reg.flatOnlyJumps, 76); // 75 jump records + 1 braid stop stay on flat board
    assert.ok(reg.maxResidualMm <= RENDER_TOLERANCE_MM, `${panel} residual ${reg.maxResidualMm} > ${RENDER_TOLERANCE_MM}`);

    // Validate multiple registered needle positions across placement & couching passes against independent 3D dolman cords.
    const cordSegs = [];
    for (const cord of pack.cords.filter(c => c.instance === panel)) {
      const pts = [];
      for (let i = 0; i < cord.anchors.length; i += 3) {
        pts.push(evalAnchor3d(cord.anchors[i], cord.anchors[i + 1], cord.anchors[i + 2]));
      }
      for (let i = 1; i < pts.length; i++) cordSegs.push([pts[i - 1], pts[i]]);
    }
    const couch = reg.points.filter(p => p.onSurface && p.pass === "couching");
    let sumMm = 0;
    for (let i = 0; i < couch.length; i += 25) {
      let best = Infinity;
      for (const [a, b] of cordSegs) {
        const d = ptSegDist(couch[i].pos3d, a, b) * 1000;
        if (d < best) best = d;
      }
      assert.ok(best < 0.75, `${panel} couch stitch ${couch[i].index} drifted ${best} mm from 3D cord`);
      sumMm += best;
    }
    assert.ok(sumMm / Math.ceil(couch.length / 25) < 0.1, `mean 3D cord drift too high on ${panel}`);

    // Every jump & stop record must remain off the 3D surface (`onSurface: false`).
    for (const r of reg.points.filter(p => p.kind !== "stitch")) {
      assert.equal(r.onSurface, false);
      assert.equal(r.pos3d, null);
    }
  }

  // Standalone flat sampler has NO panel correspondence and leaves all stitches strictly on the flat sample.
  const sampler = decodeDst(await readFile(new URL("../public/wardrobe/hussar/braid-sampler.dst", import.meta.url)));
  const samplerReg = registerDstOnDolmanPanel(sampler, pack, unpacked, null, RENDER_TOLERANCE_MM);
  assert.equal(samplerReg.registered, false);
  assert.equal(samplerReg.registeredStitches, 0);
  assert.equal(samplerReg.unmatchedStitches, 147);

  // Fail closed on invalid or truncated DST bytes.
  assert.throws(() => decodeDst(new Uint8Array(100)), /truncated/i);
  assert.throws(() => decodeDst(new TextEncoder().encode("<html>Not a DST file</html>".padEnd(600, " "))), /valid Tajima DST/i);

  // Byte-exact parity against native `ftx export embroidery` when the release binary is built.
  if (existsSync(NATIVE_FTX)) {
    const outDir = mkdtempSync(path.join(tmpdir(), "ftx-hussar-dst-"));
    const patPath = path.join(REPO_ROOT, "showcase/public/exhibits/hussar/dolman-pattern.json");
    const raw = execFileSync(NATIVE_FTX, ["export", "embroidery", patPath, "--out", outDir, "--json"], {
      encoding: "utf8",
    });
    const { ok, result } = JSON.parse(raw);
    assert.equal(ok, true);
    assert.equal(typeof result?.manifest, "string");
    const manifest = JSON.parse(readFileSync(result.manifest, "utf8"));
    const rFile = manifest.files["dst_dolman_front.R"].file;
    const lFile = manifest.files["dst_dolman_front.L"].file;
    assert.deepEqual(
      readFileSync(path.join(outDir, rFile)),
      readFileSync(path.join(REPO_ROOT, "showcase/public/exhibits/hussar/dolman-front-R.dst")),
      "dolman-front-R.dst must be byte-identical to native ftx export embroidery",
    );
    assert.deepEqual(
      readFileSync(path.join(outDir, lFile)),
      readFileSync(path.join(REPO_ROOT, "showcase/public/exhibits/hussar/dolman-front-L.dst")),
      "dolman-front-L.dst must be byte-identical to native ftx export embroidery",
    );
  }
});

test("Hussar embroidery trace page pauses at braid stop, registers 3D chest stitches, refuses unregistered sampler, serves DST/PDF, and fits phone", async () => {
  const page = await browser.newPage({
    viewport: { width: 1180, height: 840 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/wardrobe/hussar/embroidery/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".hs-flat-svg").waitFor({ timeout: 20000 });
    await page.locator('canvas[data-ready="true"]').waitFor({ timeout: 20000 });

    // Starts paused right at the braid stop (#296 1-based / index 295).
    assert.match(await page.locator(".hs-scrub-meta").innerText(), /Stop · lay the soutache braid/i);
    assert.equal(await page.locator('[data-metric="registered"]').innerText(), "2172");
    assert.equal(await page.locator('[data-metric="flat-only"]').innerText(), "76");
    assert.equal(await page.locator('[data-metric="active-status"]').innerText(), "braid-stop");

    // Advance into couching pass ("All stitches") and verify registered triangle + 3D chest coordinates.
    await page.getByRole("button", { name: "All stitches" }).click();
    assert.match(await page.locator('[data-metric="active-status"]').innerText(), /^Tri #\d+$/);
    assert.match(await page.locator(".hs-coord-grid").innerText(), /Settled 3D chest/i);

    // Clicking "Pause at braid stop" returns cleanly to the stop record.
    await page.getByRole("button", { name: "Pause at braid stop" }).click();
    assert.match(await page.locator(".hs-scrub-meta").innerText(), /Stop · lay the soutache braid/i);

    // Switch to left chest panel and confirm mirrored registration.
    await page.locator('button[data-source="dolman_front.L"]').click();
    await page.waitForFunction(() => document.querySelector('canvas[data-panel="dolman_front.L"]'));
    assert.equal(await page.locator('[data-metric="registered"]').innerText(), "2172");

    // Switch to unregistered standalone flat sampler: 0 registered 3D stitches, explicit banner, honest completion label.
    await page.locator('button[data-source="braid-sampler"]').click();
    await page.locator(".hs-unregistered-banner").waitFor({ timeout: 10000 });
    assert.equal(await page.locator('[data-metric="registered"]').innerText(), "0");
    assert.match(await page.locator(".hs-unregistered-banner").innerText(), /No panel-to-surface correspondence/i);
    await page.getByRole("button", { name: "All stitches" }).click();
    assert.match(await page.locator(".hs-scrub-meta").innerText(), /flat sampler loop finished/i);

    // Switch back to dolman_front.R, replay from record 0, verify automatic pause at braid stop (#296), and verify DST/PDF downloads.
    await page.locator('button[data-source="dolman_front.R"]').click();
    await page.waitForFunction(() => document.querySelector('[data-metric="registered"]')?.textContent === "2172");
    await page.getByRole("button", { name: "All stitches" }).click();
    await page.getByRole("button", { name: "Replay DST" }).click();
    await page.waitForFunction(
      () => document.querySelector(".hs-phase-badge")?.getAttribute("data-stop") === "true",
      { timeout: 10000 },
    );
    assert.match(await page.locator(".hs-scrub-meta").innerText(), /Record 296 \/ 2248/);
    const dstHref = await page.locator("a.hs-dl-primary").getAttribute("href");
    const dstRes = await page.request.get(new URL(dstHref, site).href);
    assert.equal(dstRes.status(), 200);
    assert.ok((await dstRes.body()).length > 7000);
    const pdfRes = await page.request.get(new URL("/patterns/hussar-blue.pdf", site).href);
    assert.equal(pdfRes.status(), 200);
    assert.equal((await pdfRes.body()).subarray(0, 5).toString(), "%PDF-");

    // Desktop & mobile overflow + single aria-current check.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

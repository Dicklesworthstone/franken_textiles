// Verify A4 tiled pattern assembly on screen (ftx-qiu) against the actual exported PDF bytes.
// Run with: FTX_STUDIO_URL=http://127.0.0.1:3000 node --test scripts/tile-floor.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";
import { parseTiledPdf } from "../public/labs/pdf-tiles.mjs";

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

test("PDF stream parser derives exact page counts, frame insets, 5 mm overlap, 100 mm scale bar, and join codes", async () => {
  const bolero = await parseTiledPdf(await readFile(new URL("../public/patterns/chart-zouave_bolero.pdf", import.meta.url)));
  assert.equal(bolero.totalPages, 7);
  assert.equal(bolero.printedTileCount, 6);
  assert.equal(bolero.blankTileCount, 3);
  assert.equal(bolero.pageWidthMm, 210);
  assert.equal(bolero.pageHeightMm, 297);
  assert.equal(bolero.frameWidthMm, 190);
  assert.equal(bolero.frameHeightMm, 277);
  assert.equal(bolero.overlapMm, 5);
  assert.equal(bolero.scaleBarMm, 100);
  assert.equal(bolero.cols, 3);
  assert.equal(bolero.rows, 3);
  assert.deepEqual(bolero.tiles.find(t => t.name === "A1")?.joins, ["A1|B1", "A1|A2"]);

  const trousers = await parseTiledPdf(await readFile(new URL("../public/patterns/chart-zouave_trousers.pdf", import.meta.url)));
  assert.equal(trousers.totalPages, 13);
  assert.equal(trousers.printedTileCount, 12);
  assert.equal(trousers.blankTileCount, 0);
  assert.equal(trousers.overlapMm, 5);

  const fez = await parseTiledPdf(await readFile(new URL("../public/patterns/chart-fez.pdf", import.meta.url)));
  assert.equal(fez.totalPages, 5);
  assert.equal(fez.printedTileCount, 4);
  assert.equal(fez.blankTileCount, 2);

  // Verify every known-good exported tiled PDF decodes cleanly.
  for (const [file, pages, printed, blank] of [
    ["../public/patterns/bolero.pdf", 8, 7, 2],
    ["../public/patterns/trousers.pdf", 13, 12, 0],
    ["../public/patterns/fez.pdf", 5, 4, 2],
    ["../public/patterns/hussar-blue.pdf", 86, 85, 5],
    ["../public/patterns/hussar-yellow.pdf", 90, 89, 1],
  ]) {
    const parsed = await parseTiledPdf(await readFile(new URL(file, import.meta.url)));
    assert.equal(parsed.totalPages, pages);
    assert.equal(parsed.printedTileCount, printed);
    assert.equal(parsed.blankTileCount, blank);
  }

  // Fail closed on non-PDF or truncated PDF bytes (never fall back to hardcoded defaults).
  await assert.rejects(() => parseTiledPdf(new TextEncoder().encode("not a pdf")), /not a valid PDF/i);
  await assert.rejects(() => parseTiledPdf(new TextEncoder().encode("%PDF-1.4\n/Type /Page\n")), /cover \+ tile pages/i);
});

test("PDF stream parser rejects corrupt and truncated page streams through one error path with zero unhandled rejections", async () => {
  const boleroBytes = new Uint8Array(
    await readFile(new URL("../public/patterns/chart-zouave_bolero.pdf", import.meta.url)),
  );
  const boleroRaw = Buffer.from(boleroBytes).toString("latin1");

  // Locate each stream payload in document order (page 1 = cover, page 2 = A1, page 3 = B1, ...).
  const streamSpans = [];
  for (const m of boleroRaw.matchAll(/<<\s*\/Length\s+(\d+)\s+\/Filter\s+\/FlateDecode\s*>>\nstream\n/g)) {
    const start = m.index + m[0].length;
    const len = Number(m[1]);
    streamSpans.push({ dictStart: m.index, start, len, end: start + len });
  }
  assert.equal(streamSpans.length, 7);

  const unhandled = [];
  const onUnhandled = reason => unhandled.push(reason);
  process.on("unhandledRejection", onUnhandled);
  try {
    // 1. Corrupt bytes inside page 3 (B1) deflate payload — must reject cleanly, never emit partial map.
    const corruptPage3 = boleroBytes.slice();
    const p3 = streamSpans[2];
    for (let i = 0; i < 12; i++) corruptPage3[p3.start + Math.min(i + 4, p3.len - 1)] ^= 0xff;
    await assert.rejects(() => parseTiledPdf(corruptPage3), /Damaged page 3 deflate stream/i);

    // 2. Corrupt zlib header on cover page (page 1).
    const corruptCover = boleroBytes.slice();
    corruptCover[streamSpans[0].start] ^= 0xff;
    corruptCover[streamSpans[0].start + 1] ^= 0xff;
    await assert.rejects(() => parseTiledPdf(corruptCover), /Damaged page 1 deflate stream/i);

    // 3. Corrupt Adler-32 checksum at the end of page 3 deflate payload.
    const corruptAdler = boleroBytes.slice();
    corruptAdler[p3.end - 1] ^= 0xff;
    await assert.rejects(() => parseTiledPdf(corruptAdler), /Damaged page 3 deflate stream/i);

    // 4. Truncated page 3 deflate payload WITH updated /Length (short valid-looking stream object, incomplete zlib).
    const truncatedLen = Math.max(8, Math.floor(p3.len / 2));
    const beforeDict = boleroRaw.slice(0, p3.dictStart);
    const updatedHeader = `<< /Length ${truncatedLen} /Filter /FlateDecode >>\nstream\n`;
    const truncatedPayload = boleroBytes.subarray(p3.start, p3.start + truncatedLen);
    const afterPayload = boleroBytes.subarray(p3.end);
    const truncatedStream = Buffer.concat([
      Buffer.from(beforeDict + updatedHeader, "latin1"),
      Buffer.from(truncatedPayload),
      Buffer.from(afterPayload),
    ]);
    await assert.rejects(() => parseTiledPdf(truncatedStream), /Damaged page 3 deflate stream/i);

    // 5. Truncated stream payload WITHOUT updating /Length (missing endstream at declared /Length).
    const mismatchLen = Buffer.concat([
      Buffer.from(boleroBytes.subarray(0, p3.start + truncatedLen)),
      Buffer.from(boleroBytes.subarray(p3.end)),
    ]);
    await assert.rejects(() => parseTiledPdf(mismatchLen), /missing endstream at declared \/Length/i);

    // 6. Truncated PDF file mid-stream (EOF before endstream) and mid-tail (missing %%EOF).
    await assert.rejects(
      () => parseTiledPdf(boleroBytes.subarray(0, p3.start + truncatedLen)),
      /Truncated PDF stream/i,
    );
    const eofIdx = boleroRaw.lastIndexOf("%%EOF");
    await assert.rejects(
      () => parseTiledPdf(boleroBytes.subarray(0, eofIdx)),
      /missing %%EOF/i,
    );

    const indirectLength = Buffer.from(
      boleroRaw.slice(0, p3.dictStart) +
        boleroRaw.slice(p3.dictStart).replace(`/Length ${p3.len} `, `/Length ${p3.len} 0 R `),
      "latin1",
    );
    await assert.rejects(() => parseTiledPdf(indirectLength), /missing a direct \/Length/i);

    // Allow any stray async writer rejections a tick to surface.
    await new Promise(resolve => setTimeout(resolve, 25));
    assert.deepEqual(unhandled, [], "corrupt/truncated stream rejection must leave zero unhandled rejections");
  } finally {
    process.off("unhandledRejection", onUnhandled);
  }
});

test("A4 tile assembly floor toggles alignment crosses, compares master & scale bar, serves exact PDF, and fits mobile", async () => {
  const page = await browser.newPage({
    viewport: { width: 1120, height: 820 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/studies/tiling/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".tf-svg").waitFor({ timeout: 15000 });

    assert.equal(await page.locator(".tf-tile").count(), 6);
    assert.equal(await page.locator(".tf-tile-blank").count(), 3);
    assert.equal(await page.locator('[data-crosses="true"]').count(), 6);
    assert.equal(await page.locator('[data-master-overlay="true"]').count(), 1);
    assert.equal(await page.locator('[data-scale-mm="100"]').count(), 1);

    const kpiText = await page.locator(".tf-kpis").innerText();
    assert.match(kpiText, /7 pages/);
    assert.match(kpiText, /5 mm/);
    assert.match(kpiText, /100 mm = 100 mm/);

    // Verify scale bar widths in SVG match identically (PDF 100 mm === Master 100 mm).
    const barWidths = await page.locator(".tf-scale-bars rect").evaluateAll(rects => rects.map(r => Number(r.getAttribute("width"))));
    assert.deepEqual(barWidths, [100, 100]);

    // Toggle alignment crosses off and back on.
    const crossCheck = page.getByRole("checkbox", { name: /alignment crosses/i });
    await crossCheck.uncheck();
    assert.equal(await page.locator('[data-crosses="true"]').count(), 0);
    await crossCheck.check();
    assert.equal(await page.locator('[data-crosses="true"]').count(), 6);

    // Toggle full master boundary off and back on.
    const masterCheck = page.getByRole("checkbox", { name: /full uncut master/i });
    await masterCheck.uncheck();
    assert.equal(await page.locator('[data-master-overlay="true"]').count(), 0);
    await masterCheck.check();
    assert.equal(await page.locator('[data-master-overlay="true"]').count(), 1);

    // Selecting an omitted blank tile and then hiding blank tiles cleanly falls back to a printed tile.
    await page.locator('.tf-tile-blank[data-tile="C2"]').click();
    assert.match(await page.locator(".tf-tile-card").innerText(), /Tile C2/);
    const blanksCheck = page.getByRole("checkbox", { name: /omitted blank tiles/i });
    await blanksCheck.uncheck();
    assert.equal(await page.locator(".tf-tile-blank").count(), 0);
    assert.match(await page.locator(".tf-tile-card").innerText(), /Tile A1/);
    await blanksCheck.check();
    assert.equal(await page.locator(".tf-tile-blank").count(), 3);

    // Inspect tile B1 and verify join codes derived from its PDF stream.
    await page.locator('.tf-tile[data-tile="B1"]').click();
    const cardText = await page.locator(".tf-tile-card").innerText();
    assert.match(cardText, /Tile B1/);
    assert.match(cardText, /PDF Page 3/i);
    assert.match(cardText, /A1\|B1/);
    assert.match(cardText, /B1\|C1/);

    // Verify downloadable PDF matches the exact served PDF file byte-for-byte.
    const pdfHref = await page.locator("a.tf-dl-primary").getAttribute("href");
    const dlRes = await page.request.get(new URL(pdfHref, site).href);
    const dlBytes = await dlRes.body();
    const expectedBytes = await readFile(new URL("../public/patterns/chart-zouave_bolero.pdf", import.meta.url));
    assert.deepEqual(dlBytes, expectedBytes);

    // Switch to trousers (13 pages / 12 tiles) and verify live re-parse.
    await page.getByRole("combobox", { name: /exported A4 pattern PDF/i }).selectOption("zouave_trousers");
    await page.waitForFunction(() => document.querySelectorAll(".tf-tile").length === 12);
    assert.match(await page.locator(".tf-kpis").innerText(), /13 pages/);

    // Desktop & mobile overflow + single aria-current checks.
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert.equal(await page.locator('header [aria-current="page"]').count(), 1);
    await page.setViewportSize({ width: 390, height: 844 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  } finally {
    await page.close();
  }
});

test("the drafting studio assembles the current draft’s exact A4 PDF and master", async () => {
  const page = await browser.newPage({
    viewport: { width: 390, height: 844 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  try {
    await page.goto(new URL("/sections/draft/", site).href, { waitUntil: "domcontentloaded" });
    const drafting = page.locator(".draft-lab");
    const floor = page.getByRole("region", { name: "A4 tiles for the current draft" });
    const fileBytes = async link => Uint8Array.from(await link.evaluate(async anchor => {
      const response = await fetch(anchor.href);
      if (!response.ok) throw Error(`Export unavailable (${response.status}).`);
      return Array.from(new Uint8Array(await response.arrayBuffer()));
    }));
    await drafting.getByRole("button", { name: "Fez", exact: true }).click();

    let firstPdf;
    for (const height of [104, 120]) {
      await drafting.getByRole("slider", { name: "Chart stature in centimetres" }).fill(String(height));
      const sourcePdf = drafting.getByRole("link", { name: "A4 pattern PDF" });
      const sourceMaster = drafting.getByRole("link", { name: "Master SVG" });
      await sourcePdf.waitFor();
      const expectedPdf = await fileBytes(sourcePdf);
      const expectedMaster = await fileBytes(sourceMaster);
      if (firstPdf) {
        assert.notDeepEqual(expectedPdf, firstPdf);
      } else {
        firstPdf = expectedPdf;
        await drafting.getByRole("button", { name: "Preview this draft’s A4 tiles" }).click();
      }

      await floor.locator(".tf-svg").waitFor();
      if (height === 104) {
        const controls = await floor.locator(".tf-stage-head").boundingBox();
        const header = await page.locator("header.ft-head").boundingBox();
        assert.ok(controls && header && controls.y >= header.y + header.height - 1 && controls.y < 200,
          "opening the preview should put its first row below the sticky header");
      }
      assert.match(await floor.locator(".tf-summary").innerText(), new RegExp(`Fez.*${height} cm`));
      assert.deepEqual(await fileBytes(floor.getByRole("link", { name: /download this exact A4 PDF/i })), expectedPdf);
      assert.deepEqual(await fileBytes(floor.getByRole("link", { name: /download uncut master SVG/i })), expectedMaster);
      const parsed = await parseTiledPdf(expectedPdf);
      assert.equal(await floor.locator(".tf-tile").count(), parsed.printedTileCount);
      assert.match(await floor.locator(".tf-kpis").innerText(), new RegExp(`${parsed.totalPages} pages`));
      const viewBox = new TextDecoder().decode(expectedMaster).match(/\bviewBox="([^"]+)"/);
      assert.ok(viewBox, "master must declare its millimetre extent");
      const [, , widthMm, heightMm] = viewBox[1].trim().split(/\s+/).map(Number);
      const boundary = await floor.locator(".tf-master-overlay rect").first().evaluate(rect =>
        [Number(rect.getAttribute("width")), Number(rect.getAttribute("height"))]);
      assert.deepEqual(boundary, [widthMm, heightMm]);
    }

    assert.equal(await page.locator("header").count(), 1);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await floor.getByRole("button", { name: "Tape flush" }).click();
    assert.match(await floor.locator(".tf-master-overlay").textContent(), /1:1 MATCH/);
    await drafting.getByRole("button", { name: "Hide A4 tile preview" }).click();
    assert.equal(await page.locator(".tf-svg").count(), 0);
  } finally {
    await page.close();
  }
});

test("A corrupt PDF retry clears an earlier successful preview without throwing a page error", async () => {
  const boleroBytes = new Uint8Array(
    await readFile(new URL("../public/patterns/chart-zouave_bolero.pdf", import.meta.url)),
  );
  const boleroRaw = Buffer.from(boleroBytes).toString("latin1");
  const streams = [...boleroRaw.matchAll(/<<\s*\/Length\s+(\d+)\s+\/Filter\s+\/FlateDecode\s*>>\nstream\n/g)];
  assert.equal(streams.length, 7);
  const p3Start = streams[2].index + streams[2][0].length;
  const p3Len = Number(streams[2][1]);
  const damaged = boleroBytes.slice();
  for (let i = 0; i < 12; i++) damaged[p3Start + Math.min(i + 4, p3Len - 1)] ^= 0xff;

  const page = await browser.newPage({
    viewport: { width: 1120, height: 820 },
    reducedMotion: "reduce",
    userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
  });
  const pageErrors = [];
  page.on("pageerror", err => pageErrors.push(err.message));
  try {
    await page.goto(new URL("/studies/tiling/", site).href, { waitUntil: "domcontentloaded" });
    await page.locator(".tf-svg").waitFor();
    const pattern = page.getByRole("combobox", { name: "Choose exported A4 pattern PDF" });
    await page.route("**/patterns/chart-zouave_trousers.pdf", route => route.fulfill({ status: 503 }));
    await page.route("**/patterns/chart-zouave_bolero.pdf*", route =>
      route.fulfill({
        status: 200,
        contentType: "application/pdf",
        body: Buffer.from(damaged),
      }),
    );
    const status = page.getByRole("status");
    await pattern.selectOption("zouave_trousers");
    await status.filter({ hasText: "Could not fetch exported PDF (503)." }).waitFor();
    await pattern.selectOption("zouave_bolero");
    await status.filter({ hasText: /Damaged page 3 deflate stream/i }).waitFor();
    assert.equal(await page.locator(".tf-svg").count(), 0, "must not render a partial cover map after page 3 fails");
    assert.equal(await page.locator(".tf-tile").count(), 0);
    assert.match(await page.locator(".tf-stage-head").innerText(), /assembly preview unavailable/i);
    assert.deepEqual(pageErrors, [], "damaged stream must not produce unhandled browser rejections");
  } finally {
    await page.close();
  }
});

// Actual offline generated page -> corrected input. No model/detector doubles.
// Prepare with the capture_editor_fixture_ real-binary test and FTX_CAPTURE_EDITOR_DIR.
// NODE_PATH=... node tools/check_capture_review.cjs CASE_DIR NEW_OUTPUT_DIR
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const {pathToFileURL} = require("node:url");
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
if (process.argv.length !== 4) throw Error("Usage: node tools/check_capture_review.cjs CASE_DIR NEW_OUTPUT_DIR");
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]);
const reviewDir = path.join(root, "edit-review");
const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const report = read("edit-review/review.json"), replay = read("edit-review/capture.json"), good = read("good.json");
const hash = b => crypto.createHash("sha256").update(b).digest("hex");
const frame = 0, point = 3;
assert.equal(report.frames.length, 2);
assert.equal(replay.fit.frames[frame].keypoints[point].partition, "held_out");
assert.equal(read("bad-fit.json").solution, null);
assert.equal(read("expected-fit.json").status, "ACCEPTED");
fs.mkdirSync(out, {mode: 0o700});
function verifyFiles() {
  for (const item of [report.original_input, report.replay_input, report.viewer, report.editor,
    ...report.frames.flatMap(f => [f.source_image, f.keypoint_svg])]) {
    const b = fs.readFileSync(path.join(reviewDir, item.file));
    assert.equal(b.length, item.bytes); assert.equal(hash(b), item.sha256);
  }
}
async function download(page, name, expected) {
  const event = page.waitForEvent("download");
  await page.locator("#correctedInput").click();
  const file = await event;
  assert.equal(file.suggestedFilename(), "capture-edited.json");
  const destination = path.join(out, name);
  assert(!fs.existsSync(destination));
  await file.saveAs(destination);
  assert.deepEqual(JSON.parse(fs.readFileSync(destination)), expected);
  return destination;
}
async function hidden(page) {
  assert(await page.locator("#correctedInput").isHidden());
  assert.equal(await page.locator("#correctedInput").getAttribute("href"), null);
}
async function displayed(page, i, j) {
  const g = page.locator("section").nth(i).locator('g[data-point="' + j + '"]');
  return [Number(await g.locator("circle").getAttribute("cx")), Number(await g.locator("circle").getAttribute("cy"))];
}
(async () => {
  verifyFiles();
  const browser = await chromium.launch({headless: true});
  const errors = [], requests = [], userAgent = "OpenAI File Downloader, XaiImageApiFetch/1.0";
  function observe(context) {
    context.on("request", r => requests.push({url: r.url(), method: r.method()}));
    context.on("page", page => {
      page.on("pageerror", e => errors.push(String(e)));
      page.on("console", m => { if (m.type() === "error") errors.push(m.text()); });
    });
  }
  try {
    const context = await browser.newContext({viewport: {width: 1200, height: 1000}, userAgent});
    observe(context);
    // Observe the real URL API; do not replace any generated data or suppress revocation.
    await context.addInitScript(() => {
      globalThis.__revoked = [];
      const revoke = URL.revokeObjectURL.bind(URL);
      URL.revokeObjectURL = url => { globalThis.__revoked.push(url); return revoke(url); };
    });
    const page = await context.newPage();
    await page.goto(pathToFileURL(path.join(reviewDir, "index.html")).href);
    await page.waitForFunction(() => !document.getElementById("captureEditor").hidden);
    assert.match(await page.locator("#editStatus").innerText(), /^Select a point/);
    assert.equal(await page.evaluate(() => globalThis.captureInjection), undefined);
    assert.deepEqual(await page.locator("#captureInput").evaluate(n => JSON.parse(n.textContent)), replay);
    assert.equal(await page.locator("script").count(), 2);
    const policy = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content");
    assert.equal(policy.split(";").map(s => s.trim()).find(s => s.startsWith("script-src")), "script-src 'self'");
    assert.equal(await page.locator("section").count(), report.frames.length);
    for (let i = 0; i < report.frames.length; ++i) {
      const section = page.locator("section").nth(i), f = report.frames[i];
      const image = await section.locator("image").getAttribute("href");
      assert.equal(image, f.source_image.file);
      assert.deepEqual(await page.evaluate(url => new Promise((resolve, reject) => {
        const image = new Image(); image.onload = () => resolve([image.naturalWidth, image.naturalHeight]);
        image.onerror = () => reject(Error("Source image did not load")); image.src = url;
      }), image), f.image_size_px);
      assert.equal(await section.locator("tbody tr").count(), f.keypoints.length);
      for (let j = 0; j < f.keypoints.length; ++j) {
        assert.deepEqual(await displayed(page, i, j), f.keypoints[j].pixel_px);
        assert.equal(await section.locator('g[data-point="' + j + '"]').getAttribute("data-partition"),
          f.keypoints[j].partition === "fit" ? "FIT" : "CHECK");
      }
    }
    await hidden(page);
    for (const [label, width, zoom] of [["desktop", 1200, 1], ["mobile", 600, 1], ["zoom", 1200, 1.4]]) {
      await page.setViewportSize({width, height: 1000});
      await page.evaluate(z => document.body.style.zoom = z, zoom);
      await page.locator("#resetPoints").click();
      await page.locator('.edit-point[data-frame="0"][data-point="3"]').click();
      const svg = page.locator("section").nth(0).locator("svg");
      await svg.scrollIntoViewIfNeeded();
      const location = await svg.evaluate(n => {
        n.addEventListener("click", e => {
          const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(n.getScreenCTM().inverse());
          globalThis.__clickedPixel = [p.x, p.y];
        }, {once: true, capture: true});
        const p = new DOMPoint(360.75, 300.25).matrixTransform(n.getScreenCTM());
        return [p.x, p.y];
      });
      await page.mouse.click(...location);
      const pixel = await page.evaluate(() => globalThis.__clickedPixel);
      assert.deepEqual(await displayed(page, frame, point), pixel);
      const expected = structuredClone(replay);
      expected.fit.frames[frame].keypoints[point].pixel_px = pixel;
      await download(page, "click-" + label + ".json", expected);
      const previous = await page.locator("#correctedInput").getAttribute("href");
      await page.locator("#editX").fill("");
      await hidden(page);
      assert(await page.evaluate(url => globalThis.__revoked.includes(url), previous));
    }
    // Invalid or pending edits cannot advertise the last valid input or clamp to its boundary.
    const prior = await displayed(page, frame, point);
    for (const [x, y] of [["-1", "200"], ["1280", "200"], ["200", "960"], ["", "200"], ["1e309", "200"]]) {
      await page.locator("#editX").fill(x); await page.locator("#editY").fill(y);
      await page.locator("#applyPoint").click(); await hidden(page);
      assert.deepEqual(await displayed(page, frame, point), prior);
    }
    const target = good.fit.frames[frame].keypoints[point].pixel_px;
    await page.locator("#editX").fill(String(target[0]));
    await page.locator("#editY").fill(String(target[1]));
    await page.locator("#applyPoint").click();
    assert.deepEqual(await displayed(page, frame, point), target);
    const corrected = structuredClone(replay);
    corrected.fit.frames[frame].keypoints[point].pixel_px = target;
    const final = await download(page, "capture-edited.json", corrected);
    assert.equal(corrected.reviewed_sources_sha256, report.capture_input.sources_sha256);
    await page.screenshot({path: path.join(out, "corrected.png"), fullPage: true});
    const finalUrl = await page.locator("#correctedInput").getAttribute("href");
    await page.locator("#resetPoints").click(); await hidden(page);
    assert(await page.evaluate(url => globalThis.__revoked.includes(url), finalUrl));
    for (let i = 0; i < report.frames.length; ++i) {
      for (let j = 0; j < report.frames[i].keypoints.length; ++j) {
        assert.deepEqual(await displayed(page, i, j), report.frames[i].keypoints[j].pixel_px);
      }
    }
    assert(await page.locator("#applyPoint").isDisabled());
    const staticContext = await browser.newContext({javaScriptEnabled: false, userAgent});
    observe(staticContext);
    const staticPage = await staticContext.newPage();
    await staticPage.goto(pathToFileURL(path.join(reviewDir, "index.html")).href);
    assert(await staticPage.locator("#captureEditor").isHidden());
    assert.equal(await staticPage.locator("svg circle").count(), 20);
    assert.equal(await staticPage.locator("tbody tr").count(), 20);
    assert(await staticPage.locator("noscript").isVisible());
    const unsafePage = await context.newPage();
    await unsafePage.goto(pathToFileURL(path.join(root, "numeric-unsafe/review/index.html")).href);
    await unsafePage.waitForFunction(() => !document.getElementById("captureEditor").hidden);
    assert.match(await unsafePage.locator("#editStatus").innerText(), /Editor unavailable:.*safe browser numeric precision/);
    await hidden(unsafePage);
    assert(await unsafePage.locator("#applyPoint").isDisabled());
    assert.equal(await unsafePage.locator("svg circle").count(), 20);
    assert((await unsafePage.locator("#captureInput").textContent()).includes("9007199254740993"));
    assert.deepEqual(errors, []);
    const allowed = [reviewDir, path.join(root, "numeric-unsafe/review")].map(dir => pathToFileURL(dir + path.sep).href);
    assert(requests.every(r => r.method === "GET" && allowed.some(prefix => r.url.startsWith(prefix))));
    verifyFiles();
    console.log(JSON.stringify({status: "PASS", frames: 2, points: 20, clickLayouts: 3,
      invalidEdits: 5, correctedFile: final, sha256: hash(fs.readFileSync(final)), errors, requests,
      boundary: "Synthetic actual page and data only; fit acceptance is a separate ordinary CLI run"}, null, 2));
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });

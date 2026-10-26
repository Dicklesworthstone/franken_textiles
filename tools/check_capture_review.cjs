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
function verifyFiles(directory = reviewDir, records = report) {
  for (const item of [records.original_input, records.replay_input, records.viewer, records.editor,
    ...records.frames.flatMap(f => [f.source_image, f.keypoint_svg])]) {
    const b = fs.readFileSync(path.join(directory, item.file));
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
    // Start with the actual native zero-observation review. Coordinates, roles and sigmas below
    // come from the independent synthetic good input, never from the candidate editor/fitter.
    const emptyDir = path.join(root, "empty-review"), empty = read("empty-review/capture.json");
    const emptyReport = read("empty-review/review.json");
    assert(empty.fit.frames.every(f => f.keypoints.length === 0));
    assert.equal(emptyReport.capture_input.sources_sha256, report.capture_input.sources_sha256);
    const initialHashes = new Map(["empty.json", "good.json", "input.json", "empty-review/capture.json",
      "empty-review/review.json", "empty-review/index.html", "empty-review/review.js",
      ...emptyReport.frames.map(f => "empty-review/" + f.source_image.file)]
      .map(file => [file, hash(fs.readFileSync(path.join(root, file)))]));
    const placing = await context.newPage();
    await placing.goto(pathToFileURL(path.join(emptyDir, "index.html")).href);
    await placing.waitForFunction(() => !document.getElementById("newPointFields").disabled);
    assert.deepEqual(await placing.locator("#captureInput").evaluate(n => JSON.parse(n.textContent)), empty);
    assert.equal(await placing.locator("tbody tr, svg circle").count(), 0);
    await hidden(placing);
    // Every declaration is explicit; an incomplete/invalid one creates no observation.
    for (const [id, value] of [[null, null], ["newFrame", "0"], ["newTarget", "p0"], ["newPartition", "fit"]]) {
      if (id) await placing.locator("#" + id).selectOption(value);
      await placing.locator("#preparePoint").click();
      assert(await placing.locator("#applyPoint").isDisabled());
      await hidden(placing);
    }
    for (const sigma of ["0", "-1", "0.0000001", "1000001", "1e309"]) {
      await placing.locator("#newSigma").fill(sigma);
      await placing.locator("#preparePoint").click(); await hidden(placing);
      assert(await placing.locator("#applyPoint").isDisabled());
    }
    async function declare(i, p) {
      await placing.locator("#newFrame").selectOption(String(i));
      await placing.locator("#newTarget").selectOption(p.name);
      await placing.locator("#newPartition").selectOption(p.partition);
      await placing.locator("#newSigma").fill(String(p.sigma_px));
      await placing.locator("#preparePoint").click();
      assert(await placing.locator("#applyPoint").isEnabled());
      await hidden(placing);
    }
    const p0 = good.fit.frames[0].keypoints[0];
    await declare(0, p0);
    // Image placement uses the same real screen transform as correction, including scrolling.
    const svg = placing.locator("section").nth(0).locator("svg");
    await svg.scrollIntoViewIfNeeded();
    const screen = await svg.evaluate((n, xy) => {
      n.addEventListener("click", e => {
        const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(n.getScreenCTM().inverse());
        globalThis.__placedPixel = [p.x, p.y];
      }, {once: true, capture: true});
      const p = new DOMPoint(...xy).matrixTransform(n.getScreenCTM()); return [p.x, p.y];
    }, p0.pixel_px);
    await placing.mouse.click(...screen);
    const clickPixel = await placing.evaluate(() => globalThis.__placedPixel);
    assert.deepEqual(await displayed(placing, 0, 0), clickPixel);
    const partial = structuredClone(empty);
    partial.fit.frames[0].keypoints.push({...p0, pixel_px: clickPixel});
    await download(placing, "partial-placement.json", partial);
    // Changing a declaration revokes prior output. Cancelling does not append the pending point.
    const partialUrl = await placing.locator("#correctedInput").getAttribute("href");
    await declare(0, good.fit.frames[0].keypoints[1]);
    assert(await placing.evaluate(url => globalThis.__revoked.includes(url), partialUrl));
    await placing.locator("#cancelPoint").click(); await hidden(placing);
    assert.equal(await placing.locator("tbody tr").count(), 1);
    // The chooser omits already used names; the click guard also refuses a forged DOM option.
    assert.equal(await placing.locator('#newTarget option[value="p0"]').count(), 0);
    for (const name of [p0.name, "undeclared-target"]) {
      await placing.evaluate(({name, sigma}) => {
        const target = document.getElementById("newTarget"); target.add(new Option(name, name)); target.value = name;
        document.getElementById("newPartition").value = "fit";
        document.getElementById("newSigma").value = sigma;
      }, {name, sigma: String(p0.sigma_px)});
      await placing.locator("#preparePoint").click(); await hidden(placing);
      assert(await placing.locator("#applyPoint").isDisabled());
      assert.equal(await placing.locator("tbody tr").count(), 1);
    }
    await placing.locator("#resetPoints").click(); await hidden(placing);
    assert.equal(await placing.locator("tbody tr, svg circle").count(), 0);
    // A new point follows the same refusal bounds as a correction; no hidden zero gets inserted.
    await declare(0, p0);
    for (const [x, y] of [["-1", "200"], ["1280", "200"], ["200", "960"], ["", "200"]]) {
      await placing.locator("#editX").fill(x); await placing.locator("#editY").fill(y);
      await placing.locator("#applyPoint").click(); await hidden(placing);
      assert.equal(await placing.locator("tbody tr, svg circle").count(), 0);
    }
    await placing.locator("#resetPoints").click();
    for (let i = 0; i < good.fit.frames.length; ++i) {
      for (let j = 0; j < good.fit.frames[i].keypoints.length; ++j) {
        const p = good.fit.frames[i].keypoints[j];
        await declare(i, p);
        await placing.locator("#editX").fill(String(p.pixel_px[0]));
        await placing.locator("#editY").fill(String(p.pixel_px[1]));
        await placing.locator("#applyPoint").click();
        assert.deepEqual(await displayed(placing, i, j), p.pixel_px);
        const row = placing.locator("section").nth(i).locator("tbody tr").nth(j);
        assert.equal(await row.locator("td").nth(1).innerText(), p.partition === "fit" ? "FIT" : "CHECK");
        assert.equal(Number(await row.locator("td").nth(3).innerText()), p.sigma_px);
      }
    }
    const annotated = structuredClone(empty);
    annotated.fit.frames.forEach((f, i) => f.keypoints = structuredClone(good.fit.frames[i].keypoints));
    const annotatedFile = await download(placing, "capture-annotated.json", annotated);
    assert.equal(await placing.evaluate(() => globalThis.captureInjection), undefined);
    assert.equal(await placing.locator("svg circle").count(), 20);
    await placing.screenshot({path: path.join(out, "annotated.png"), fullPage: true});
    await placing.locator("#resetPoints").click(); await hidden(placing);
    assert.equal(await placing.locator("tbody tr, svg circle").count(), 0);
    for (const [file, expectedHash] of initialHashes) {
      assert.equal(hash(fs.readFileSync(path.join(root, file))), expectedHash);
    }
    // The pre-fit page shows exactly the sampled constraints the ordinary mask reader supplied.
    const maskDir = path.join(root, "mask-review"), maskReport = read("mask-review/review.json");
    const maskReplay = read("mask-review/capture.json"), maskedPage = await context.newPage();
    verifyFiles(maskDir, maskReport);
    const maskSources = ["masks/manifest.json", "masks/mask-0.png", "masks/mask-1.png"];
    const maskHashes = maskSources.map(file => hash(fs.readFileSync(path.join(root, file))));
    assert.equal(maskHashes[0], maskReport.capture_input.silhouette_input.manifest_sha256);
    assert.equal(read("masked-bad-fit.json").solution, null);
    assert.equal(read("expected-masked-fit.json").status, "ACCEPTED");
    assert.equal(maskReplay.reviewed_sources_sha256, maskReport.capture_input.sources_sha256);
    assert.equal(maskReplay.reviewed_sources_sha256, read("expected-masked-fit.json").capture_input.sources_sha256);
    assert.equal(typeof maskReplay.silhouettes, "string");
    await maskedPage.goto(pathToFileURL(path.join(maskDir, "index.html")).href);
    await maskedPage.waitForFunction(() => !document.getElementById("captureEditor").hidden);
    for (let i = 0; i < maskReport.frames.length; ++i) {
      const section = maskedPage.locator("section").nth(i), f = maskReport.frames[i];
      const slices = section.locator("g[data-silhouette]");
      assert.equal(await slices.count(), 5);
      assert.equal(await section.locator(".silhouette-review li").count(), 5);
      assert((await section.locator(".silhouette-review").innerText()).includes(f.silhouette.evidence));
      for (let j = 0; j < f.silhouette.lines.length; ++j) {
        const line = f.silhouette.lines[j], group = slices.nth(j), segments = group.locator("line");
        assert.equal(await group.getAttribute("data-silhouette"), line.partition);
        assert.equal(await group.getAttribute("pointer-events"), "none");
        assert.equal(await segments.count(), line.intervals_px.length + 1);
        const along = line.axis === "horizontal" ? 0 : 1;
        const intervals = [[0, f.image_size_px[along] - 1], ...line.intervals_px];
        for (let k = 0; k < intervals.length; ++k) {
          const [lo, hi] = intervals[k];
          const expected = along === 0 ? [lo, line.at_px, hi, line.at_px] : [line.at_px, lo, line.at_px, hi];
          assert.deepEqual(await segments.nth(k).evaluate(n => ["x1", "y1", "x2", "y2"].map(a => Number(n.getAttribute(a)))), expected);
        }
        const text = await section.locator(".silhouette-review li").nth(j).innerText();
        assert(text.includes(line.partition === "fit" ? "FIT" : "CHECK"));
        assert(text.includes("sigma " + line.sigma_px + " px"));
        if (!line.intervals_px.length) assert(text.includes("empty (background)"));
      }
    }
    const observedMarkup = await maskedPage.locator("g[data-silhouette]").evaluateAll(ns => ns.map(n => n.outerHTML));
    await maskedPage.locator('.edit-point[data-frame="0"][data-point="3"]').click();
    await maskedPage.locator("#editX").fill(String(target[0]));
    await maskedPage.locator("#editY").fill(String(target[1]));
    await maskedPage.locator("#applyPoint").click();
    const maskedExpected = structuredClone(maskReplay);
    maskedExpected.fit.frames[0].keypoints[3].pixel_px = target;
    const maskedFile = await download(maskedPage, "capture-masked.json", maskedExpected);
    assert.equal(await maskedPage.locator("g[data-silhouette]").count(), 10);
    assert.deepEqual(await maskedPage.locator("g[data-silhouette]").evaluateAll(ns => ns.map(n => n.outerHTML)), observedMarkup);
    await maskedPage.screenshot({path: path.join(out, "sampled-silhouettes.png"), fullPage: true});
    await maskedPage.locator("#resetPoints").click(); await hidden(maskedPage);
    assert.deepEqual(await maskedPage.locator("g[data-silhouette]").evaluateAll(ns => ns.map(n => n.outerHTML)), observedMarkup);
    const staticContext = await browser.newContext({javaScriptEnabled: false, userAgent});
    observe(staticContext);
    const staticPage = await staticContext.newPage();
    await staticPage.goto(pathToFileURL(path.join(reviewDir, "index.html")).href);
    assert(await staticPage.locator("#captureEditor").isHidden());
    assert.equal(await staticPage.locator("svg circle").count(), 20);
    assert.equal(await staticPage.locator("tbody tr").count(), 20);
    assert(await staticPage.locator("noscript").isVisible());
    const staticMasks = await staticContext.newPage();
    await staticMasks.goto(pathToFileURL(path.join(maskDir, "index.html")).href);
    assert.equal(await staticMasks.locator("g[data-silhouette]").count(), 10);
    assert(await staticMasks.locator("#captureEditor").isHidden());
    const unsafePage = await context.newPage();
    await unsafePage.goto(pathToFileURL(path.join(root, "numeric-unsafe/review/index.html")).href);
    await unsafePage.waitForFunction(() => !document.getElementById("captureEditor").hidden);
    assert.match(await unsafePage.locator("#editStatus").innerText(), /Editor unavailable:.*safe browser numeric precision/);
    await hidden(unsafePage);
    assert(await unsafePage.locator("#applyPoint").isDisabled());
    assert(await unsafePage.locator("#preparePoint").isDisabled());
    assert.equal(await unsafePage.locator("#newPointFields").evaluate(n => n.disabled), true);
    assert.equal(await unsafePage.locator("svg circle").count(), 20);
    assert((await unsafePage.locator("#captureInput").textContent()).includes("9007199254740993"));
    assert.deepEqual(errors, []);
    verifyFiles(maskDir, maskReport);
    assert.deepEqual(maskSources.map(file => hash(fs.readFileSync(path.join(root, file)))), maskHashes);
    const allowed = [reviewDir, emptyDir, maskDir, path.join(root, "numeric-unsafe/review")].map(dir => pathToFileURL(dir + path.sep).href);
    assert(requests.every(r => r.method === "GET" && allowed.some(prefix => r.url.startsWith(prefix))));
    verifyFiles();
    console.log(JSON.stringify({status: "PASS", frames: 2, points: 20, clickLayouts: 3,
      invalidEdits: 5, correctedFile: final, sha256: hash(fs.readFileSync(final)),
      initiallyPlaced: 20, annotatedFile, annotatedSha256: hash(fs.readFileSync(annotatedFile)), errors, requests,
      observedSlices: 10, maskedFile, maskedSha256: hash(fs.readFileSync(maskedFile)),
      boundary: "Synthetic actual page and data only; fit acceptance is a separate ordinary CLI run"}, null, 2));
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });

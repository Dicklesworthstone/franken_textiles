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
    ...records.frames.flatMap(f => [f.source_image, f.keypoint_svg, f.sheet_svg])]) {
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
async function sheetReview(page, records = report) {
  let count = 0;
  for (let i = 0; i < records.frames.length; ++i) {
    const section = page.locator("section").nth(i), sheet = section.locator(".sheet-review");
    const cameraRecord = records.capture_input.frames[i], pose = cameraRecord.sheet_pose;
    await sheet.locator("summary").click();
    assert(await sheet.locator("svg").isVisible());
    const text = await sheet.innerText();
    for (const identity of [pose.reference_id, cameraRecord.lens_id,
      cameraRecord.calibration_sha256, cameraRecord.reference_input_sha256]) assert(text.includes(identity));
    assert.equal(await sheet.locator("svg image").getAttribute("href"), records.frames[i].source_image.file);
    assert.equal(await sheet.locator("svg g[data-sheet-point]").count(), pose.input.points.length);
    assert.equal(await sheet.locator("ol li").count(), pose.input.points.length);
    // Independent pinhole projection of this synthetic, zero-distortion camera. This checks
    // display coordinates, not a new numerical calibration oracle or a real camera claim.
    assert.deepEqual(pose.camera.intrinsics.distortion.radial, [0, 0, 0]);
    assert.deepEqual(pose.camera.intrinsics.distortion.tangential, [0, 0]);
    for (let j = 0; j < pose.input.points.length; ++j) {
      const p = pose.input.points[j], marker = sheet.locator('svg g[data-sheet-point="' + j + '"]');
      const role = p.held_out ? "CHECK" : "FIT";
      assert.equal(await marker.getAttribute("data-partition"), role);
      assert.deepEqual(await marker.locator("circle").evaluate(n => [Number(n.getAttribute("cx")), Number(n.getAttribute("cy"))]), p.pixel_px);
      const delta = [p.sheet_xz_m[0], 0, p.sheet_xz_m[1]].map((x, k) => x - pose.camera.center_m[k]);
      const q = pose.camera.world_to_camera.rows.map(r => r.reduce((sum, x, k) => sum + x * delta[k], 0));
      const expected = [0, 1].map(k => pose.camera.intrinsics.principal_px[k] + pose.camera.intrinsics.focal_px[k] * q[k] / q[2]);
      const projected = await marker.locator("path.projected").evaluate(n => [Number(n.dataset.x), Number(n.dataset.y)]);
      projected.forEach((x, k) => assert(Math.abs(x - expected[k]) < 1e-9));
      const row = sheet.locator('li[data-sheet-point="' + j + '"]');
      assert.equal(await row.getAttribute("data-partition"), role);
      assert.equal(Number(await row.getAttribute("data-error-px")), pose.reprojection[j].error_px);
      ++count;
    }
  }
  return count;
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
    assert.equal(await page.locator("#removePoint").count(), 1, "selected observations can be removed without resetting all work");
    assert(await page.locator("#removePoint").isDisabled());
    assert.match(await page.locator("#editStatus").innerText(), /^Select a point/);
    assert.equal(await page.evaluate(() => globalThis.captureInjection), undefined);
    assert.deepEqual(await page.locator("#captureInput").evaluate(n => JSON.parse(n.textContent)), replay);
    assert.equal(await page.locator("script").count(), 2);
    const policy = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content");
    assert.equal(policy.split(";").map(s => s.trim()).find(s => s.startsWith("script-src")), "script-src 'self'");
    assert.equal(await page.locator("section").count(), report.frames.length);
    const sheetPoints = await sheetReview(page);
    assert.equal(sheetPoints, 18);
    const sheetBefore = await page.locator(".sheet-review").evaluateAll(nodes => nodes.map(n => n.innerHTML));
    await page.locator('.edit-point[data-frame="0"][data-point="3"]').click();
    const bodyBeforeSheetClick = await displayed(page, 0, 3);
    await page.locator("section").nth(0).locator(".sheet-review svg").click();
    await hidden(page);
    assert.deepEqual(await displayed(page, 0, 3), bodyBeforeSheetClick);
    await page.locator("#resetPoints").click();
    for (let i = 0; i < report.frames.length; ++i) {
      const section = page.locator("section").nth(i), f = report.frames[i];
      const image = await section.locator(":scope > svg image").getAttribute("href");
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
      const svg = page.locator("section").nth(0).locator(":scope > svg");
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
    // Removing an observation must preserve every remaining correspondence, including the
    // row/marker indices used by later corrections. Reset restores names and roles as well.
    async function pointDisplays(expected) {
      for (let i = 0; i < expected.fit.frames.length; ++i) {
        const section = page.locator("section").nth(i), points = expected.fit.frames[i].keypoints;
        assert.equal(await section.locator("tbody tr").count(), points.length);
        assert.equal(await section.locator("g[data-point]").count(), points.length);
        for (const [j, p] of points.entries()) {
          const cells = section.locator("tbody tr").nth(j).locator("td");
          const role = p.partition === "fit" ? "FIT" : "CHECK";
          assert.equal((await cells.nth(0).innerText()).trim(), "Select " + p.name);
          assert.equal(await cells.nth(1).innerText(), role);
          assert.equal(Number(await cells.nth(3).innerText()), p.sigma_px);
          assert.deepEqual(JSON.parse(await cells.nth(4).innerText()), expected.model.keypoints[p.name]);
          assert.equal(await cells.nth(0).locator("button").getAttribute("data-point"), String(j));
          assert.equal(await section.locator('g[data-point="' + j + '"]').getAttribute("data-partition"), role);
          assert.equal(await section.locator('g[data-point="' + j + '"] text').textContent(), p.name + " (" + role + ")");
          assert.deepEqual(await displayed(page, i, j), p.pixel_px);
        }
      }
    }
    const removed = structuredClone(corrected), removedPoint = removed.fit.frames[0].keypoints.splice(4, 1)[0];
    await page.locator('.edit-point[data-frame="0"][data-point="4"]').click();
    const beforeRemovalUrl = await page.locator("#correctedInput").getAttribute("href");
    await page.locator("#removePoint").click();
    assert(await page.evaluate(url => globalThis.__revoked.includes(url), beforeRemovalUrl));
    assert(await page.locator("#removePoint").isDisabled());
    assert(await page.locator("#applyPoint").isDisabled());
    await pointDisplays(removed);
    const removedFile = await download(page, "capture-point-removed.json", removed);
    // This selection now refers to the NEXT observation, with its original role and sigma.
    await page.locator('.edit-point[data-frame="0"][data-point="4"]').click();
    const shiftedPixel = [removed.fit.frames[0].keypoints[4].pixel_px[0] + .125, removed.fit.frames[0].keypoints[4].pixel_px[1]];
    await page.locator("#editX").fill(String(shiftedPixel[0]));
    await page.locator("#removePoint").click(); await hidden(page);
    await pointDisplays(removed);
    await page.locator("#applyPoint").click();
    const shifted = structuredClone(removed); shifted.fit.frames[0].keypoints[4].pixel_px = shiftedPixel;
    await pointDisplays(shifted);
    await download(page, "capture-shifted-point.json", shifted);
    // Removed targets become placeable again, with an explicit role and uncertainty.
    await page.locator("#newFrame").selectOption("0");
    await page.locator("#newTarget").selectOption(removedPoint.name);
    await page.locator("#newPartition").selectOption(removedPoint.partition);
    await page.locator("#newSigma").fill(String(removedPoint.sigma_px));
    await page.locator("#preparePoint").click();
    assert(await page.locator("#removePoint").isDisabled());
    await page.locator("#editX").fill(String(removedPoint.pixel_px[0]));
    await page.locator("#editY").fill(String(removedPoint.pixel_px[1]));
    await page.locator("#applyPoint").click();
    shifted.fit.frames[0].keypoints.push(removedPoint);
    await pointDisplays(shifted);
    await download(page, "capture-point-readded.json", shifted);
    await page.locator("#resetPoints").click(); await hidden(page);
    await pointDisplays(replay);
    // A same-pixel replacement may change only the explicit role/sigma. It must still
    // count as an edit and download, even when it occupies the same original list slot.
    const replaced = structuredClone(replay), last = replaced.fit.frames[1].keypoints.at(-1);
    await page.locator('.edit-point[data-frame="1"][data-point="9"]').click();
    await page.locator("#removePoint").click();
    await page.locator("#newFrame").selectOption("1");
    await page.locator("#newTarget").selectOption(last.name);
    last.partition = last.partition === "fit" ? "held_out" : "fit";
    last.sigma_px += 1;
    await page.locator("#newPartition").selectOption(last.partition);
    await page.locator("#newSigma").fill(String(last.sigma_px));
    await page.locator("#preparePoint").click();
    await page.locator("#editX").fill(String(last.pixel_px[0]));
    await page.locator("#editY").fill(String(last.pixel_px[1]));
    await page.locator("#applyPoint").click();
    await pointDisplays(replaced);
    await download(page, "capture-replaced-metadata.json", replaced);
    await page.locator("#resetPoints").click(); await hidden(page);
    await pointDisplays(replay);
    // Removing CHECKs remains an explicit edit: the ordinary fitter reports a no-held-out
    // advisory. Removing all observations below supplies the separate native refusal control.
    const noCheck = structuredClone(replay);
    for (let i = 0; i < noCheck.fit.frames.length; ++i) {
      const points = noCheck.fit.frames[i].keypoints;
      for (let j = points.length - 1; j >= 0; --j) if (points[j].partition === "held_out") {
        await page.locator('.edit-point[data-frame="' + i + '"][data-point="' + j + '"]').click();
        await page.locator("#removePoint").click(); points.splice(j, 1);
      }
    }
    await pointDisplays(noCheck);
    const noCheckFile = await download(page, "capture-no-check.json", noCheck);
    for (let i = 0; i < noCheck.fit.frames.length; ++i) {
      const points = noCheck.fit.frames[i].keypoints;
      while (points.length) {
        await page.locator('.edit-point[data-frame="' + i + '"][data-point="' + (points.length - 1) + '"]').click();
        await page.locator("#removePoint").click(); points.pop();
      }
    }
    await pointDisplays(noCheck);
    await download(page, "capture-empty-points.json", noCheck);
    await page.locator("#resetPoints").click(); await hidden(page);
    await pointDisplays(replay);
    await page.locator('.edit-point[data-frame="0"][data-point="3"]').click();
    await page.locator("#editX").fill(String(target[0]));
    await page.locator("#editY").fill(String(target[1]));
    await page.locator("#applyPoint").click();
    const finalUrl = await page.locator("#correctedInput").getAttribute("href");
    await page.locator("#resetPoints").click(); await hidden(page);
    assert(await page.evaluate(url => globalThis.__revoked.includes(url), finalUrl));
    for (let i = 0; i < report.frames.length; ++i) {
      for (let j = 0; j < report.frames[i].keypoints.length; ++j) {
        assert.deepEqual(await displayed(page, i, j), report.frames[i].keypoints[j].pixel_px);
      }
    }
    assert(await page.locator("#applyPoint").isDisabled());
    assert.deepEqual(await page.locator(".sheet-review").evaluateAll(nodes => nodes.map(n => n.innerHTML)), sheetBefore);
    // Start with the actual native zero-observation review. Coordinates, roles and sigmas below
    // come from the independent synthetic good input, never from the candidate editor/fitter.
    const emptyDir = path.join(root, "empty-review"), empty = read("empty-review/capture.json");
    const emptyReport = read("empty-review/review.json");
    const declarations = read("initial.json");
    assert(declarations.fit.frames.every(f => f.frame && !f.source && !f.calibration_sha256));
    assert.deepEqual(emptyReport.capture_input.input, declarations);
    assert(empty.fit.frames.every(f => !f.frame && f.source && f.calibration_sha256));
    assert.deepEqual(empty, read("empty-full-review/capture.json"));
    assert(empty.fit.frames.every(f => f.keypoints.length === 0));
    assert.equal(emptyReport.capture_input.sources_sha256, report.capture_input.sources_sha256);
    const initialHashes = new Map(["empty.json", "initial.json", "good.json", "input.json", "empty-review/capture.json",
      "empty-review/review.json", "empty-review/index.html", "empty-review/review.js",
      ...emptyReport.frames.map(f => "empty-review/" + f.source_image.file)]
      .map(file => [file, hash(fs.readFileSync(path.join(root, file)))]));
    const placing = await context.newPage();
    await placing.goto(pathToFileURL(path.join(emptyDir, "index.html")).href);
    await placing.waitForFunction(() => !document.getElementById("newPointFields").disabled);
    assert.deepEqual(await placing.locator("#captureInput").evaluate(n => JSON.parse(n.textContent)), empty);
    assert.equal(await placing.locator("tbody tr, section > svg circle").count(), 0);
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
    const svg = placing.locator("section").nth(0).locator(":scope > svg");
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
    assert.equal(await placing.locator("tbody tr, section > svg circle").count(), 0);
    // A new point follows the same refusal bounds as a correction; no hidden zero gets inserted.
    await declare(0, p0);
    for (const [x, y] of [["-1", "200"], ["1280", "200"], ["200", "960"], ["", "200"]]) {
      await placing.locator("#editX").fill(x); await placing.locator("#editY").fill(y);
      await placing.locator("#applyPoint").click(); await hidden(placing);
      assert.equal(await placing.locator("tbody tr, section > svg circle").count(), 0);
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
    assert.equal(await placing.locator("section > svg circle").count(), 20);
    await placing.screenshot({path: path.join(out, "annotated.png"), fullPage: true});
    await placing.locator("#resetPoints").click(); await hidden(placing);
    assert.equal(await placing.locator("tbody tr, section > svg circle").count(), 0);
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
    // Imported masks remain authoritative even if a caller tampers with the live DOM flag.
    assert.equal(await maskedPage.locator('section[data-silhouette-editable="false"]').count(), 2);
    await maskedPage.evaluate(() => {
      document.querySelector("section").dataset.silhouetteEditable = "true";
      const frame = document.getElementById("silhouetteFrame");
      frame.value = "0"; frame.dispatchEvent(new Event("change", {bubbles: true}));
      document.getElementById("applySilhouette").click();
    });
    await hidden(maskedPage);
    assert.deepEqual(await maskedPage.locator("g[data-silhouette]").evaluateAll(ns => ns.map(n => n.outerHTML)), observedMarkup);
    // Author inline sampled contours on the actual generated page, retaining the complete source
    // binding. The native fixture keeps a separately prepared positive fit for this exact evidence.
    const inlineDir = path.join(root, "inline-review"), inlineReport = read("inline-review/review.json");
    const inlineReplay = read("inline-review/capture.json"), inlineGood = read("inline-good.json");
    const disjoint = read("inline-disjoint.json").fit.frames[0].silhouette;
    verifyFiles(inlineDir, inlineReport);
    assert.equal(read("expected-inline-fit.json").status, "ACCEPTED");
    assert.equal(read("inline-check-fit.json").solution, null);
    const contour = await context.newPage();
    await contour.goto(pathToFileURL(path.join(inlineDir, "index.html")).href);
    await contour.waitForFunction(() => !document.getElementById("silhouetteFields").disabled);
    assert.equal(await contour.locator('section[data-silhouette-editable="true"]').count(), 2);
    assert.equal(await contour.locator("g[data-silhouette]").count(), 0);
    async function declareSlice(i, line, evidence, page = contour) {
      await page.locator("#silhouetteFrame").selectOption(String(i));
      await page.locator("#silhouetteLine").selectOption("");
      await page.locator("#silhouetteAxis").selectOption(line.axis);
      await page.locator("#silhouetteAt").fill(String(line.at_px));
      await page.locator("#silhouettePartition").selectOption(line.partition);
      await page.locator("#silhouetteSigma").fill(String(line.sigma_px));
      await page.locator("#silhouetteEvidence").fill(evidence);
      await page.locator("#silhouetteBackground").setChecked(false);
      await page.locator("#silhouetteIntervals").fill(line.intervals_px.map(pair => pair.join(", ")).join("\n"));
      await page.locator("#silhouetteBackground").setChecked(line.intervals_px.length === 0);
      await hidden(page);
    }
    await declareSlice(0, disjoint.lines[1], disjoint.evidence);
    await contour.locator("#applySilhouette").click(); await hidden(contour);
    assert.equal(await contour.locator("g[data-silhouette]").count(), 0);
    const partialContour = structuredClone(inlineReplay);
    partialContour.fit.frames[0].silhouette = {evidence: disjoint.evidence, lines: []};
    for (const line of disjoint.lines) {
      await declareSlice(0, line, disjoint.evidence);
      await contour.locator("#applySilhouette").click();
      partialContour.fit.frames[0].silhouette.lines.push(structuredClone(line));
    }
    await download(contour, "capture-disjoint-slices.json", partialContour);
    assert.equal(await contour.locator("section").nth(0).locator("g[data-silhouette]").count(), 2);
    // Existing evidence keeps its acquisition line, role and uncertainty during endpoint edits.
    await contour.locator("#silhouetteLine").selectOption("0");
    for (const id of ["silhouetteAxis", "silhouetteAt", "silhouettePartition", "silhouetteSigma"]) {
      assert(await contour.locator("#" + id).isDisabled());
    }
    await contour.locator("#silhouetteIntervals").fill("101, 121\n201, 226");
    await hidden(contour);
    await contour.locator("#applySilhouette").click();
    const movedContour = structuredClone(partialContour);
    movedContour.fit.frames[0].silhouette.lines[0].intervals_px = [[101, 121], [201, 226]];
    await download(contour, "capture-moved-slices.json", movedContour);
    await contour.locator("#silhouetteLine").selectOption("1");
    await contour.locator("#removeSilhouette").click();
    movedContour.fit.frames[0].silhouette.lines.pop();
    await download(contour, "capture-removed-slice.json", movedContour);
    await contour.locator("#resetPoints").click(); await hidden(contour);
    assert.equal(await contour.locator("g[data-silhouette]").count(), 0);
    // A horizontal/vertical slice is a fixed coordinate. Actual image clicks supply only interval
    // endpoints through the SVG inverse transform, including responsive sizing and CSS zoom.
    await contour.setViewportSize({width: 600, height: 1000});
    await contour.evaluate(() => document.body.style.zoom = 1.4);
    const clickedLine = {axis: "vertical", at_px: 100.5, partition: "held_out", sigma_px: 2.5,
      intervals_px: [[200, 250]]};
    await declareSlice(1, clickedLine, disjoint.evidence);
    await contour.locator("#silhouetteIntervals").fill("");
    await contour.locator("#prepareSilhouette").click();
    const contourSvg = contour.locator("section").nth(1).locator(":scope > svg");
    async function contourClick(x, y) {
      await contourSvg.scrollIntoViewIfNeeded();
      const screen = await contourSvg.evaluate((svg, xy) => {
        svg.addEventListener("click", e => {
          const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(svg.getScreenCTM().inverse());
          globalThis.__contourPixel = [p.x, p.y];
        }, {once: true, capture: true});
        const p = new DOMPoint(...xy).matrixTransform(svg.getScreenCTM()); return [p.x, p.y];
      }, [x, y]);
      await contour.mouse.click(...screen);
      return contour.evaluate(() => globalThis.__contourPixel);
    }
    const firstEnd = await contourClick(120.25, 200.25);
    await contour.locator("#applySilhouette").click(); await hidden(contour);
    const secondEnd = await contourClick(80.5, 250.75);
    await contour.locator("#applySilhouette").click();
    const clickedContour = structuredClone(inlineReplay);
    clickedContour.fit.frames[1].silhouette = {evidence: disjoint.evidence, lines: [
      {...clickedLine, intervals_px: [[firstEnd[1], secondEnd[1]]]}]};
    await download(contour, "capture-clicked-slice.json", clickedContour);
    const contourUrl = await contour.locator("#correctedInput").getAttribute("href");
    await contour.locator("#silhouetteIntervals").fill("250, 200");
    await hidden(contour);
    assert(await contour.evaluate(url => globalThis.__revoked.includes(url), contourUrl));
    await contour.locator("#applySilhouette").click(); await hidden(contour);
    await contour.locator("#cancelSilhouette").click(); await hidden(contour);
    await contour.locator("#resetPoints").click();
    for (const intervals of [[[100, 210], [200, 225]], [[100, 200], [200, 225]], [[-1, 120]], [[10, 1280]]]) {
      await declareSlice(0, {...disjoint.lines[0], intervals_px: intervals}, disjoint.evidence);
      await contour.locator("#applySilhouette").click(); await hidden(contour);
      assert.equal(await contour.locator("g[data-silhouette]").count(), 0);
    }
    await contour.locator("#resetPoints").click();
    for (let i = 0; i < inlineGood.fit.frames.length; ++i) {
      const silhouette = inlineGood.fit.frames[i].silhouette;
      // Native canonical order may start with background. Explicitly establish foreground first.
      for (const line of [...silhouette.lines].sort((a, b) => Number(!a.intervals_px.length) - Number(!b.intervals_px.length))) {
        await declareSlice(i, line, silhouette.evidence);
        await contour.locator("#applySilhouette").click();
      }
    }
    const authoredContour = structuredClone(inlineReplay);
    authoredContour.fit.frames.forEach((f, i) => {
      const silhouette = structuredClone(inlineGood.fit.frames[i].silhouette);
      silhouette.lines.sort((a, b) => Number(!a.intervals_px.length) - Number(!b.intervals_px.length));
      f.silhouette = silhouette;
    });
    const inlineFile = await download(contour, "capture-inline-silhouettes.json", authoredContour);
    assert.equal(authoredContour.reviewed_sources_sha256, inlineReplay.reviewed_sources_sha256);
    assert.equal(await contour.locator("g[data-silhouette]").count(), 10);
    await contour.locator("#resetPoints").click(); await hidden(contour);
    assert.equal(await contour.locator("g[data-silhouette]").count(), 0);
    // Committing a contour cannot silently discard a still-visible point correction or a new
    // observation awaiting placement. Both Apply and Remove must wait for explicit resolution.
    for (const newPoint of [false, true]) for (const action of ["apply", "remove"]) {
      const page = newPoint ? placing : contour, original = newPoint ? empty : inlineReplay;
      await page.locator("#resetPoints").click();
      for (const line of disjoint.lines) {
        await declareSlice(0, line, disjoint.evidence, page);
        await page.locator("#applySilhouette").click();
      }
      await page.locator("#silhouetteLine").selectOption(action === "apply" ? "0" : "1");
      if (action === "apply") await page.locator("#silhouetteIntervals").fill("101, 121\n201, 226");
      const pixel = newPoint ? p0.pixel_px : [original.fit.frames[0].keypoints[0].pixel_px[0] + .125,
        original.fit.frames[0].keypoints[0].pixel_px[1]];
      if (newPoint) await declare(0, p0);
      else {
        await page.locator('.edit-point[data-frame="0"][data-point="0"]').click();
        // Coordinates are not pending yet: the silhouette edit alone must prevent removal.
        await page.locator("#removePoint").click(); await hidden(page);
        assert.equal(await page.locator("section").nth(0).locator("tbody tr").count(), original.fit.frames[0].keypoints.length);
        assert.deepEqual(await displayed(page, 0, 0), original.fit.frames[0].keypoints[0].pixel_px);
      }
      await page.locator("#editX").fill(String(pixel[0]));
      await page.locator("#editY").fill(String(pixel[1]));
      const observedBefore = await page.locator("g[data-silhouette]").evaluateAll(nodes => nodes.map(n => n.outerHTML));
      const button = page.locator(action === "apply" ? "#applySilhouette" : "#removeSilhouette");
      if (!newPoint) { await page.locator("#removePoint").click(); await hidden(page); }
      await button.click(); await hidden(page);
      assert.deepEqual(await page.locator("g[data-silhouette]").evaluateAll(nodes => nodes.map(n => n.outerHTML)), observedBefore);
      assert.equal(await page.locator("#editX").inputValue(), String(pixel[0]));
      assert.equal(await page.locator("#editY").inputValue(), String(pixel[1]));
      if (newPoint) assert.equal(await page.locator("tbody tr").count(), 0);
      else assert.deepEqual(await displayed(page, 0, 0), original.fit.frames[0].keypoints[0].pixel_px);
      const expected = structuredClone(original);
      expected.fit.frames[0].silhouette = structuredClone(disjoint);
      if (newPoint && action === "remove") {
        await page.locator("#cancelPoint").click();
        assert.equal(await page.locator("tbody tr").count(), 0);
      } else {
        await page.locator("#applyPoint").click();
        if (newPoint) expected.fit.frames[0].keypoints.push(structuredClone(p0));
        else expected.fit.frames[0].keypoints[0].pixel_px = pixel;
      }
      await hidden(page);
      await button.click();
      if (action === "apply") expected.fit.frames[0].silhouette.lines[0].intervals_px = [[101, 121], [201, 226]];
      else expected.fit.frames[0].silhouette.lines.pop();
      await download(page, "capture-pending-" + (newPoint ? "new" : "existing") + "-" + action + ".json", expected);
      await page.locator("#resetPoints").click(); await hidden(page);
    }
    verifyFiles(inlineDir, inlineReport);
    const staticContext = await browser.newContext({javaScriptEnabled: false, userAgent});
    observe(staticContext);
    const staticPage = await staticContext.newPage();
    await staticPage.goto(pathToFileURL(path.join(reviewDir, "index.html")).href);
    assert(await staticPage.locator("#captureEditor").isHidden());
    assert.equal(await staticPage.locator("section > svg circle").count(), 20);
    assert.equal(await staticPage.locator("tbody tr").count(), 20);
    assert(await staticPage.locator("noscript").isVisible());
    assert.equal(await sheetReview(staticPage), 18);
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
    assert.equal(await unsafePage.locator("#silhouetteFields").evaluate(n => n.disabled), true);
    assert.equal(await unsafePage.locator("section > svg circle").count(), 20);
    assert((await unsafePage.locator("#captureInput").textContent()).includes("9007199254740993"));
    assert.deepEqual(errors, []);
    verifyFiles(maskDir, maskReport);
    assert.deepEqual(maskSources.map(file => hash(fs.readFileSync(path.join(root, file)))), maskHashes);
    const allowed = [reviewDir, emptyDir, maskDir, inlineDir, path.join(root, "numeric-unsafe/review")].map(dir => pathToFileURL(dir + path.sep).href);
    assert(requests.every(r => r.method === "GET" && allowed.some(prefix => r.url.startsWith(prefix))));
    verifyFiles();
    console.log(JSON.stringify({status: "PASS", frames: 2, points: 20, clickLayouts: 3,
      invalidEdits: 5, correctedFile: final, sha256: hash(fs.readFileSync(final)),
      initiallyPlaced: 20, annotatedFile, annotatedSha256: hash(fs.readFileSync(annotatedFile)), errors, requests,
      observedSlices: 10, sheetPoints, maskedFile, maskedSha256: hash(fs.readFileSync(maskedFile)),
      authoredSlices: 10, inlineFile, inlineSha256: hash(fs.readFileSync(inlineFile)),
      crossEditorPendingCases: 4,
      removedFile, noCheckFile,
      boundary: "Synthetic actual page and data only; fit acceptance is a separate ordinary CLI run"}, null, 2));
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });

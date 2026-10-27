// Actual offline model picker -> native target annotation document. No renderer doubles.
// Prepare with FTX_BODY_TARGET_DIR and targets_editor_fixture_ in body_fit_workflow.
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const {pathToFileURL} = require("node:url");
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
if (process.argv.length !== 4) throw Error("Usage: node tools/check_body_targets.cjs CASE_DIR NEW_OUTPUT_DIR");
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]);
const review = path.join(root, "review"), read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const hash = b => crypto.createHash("sha256").update(b).digest("hex");
const report = read("review/review.json"), geometry = read("review/model.json");
const seed = read("review/body-targets.json"), good = read("good.json");
const original = fs.readFileSync(path.join(root, "input.json"));
assert.equal(report.body_fitted, false);
assert.equal(report.preview.status, "UNFITTED");
assert(!Object.hasOwn(seed.keypoints, "p0"));
assert.equal(hash(original), seed.source_input_sha256);
for (const item of [report.original_input, report.annotations, report.geometry, report.viewer, report.editor]) {
  const bytes = fs.readFileSync(path.join(review, item.file));
  assert.equal(bytes.length, item.bytes); assert.equal(hash(bytes), item.sha256);
}
fs.mkdirSync(out, {mode: 0o700});

// Independently project a supplied native rest-mesh point through the documented front view.
// This must use the bitmap's actual displayed area, including at a constrained viewport and zoom.
const vertices = geometry.mesh.vertices_m;
const center = [0, 1, 2].map(k => (Math.min(...vertices.map(v => v[k])) + Math.max(...vertices.map(v => v[k]))) / 2);
const radius = Math.max(...vertices.map(v => Math.hypot(...v.map((x, k) => x - center[k]))));
function surfacePoint(face, weights) {
  return [0, 1, 2].map(k => geometry.mesh.triangles[face].reduce((s, index, j) => s + weights[j] * vertices[index][k], 0));
}
async function clickPoint(page, point, zoom) {
  const canvas = page.locator("#targetCanvas");
  await page.waitForFunction(() => document.getElementById("targetCanvas").dataset.rendered === "true");
  await canvas.scrollIntoViewIfNeeded();
  const box = await canvas.boundingBox();
  assert(box);
  const scale = 700 * 0.45 * zoom / radius;
  const xy = [450 + (point[0] - center[0]) * scale, 350 - (point[2] - center[2]) * scale];
  assert(xy[0] > 0 && xy[0] < 900 && xy[1] > 0 && xy[1] < 700);
  await page.mouse.click(box.x + xy[0] / 900 * box.width, box.y + xy[1] / 700 * box.height);
}
async function zoomTo(page, value) {
  await page.locator("#zoom").evaluate((node, next) => {
    node.value = String(next); node.dispatchEvent(new Event("input", {bubbles: true}));
  }, value);
}
async function candidate(page) {
  return JSON.parse((await page.locator("#targetDefinition").innerText()).split("\nPreview location")[0]);
}
async function hidden(page) {
  assert(await page.locator("#targetDownload").isHidden());
  assert.equal(await page.locator("#targetDownload").getAttribute("href"), null);
}
async function newTarget(page, name, kind) {
  await page.locator("#targetList").selectOption("");
  await page.locator("#targetName").fill(name);
  await page.locator("#targetKind").selectOption(kind);
}
async function addP0(page, zoom) {
  await page.locator("#targetList").selectOption("p0");
  await page.locator("#targetKind").selectOption("vertex");
  await page.locator('[data-view="front"]').click();
  await zoomTo(page, zoom);
  // Face 5 is the near (-Y) side of the independently authored synthetic box. A point
  // just inside its lower corner selects vertex 0 instead of relying on edge tie breaking.
  assert.deepEqual(geometry.mesh.triangles[5], [0, 4, 5]);
  await clickPoint(page, surfacePoint(5, [0.97, 0.015, 0.015]), zoom);
  assert.deepEqual(await candidate(page), {kind: "vertex", index: 0});
  await page.locator("#applyTarget").click();
  assert(await page.locator("#targetDownload").isVisible());
}

(async () => {
  const browser = await chromium.launch({headless: true});
  const errors = [], requests = [];
  try {
    const context = await browser.newContext({viewport: {width: 1400, height: 850},
      userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0"});
    context.on("request", request => requests.push({url: request.url(), method: request.method()}));
    await context.addInitScript(() => {
      globalThis.__revoked = [];
      const revoke = URL.revokeObjectURL.bind(URL);
      URL.revokeObjectURL = url => { globalThis.__revoked.push(url); return revoke(url); };
    });
    const page = await context.newPage();
    page.on("pageerror", error => errors.push(String(error)));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(pathToFileURL(path.join(review, "index.html")).href);
    await page.waitForFunction(() => !document.getElementById("targetEditor").hidden);
    assert.equal(await page.evaluate(() => globalThis.targetInjection), undefined);
    assert.equal(await page.locator("script").count(), 2);
    await hidden(page);
    await addP0(page, 1);

    // A queued view change must not select from the previous frame's projected triangles.
    await page.evaluate(() => {
      const yaw = document.getElementById("yaw"), canvas = document.getElementById("targetCanvas");
      yaw.value = "180"; yaw.dispatchEvent(new Event("input", {bubbles: true}));
      const rect = canvas.getBoundingClientRect();
      canvas.dispatchEvent(new MouseEvent("click", {bubbles: true,
        clientX: rect.x + rect.width / 2, clientY: rect.y + rect.height / 2}));
    });
    assert.match(await page.locator("#targetStatus").innerText(), /Wait for the current model view/);
    await page.locator('[data-view="front"]').click();
    await page.locator("#cancelTarget").click();

    const firstUrl = await page.locator("#targetDownload").getAttribute("href");
    await newTarget(page, "pending", "surface");
    await hidden(page);
    assert((await page.evaluate(() => globalThis.__revoked)).includes(firstUrl));
    await page.locator("#cancelTarget").click();

    await newTarget(page, "surface_sample", "surface");
    await clickPoint(page, surfacePoint(5, [0.55, 0.2, 0.25]), 1);
    const surface = await candidate(page);
    assert.equal(surface.kind, "surface"); assert.equal(surface.face, 5);
    assert(Math.abs(surface.barycentric.reduce((a, b) => a + b, 0) - 1) < 1e-12);
    surface.barycentric.forEach((w, i) => assert(Math.abs(w - [0.55, 0.2, 0.25][i]) < 0.01));
    await page.locator("#applyTarget").click();
    assert(await page.locator("#targetDownload").isVisible());
    await page.locator("#removeTarget").click();
    assert.equal(await page.locator('#targetList option[value="surface_sample"]').count(), 0);

    for (const kind of ["bone_head", "bone_tail"]) {
      await newTarget(page, "rig_sample", kind);
      await page.locator("#targetBone").selectOption("arm");
      assert.deepEqual(await candidate(page), {kind, bone: "arm"});
      await page.locator("#applyTarget").click();
      assert(await page.locator("#targetDownload").isVisible());
      await page.locator("#removeTarget").click();
    }
    // Selecting an observed name cannot remove it, including after a DOM-only control change.
    await page.locator("#targetList").selectOption("p0");
    assert(await page.locator("#removeTarget").isDisabled());
    await page.locator("#removeTarget").evaluate(node => { node.disabled = false; });
    await page.locator("#removeTarget").click();
    await hidden(page);
    await page.locator("#cancelTarget").click();

    await page.locator("#resetTargets").click();
    await hidden(page);
    // A second aspect ratio and zoom catches wrong canvas-to-bitmap coordinate scaling.
    await page.setViewportSize({width: 900, height: 1100});
    await addP0(page, 1.2);
    const event = page.waitForEvent("download");
    await page.locator("#targetDownload").click();
    const download = await event;
    assert.equal(download.suggestedFilename(), "body-targets.json");
    const destination = path.join(out, "body-targets.json");
    await download.saveAs(destination);
    const annotation = JSON.parse(fs.readFileSync(destination));
    assert.deepEqual(annotation, {...seed, keypoints: good.model.keypoints});
    assert.deepEqual(fs.readFileSync(path.join(root, "input.json")), original);
    assert.deepEqual(errors, []);
    assert(requests.every(r => r.method === "GET" && r.url.startsWith("file:")));
    console.log(JSON.stringify({status: "PASS", authored_targets: Object.keys(annotation.keypoints).length,
      exercised: ["vertex", "surface", "bone_head", "bone_tail", "pending", "cancel", "reset", "observed-name lock", "two viewport sizes"],
      download: destination, browser_errors: errors.length, local_requests: requests.length}));
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

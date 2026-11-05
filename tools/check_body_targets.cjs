// Actual offline model picker -> native target annotation document. No renderer doubles.
// Prepare with FTX_BODY_TARGET_DIR and targets_editor_fixture_ in body_fit_workflow.
// FTX_BODY_TARGET_SOURCE tests exact source JS against a retained review, not a rebuilt CLI.
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const {pathToFileURL} = require("node:url");
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
if (process.argv.length !== 4) throw Error("Usage: node tools/check_body_targets.cjs CASE_DIR NEW_OUTPUT_DIR");
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]);
const review = path.join(root, "review"), read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const hash = b => crypto.createHash("sha256").update(b).digest("hex");
const sourceScript = process.env.FTX_BODY_TARGET_SOURCE
  ? fs.readFileSync(path.resolve(process.env.FTX_BODY_TARGET_SOURCE)) : null;
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
async function allHidden(page) {
  await hidden(page);
  assert(await page.locator("#targetDraftDownload").isHidden());
  assert.equal(await page.locator("#targetDraftDownload").getAttribute("href"), null);
}
async function saveDownload(page, id, filename) {
  const event = page.waitForEvent("download");
  await page.locator("#" + id).click();
  const download = await event;
  assert.equal(download.suggestedFilename(), filename);
  const destination = path.join(out, filename);
  await download.saveAs(destination);
  return destination;
}
async function loadTargets(page, value, name = "saved-targets.json") {
  const buffer = Buffer.isBuffer(value) ? value : Buffer.from(JSON.stringify(value));
  await page.locator("#targetDraftFile").setInputFiles({name, mimeType: "application/json", buffer});
  assert.equal(await page.locator("#targetDraftFile").inputValue(), "");
}
async function restored(page) {
  await page.waitForFunction(() => document.getElementById("targetStatus").textContent.includes("applied targets restored"));
}
async function targetNames(page) {
  return page.locator("#targetList option").evaluateAll(nodes => nodes.map(node => node.value).sort());
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
    let sourceLoads = 0;
    if (sourceScript) await context.route(pathToFileURL(path.join(review, report.editor.file)).href, route => {
      ++sourceLoads;
      return route.fulfill({contentType: 'application/javascript', body: sourceScript});
    });
    context.on("request", request => requests.push({url: request.url(), method: request.method()}));
    await context.addInitScript(() => {
      globalThis.__revoked = [];
      const revoke = URL.revokeObjectURL.bind(URL);
      URL.revokeObjectURL = url => { globalThis.__revoked.push(url); return revoke(url); };
      // Only file-read latency is controlled; real File bytes, decoder, picker and renderer execute.
      const arrayBuffer = File.prototype.arrayBuffer;
      File.prototype.arrayBuffer = function () {
        const bytes = arrayBuffer.call(this);
        if (!this.name.startsWith("slow-")) return bytes;
        return new Promise((resolve, reject) => {
          globalThis.__releaseTargetRead = () => bytes.then(resolve, reject);
          globalThis.__rejectTargetRead = () => reject(new Error("Synthetic file read failure"));
        });
      };
    });
    const page = await context.newPage();
    page.on("pageerror", error => errors.push(String(error)));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(pathToFileURL(path.join(review, "index.html")).href);
    await page.waitForFunction(() => !document.getElementById("targetEditor").hidden);
    if (sourceScript) assert(sourceLoads > 0, 'The target source override must actually execute.');
    assert.equal(await page.evaluate(() => globalThis.targetInjection), undefined);
    assert.equal(await page.locator("script").count(), 2);
    await hidden(page);
    assert.equal(await page.locator("#targetDraftDownload").count(), 1);
    assert(await page.locator("#targetDraftDownload").isVisible());
    await newTarget(page, "saved_surface", "surface");
    await clickPoint(page, surfacePoint(5, [0.55, 0.2, 0.25]), 1);
    await page.locator("#applyTarget").click();
    await hidden(page);
    const partialPath = await saveDownload(page, "targetDraftDownload", "body-targets-draft.json");
    const partial = JSON.parse(fs.readFileSync(partialPath));
    assert.equal(partial.source_input_sha256, seed.source_input_sha256);
    assert.equal(partial.model_sha256, seed.model_sha256);
    assert(!Object.hasOwn(partial.keypoints, "p0"));
    assert.equal(partial.keypoints.saved_surface.kind, "surface");
    await page.reload();
    await page.waitForFunction(() => !document.getElementById("targetEditor").hidden);
    await page.locator("#targetDraftFile").setInputFiles(partialPath);
    await restored(page);
    assert.equal(await page.locator("#targetDraftFile").inputValue(), "");
    await hidden(page);
    assert.match(await page.locator("#targetStatus").innerText(), /missing observed targets: p0/);
    await page.locator("#targetList").selectOption("saved_surface");
    assert.deepEqual(await candidate(page), partial.keypoints.saved_surface);
    await page.locator("#removeTarget").click();
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

    const complete = {...seed, keypoints: good.model.keypoints};
    const wrong = (key, value) => ({...complete, [key]: value});
    const target = value => ({...complete, keypoints: {...complete.keypoints, p0: value}});
    const invalid = [
      wrong("schema", "ftx-body-targets/2"), wrong("source_input", seed.source_input + "-other"),
      wrong("source_input_sha256", "0".repeat(64)), wrong("model_sha256", "0".repeat(64)),
      wrong("extra", true), wrong("keypoints", null), wrong("keypoints", []), wrong("keypoints", {}),
      wrong("keypoints", Object.fromEntries(Array.from({length: 129}, (_, i) => ["many" + i, {kind: "vertex", index: 0}]))),
      ...[" ", "é".repeat(129), "\ud800"].map(name => wrong("keypoints", {[name]: {kind: "vertex", index: 0}})),
      target(null), target({kind: "unknown"}), target({kind: "vertex"}),
      target({kind: "vertex", index: 0, extra: true}), target({kind: "vertex", index: -1}),
      target({kind: "vertex", index: 100000}), target({kind: "vertex", index: 0.5}),
      target({kind: "vertex", index: 1}),
      target({kind: "surface", face: 0, barycentric: [0, 1, 0]}),
      target({kind: "surface", face: 200000, barycentric: [0.2, 0.3, 0.5]}),
      target({kind: "surface", face: 0, barycentric: [-0.1, 0.6, 0.5]}),
      target({kind: "surface", face: 0, barycentric: [0.2, 0.3, 0.4]}),
      target({kind: "surface", face: 0, barycentric: [1, 0]}),
      target({kind: "bone_head", bone: "absent"}), target({kind: "bone_tail", bone: "arm", index: 0}),
      Buffer.from("{"), Buffer.from([0xff]),
      Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from(JSON.stringify(complete))]),
      Buffer.alloc(256 * 1024 + 1, 0x20),
    ];
    const names = await targetNames(page);
    for (let i = 0; i < invalid.length; ++i) {
      const oldUrl = await page.locator("#targetDownload").getAttribute("href");
      assert(oldUrl);
      await loadTargets(page, invalid[i], "invalid-" + i + ".json");
      await page.waitForFunction(() => document.getElementById("targetStatus").textContent.startsWith("Saved targets not loaded:"));
      await allHidden(page);
      assert((await page.evaluate(() => globalThis.__revoked)).includes(oldUrl));
      assert.deepEqual(await targetNames(page), names);
      await page.locator("#targetList").selectOption("p0");
      assert.deepEqual(await candidate(page), {kind: "vertex", index: 0});
      await allHidden(page);
      await page.locator("#cancelTarget").click();
      assert(await page.locator("#targetDownload").isVisible());
    }
    // Choosing, cancelling or failing to read a replacement must not republish old applied maps
    // merely because an existing target is selected. Explicit Apply/Cancel resolves the import.
    for (const event of ["focus", "click", "cancel", "empty"]) {
      const oldUrl = await page.locator("#targetDownload").getAttribute("href");
      if (event === "empty") await page.locator("#targetDraftFile").setInputFiles([]);
      else await page.locator("#targetDraftFile").dispatchEvent(event);
      await allHidden(page);
      assert((await page.evaluate(() => globalThis.__revoked)).includes(oldUrl));
      await page.locator("#targetList").selectOption("p0");
      await allHidden(page);
      assert.deepEqual(await candidate(page), {kind: "vertex", index: 0});
      await page.locator("#cancelTarget").click();
      assert(await page.locator("#targetDownload").isVisible());
    }
    for (const reject of [false, true]) {
      await loadTargets(page, complete, "slow-unresolved.json");
      await page.waitForFunction(() => typeof globalThis.__releaseTargetRead === "function");
      await allHidden(page);
      if (reject) {
        await page.evaluate(() => globalThis.__rejectTargetRead());
        await page.waitForFunction(() => document.getElementById("targetStatus").textContent.startsWith("Saved targets not loaded:"));
      }
      await page.locator("#targetList").selectOption("p0");
      await allHidden(page);
      if (!reject) await page.evaluate(async () => { await globalThis.__releaseTargetRead(); });
      await allHidden(page);
      await page.locator("#applyTarget").click();
      assert(await page.locator("#targetDownload").isVisible());
      await page.evaluate(() => { globalThis.__releaseTargetRead = null; globalThis.__rejectTargetRead = null; });
    }
    for (const pendingEdit of [false, true]) {
      if (pendingEdit) await page.locator("#targetKind").selectOption("surface");
      const before = await page.locator("#targetDefinition").innerText();
      await page.evaluate(() => {
        window.dispatchEvent(new PageTransitionEvent("pagehide", {persisted: true}));
        window.dispatchEvent(new PageTransitionEvent("pageshow", {persisted: true}));
      });
      await allHidden(page);
      assert.equal(await page.locator("#targetDefinition").innerText(), before);
      assert.match(await page.locator("#targetStatus").innerText(), /cancel.*page|page.*cancel/i);
      await page.locator("#targetList").selectOption("p0");
      await allHidden(page);
      await page.locator("#cancelTarget").click();
      assert(await page.locator("#targetDownload").isVisible());
    }
    const completeBytes = Buffer.from(JSON.stringify(complete));
    await loadTargets(page, Buffer.concat([completeBytes, Buffer.alloc(256 * 1024 - completeBytes.length, 0x20)]));
    await restored(page); await hidden(page);
    await page.locator("#targetList").selectOption("p0");
    await hidden(page);
    await page.locator("#cancelTarget").click(); await hidden(page);
    await page.locator("#applyTarget").click();
    assert(await page.locator("#targetDownload").isVisible());

    const races = ["name", "selection", "kind", "canvas", "cancel", "reset", "chooser", "replacement", "pagehide"];
    for (const action of races) {
      await page.locator("#targetList").selectOption("");
      await loadTargets(page, {...seed, keypoints: {late: {kind: "vertex", index: 0}}}, "slow-" + action + ".json");
      await page.waitForFunction(() => typeof globalThis.__releaseTargetRead === "function");
      await allHidden(page);
      if (action === "name") await page.locator("#targetName").fill("later-name");
      if (action === "selection") await page.locator("#targetList").selectOption("p1");
      if (action === "kind") await page.locator("#targetKind").selectOption("bone_head");
      if (action === "canvas") await clickPoint(page, surfacePoint(5, [0.6, 0.2, 0.2]), 1);
      if (action === "cancel") await page.locator("#cancelTarget").click();
      if (action === "reset") await page.locator("#resetTargets").click();
      if (action === "chooser") {
        const choosing = page.waitForEvent("filechooser");
        await page.locator("#targetDraftFile").click();
        await (await choosing).setFiles([]);
      }
      if (action === "replacement") { await loadTargets(page, partial); await restored(page); }
      if (action === "pagehide") await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pagehide")));
      await page.evaluate(async () => { await globalThis.__releaseTargetRead(); globalThis.__releaseTargetRead = null; });
      assert.equal(await page.locator('#targetList option[value="late"]').count(), 0, action);
      assert(!((await page.locator("#targetStatus").innerText()).startsWith("Reading saved targets;")), action);
      if (action === "replacement") assert.equal(await page.locator('#targetList option[value="saved_surface"]').count(), 1);
      await page.locator("#resetTargets").click();
      await addP0(page, 1);
    }

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
    if (sourceScript) assert(sourceLoads > 0, 'The target source override must actually execute.');
    assert.deepEqual(errors, []);
    assert(requests.every(r => r.method === "GET" && r.url.startsWith("file:")));
    console.log(JSON.stringify({status: "PASS", authored_targets: Object.keys(annotation.keypoints).length,
      editor_script: {mode: sourceScript ? 'source override' : 'compiled review',
        sha256: sourceScript ? hash(sourceScript) : report.editor.sha256, source_loads: sourceLoads},
      draft: partialPath, invalid_drafts: invalid.length, read_races: races,
      exercised: ["vertex", "surface", "bone_head", "bone_tail", "pending", "cancel", "reset", "observed-name lock", "two viewport sizes", "unresolved-import link withdrawal", "read failure preserves work", "cached-page recovery"],
      download: destination, browser_errors: errors.length, local_requests: requests.length}));
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

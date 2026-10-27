// Actual offline start editor -> native initialization replay -> ordinary body fitter.
// Prepare with FTX_BODY_START_DIR and start_edits_editor_fixture_ in body_fit_workflow.
// Synthetic numerical acceptance only; no photograph, anatomical or physical-fit claim.
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const {spawnSync} = require("node:child_process");
const {pathToFileURL} = require("node:url");
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
if (process.argv.length !== 5) throw Error("Usage: node tools/check_body_starts.cjs CASE_DIR NEW_OUTPUT_DIR FTX_BINARY");
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]), binary = path.resolve(process.argv[4]);
const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const report = read("review/review.json"), seed = read("review/body-starts.json");
const expected = read("expected-annotations.json"), original = fs.readFileSync(path.join(root, "input.json"));
assert.deepEqual(seed.starts, []);
assert.equal(seed.source_input_sha256, hash(original));
assert.equal(report.body_fitted, false);
for (const item of [report.original_input, report.start_editor.annotations, report.viewer, report.start_editor.script]) {
  const bytes = fs.readFileSync(path.join(root, "review", item.file));
  assert.equal(bytes.length, item.bytes); assert.equal(hash(bytes), item.sha256);
}
fs.mkdirSync(out, {mode: 0o700});

function command(cwd, args) {
  const result = spawnSync(binary, [...args, "--json"], {cwd, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, timeout: 120000});
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  return JSON.parse(result.stdout).result;
}
function control(page, group, index, frame) {
  return page.locator(`#startParameters input[data-group="${group}"][data-parameter="${index}"]${frame === undefined ? "" : `[data-frame="${frame}"]`}`);
}
async function hidden(page) {
  assert(await page.locator("#startDownload").isHidden());
  assert.equal(await page.locator("#startDownload").getAttribute("href"), null);
}
async function visible(page) {
  assert(await page.locator("#startDownload").isVisible());
  assert.match(await page.locator("#startDownload").getAttribute("href"), /^blob:/);
}
async function fill(page, start) {
  for (const [i, value] of start.shape.entries()) await control(page, "shape", i).fill(String(value));
  for (const pose of start.poses) {
    for (const [i, value] of pose.values.entries()) await control(page, "pose", i, pose.frame_index).fill(String(value));
  }
}
async function appliedCount(page) {
  return page.locator("#startList").evaluate(node => [...node.options].filter(option => option.value !== "").length);
}
async function revoked(page, url) {
  assert((await page.evaluate(() => globalThis.__revoked)).includes(url));
}
async function add(page, start) {
  await page.locator("#newStart").click();
  await fill(page, start);
  await page.locator("#applyStart").click();
}

(async () => {
  const browser = await chromium.launch({headless: true});
  const errors = [], requests = [];
  try {
    const context = await browser.newContext({viewport: {width: 1100, height: 900},
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
    await page.goto(pathToFileURL(path.join(root, "review/index.html")).href);
    await page.waitForFunction(() => !document.getElementById("startEditor").hidden);
    assert.equal(await page.evaluate(() => globalThis.startInjection), undefined);
    await hidden(page);
    await page.locator("#newStart").click();
    assert((await page.locator("#startParameters input").evaluateAll(nodes => nodes.map(node => node.value))).every(value => value === ""));
    const labels = await page.locator("#startParameters").innerText();
    for (const parameter of [...report.model.shape, ...report.model.pose]) assert(labels.includes(parameter.name));
    assert(labels.includes("dimensionless"));
    for (const [group, parameters] of [["shape", report.model.shape], ["pose", report.model.pose]]) {
      for (const [index, parameter] of parameters.entries()) {
        const inputs = page.locator(`#startParameters input[data-group="${group}"][data-parameter="${index}"]`);
        for (const input of await inputs.all()) {
          assert.equal(await input.getAttribute("type"), "number");
          assert.equal(Number(await input.getAttribute("min")), parameter.lower);
          assert.equal(Number(await input.getAttribute("max")), parameter.upper);
        }
      }
    }
    await page.locator("#applyStart").click();
    assert.equal(await appliedCount(page), 0);
    await hidden(page);

    // Midpoints are an explicit guess, never an automatically inferred body/pose.
    await page.locator("#midpointStart").click();
    for (const [i, parameter] of report.model.shape.entries()) {
      assert.equal(Number(await control(page, "shape", i).inputValue()), parameter.lower + (parameter.upper - parameter.lower) / 2);
    }
    for (const pose of expected.starts[0].poses) {
      for (const [i, parameter] of report.model.pose.entries()) {
        assert.equal(Number(await control(page, "pose", i, pose.frame_index).inputValue()), parameter.lower + (parameter.upper - parameter.lower) / 2);
      }
    }
    await page.locator("#cancelStart").click();
    assert.equal(await appliedCount(page), 0);
    await add(page, expected.starts[0]);
    assert.equal(await appliedCount(page), 1);
    await hidden(page);

    await page.locator("#duplicateStart").click();
    await page.locator("#applyStart").click();
    assert.equal(await appliedCount(page), 1);
    assert.match(await page.locator("#startStatus").innerText(), /distinct|duplicate/i);
    await hidden(page);
    await fill(page, expected.starts[1]);
    await page.locator("#applyStart").click();
    assert.equal(await appliedCount(page), 2);
    await visible(page);

    let url = await page.locator("#startDownload").getAttribute("href");
    await control(page, "shape", 0).fill("");
    await hidden(page); await revoked(page, url);
    await page.locator("#cancelStart").click();
    await visible(page);
    assert.equal(Number(await control(page, "shape", 0).inputValue()), expected.starts[1].shape[0]);
    url = await page.locator("#startDownload").getAttribute("href");
    await control(page, "shape", 0).fill(String(report.model.shape[0].lower));
    await page.locator("#applyStart").click();
    await hidden(page); await revoked(page, url);
    assert.match(await page.locator("#startStatus").innerText(), /bound|strict|interior|between/i);
    assert.equal(await appliedCount(page), 2);
    await page.locator("#cancelStart").click();
    await visible(page);
    url = await page.locator("#startDownload").getAttribute("href");
    await page.locator("#removeStart").click();
    assert.equal(await appliedCount(page), 1);
    await hidden(page); await revoked(page, url);
    await add(page, expected.starts[1]);

    // Copies can be edited into distinct starts, up to the ordinary native limit.
    for (const value of [0.58, 0.59]) {
      await page.locator("#duplicateStart").click();
      await control(page, "shape", 0).fill(String(value));
      await page.locator("#applyStart").click();
    }
    assert.equal(await appliedCount(page), 4);
    assert(await page.locator("#newStart").isDisabled());
    assert(await page.locator("#duplicateStart").isDisabled());
    url = await page.locator("#startDownload").getAttribute("href");
    await page.locator("#resetStarts").click();
    assert.equal(await appliedCount(page), 0);
    await hidden(page); await revoked(page, url);
    for (const start of expected.starts) await add(page, start);
    await visible(page);
    assert.match(await page.locator("#startPreviewNotice").innerText(), /regenerat|generate a new native review|fit-start-review/i);

    const event = page.waitForEvent("download");
    await page.locator("#startDownload").click();
    const download = await event;
    assert.equal(download.suggestedFilename(), "body-starts.json");
    const destination = path.join(out, "body-starts.json");
    await download.saveAs(destination);
    assert.deepEqual(JSON.parse(fs.readFileSync(destination)), expected);
    assert.deepEqual(fs.readFileSync(path.join(root, "input.json")), original);

    // Existing geometry and exact long native timestamp labels must stay frozen while editing.
    await page.goto(pathToFileURL(path.join(root, "seeded-review/index.html")).href);
    await page.waitForFunction(() => !document.getElementById("startEditor").hidden);
    assert.equal(await page.evaluate(() => globalThis.startInjection), undefined);
    const projections = await page.locator("svg").evaluateAll(nodes => nodes.map(node => node.outerHTML));
    const meshes = await page.locator('a[href$=".obj"]').evaluateAll(nodes => nodes.map(node => node.getAttribute("href")));
    assert.equal(projections.length, 4); assert.equal(meshes.length, 4);
    assert((await page.locator("body").innerText()).includes("9007199254740993"));
    await page.locator("#startList").selectOption("0");
    await control(page, "shape", 0).fill("0.5");
    await page.locator("#applyStart").click();
    assert.deepEqual(await page.locator("svg").evaluateAll(nodes => nodes.map(node => node.outerHTML)), projections);
    assert.deepEqual(await page.locator('a[href$=".obj"]').evaluateAll(nodes => nodes.map(node => node.getAttribute("href"))), meshes);
    assert.match(await page.locator("#startPreviewNotice").innerText(), /original|unchanged|frozen/i);

    const applied = path.join(out, "authored"), model = path.join(root, "model.ftxbody");
    command(out, ["measure", "fit-starts", destination, "--model", model, "--out", applied]);
    const inputBytes = fs.readFileSync(path.join(applied, "input.json"));
    assert.deepEqual(inputBytes, fs.readFileSync(path.join(root, "authored/input.json")));
    assert(inputBytes.includes(Buffer.from("9007199254740993")));
    const fitted = command(applied, ["measure", "fit-body", "input.json", "--model", model, "--out", "fit.json"]);
    const reference = read("authored/expected-fit.json");
    assert.equal(fitted.status, "ACCEPTED");
    assert.equal(fitted.input_sha256, hash(inputBytes));
    assert.equal(fitted.capture_input.source, fs.realpathSync(path.join(applied, "input.json")));
    assert.equal(reference.capture_input.source, fs.realpathSync(path.join(root, "authored/input.json")));
    reference.capture_input.source = fitted.capture_input.source;
    assert.deepEqual(fitted, reference);
    assert.deepEqual(errors, []);
    assert(requests.every(request => request.method === "GET" && request.url.startsWith("file:")));
    console.log(JSON.stringify({status: "PASS", authored_starts: expected.starts.length, fit: fitted.status,
      exercised: ["empty bootstrap", "named bounded parameters", "midpoint", "copy", "distinctness", "partial edit", "boundary", "cancel", "remove", "maximum starts", "reset", "frozen native previews", "exact native frame identities", "download to ordinary fit"],
      download: destination, browser_errors: errors.length, local_requests: requests.length}));
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

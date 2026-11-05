// Actual offline start editor -> native initialization replay -> ordinary body fitter.
// Prepare with FTX_BODY_START_DIR and start_edits_editor_fixture_ in body_fit_workflow.
// Synthetic numerical acceptance only; no photograph, anatomical or physical-fit claim.
// FTX_START_EDITOR_SOURCE tests exact source JS against a retained native review; not a rebuilt CLI.
const assert = require("node:assert/strict");
const fs = require("node:fs"), path = require("node:path"), crypto = require("node:crypto");
const {spawnSync} = require("node:child_process");
const {pathToFileURL} = require("node:url");
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
if (process.argv.length !== 5) throw Error("Usage: node tools/check_body_starts.cjs CASE_DIR NEW_OUTPUT_DIR FTX_BINARY");
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]), binary = path.resolve(process.argv[4]);
const read = name => JSON.parse(fs.readFileSync(path.join(root, name)));
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const sourceScript = process.env.FTX_START_EDITOR_SOURCE
  ? fs.readFileSync(path.resolve(process.env.FTX_START_EDITOR_SOURCE)) : null;
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
    let sourceLoads = 0;
    if (sourceScript) await context.route('**/start-editor.js', route => {
      ++sourceLoads;
      return route.fulfill({contentType: 'application/javascript', body: sourceScript});
    });
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

    // Preserve the first applied start before the complete native set exists.
    assert.equal(await page.locator('#startDraftDownload').count(), 1, 'one valid start needs a resumable draft');
    const partialEvent = page.waitForEvent('download');
    await page.locator('#startDraftDownload').click();
    const partialDownload = await partialEvent;
    assert.equal(partialDownload.suggestedFilename(), 'body-starts-draft.json');
    const partialPath = path.join(out, 'one-start-draft.json');
    await partialDownload.saveAs(partialPath);
    const partial = {...expected, starts: [expected.starts[0]]};
    assert.deepEqual(JSON.parse(fs.readFileSync(partialPath)), partial);
    const refused = spawnSync(binary, ['measure', 'fit-starts', partialPath, '--model', path.join(root, 'model.ftxbody'),
      '--out', path.join(out, 'partial-refused'), '--json'], {encoding: 'utf8', timeout: 120000});
    assert.ifError(refused.error); assert.equal(refused.status, 3, refused.stdout + refused.stderr);
    assert.match(JSON.parse(refused.stdout).refusal.message, /2\.\.4/);
    assert(!fs.existsSync(path.join(out, 'partial-refused')));
    await page.reload(); await page.waitForFunction(() => !document.getElementById('startEditor').hidden);
    await page.locator('#startDraftFile').setInputFiles(partialPath);
    await page.waitForFunction(() => document.getElementById('startStatus').textContent.includes('starts restored'));
    assert.equal(await page.locator('#startDraftFile').inputValue(), '');
    assert.equal(await appliedCount(page), 1); await hidden(page);
    for (const [index, value] of partial.starts[0].shape.entries()) assert.equal(Number(await control(page, 'shape', index).inputValue()), value);
    for (const pose of partial.starts[0].poses) for (const [index, value] of pose.values.entries()) {
      assert.equal(Number(await control(page, 'pose', index, pose.frame_index).inputValue()), value);
    }

    await page.locator("#duplicateStart").click();
    await page.locator("#applyStart").click();
    assert.equal(await appliedCount(page), 1);
    assert.match(await page.locator("#startStatus").innerText(), /distinct|duplicate/i);
    await hidden(page);
    await fill(page, expected.starts[1]);
    await page.locator("#applyStart").click();
    assert.equal(await appliedCount(page), 2);
    await visible(page);

    const load = async bytes => {
      await page.locator('#startDraftFile').setInputFiles({name: 'saved-starts.json', mimeType: 'application/json', buffer: Buffer.from(bytes)});
      await page.waitForFunction(() => !document.getElementById('startStatus').textContent.startsWith('Reading saved starts'));
    };
    await load(JSON.stringify(expected)); await hidden(page);
    await page.locator('#startList').selectOption('1'); await hidden(page);
    await page.locator('#applyStart').click(); await visible(page);
    const form = () => page.locator('#startParameters input').evaluateAll(nodes => nodes.map(node => node.value));
    const badFiles = [Buffer.from('{'), Buffer.from([0xc3, 0x28]), Buffer.alloc(256 * 1024 + 1, 32)];
    for (const edit of [
      value => { value.source_input += '-different'; }, value => { value.source_input_sha256 = '0'.repeat(64); },
      value => { value.model_sha256 = '0'.repeat(64); }, value => { value.capture_sources_sha256 = '0'.repeat(64); },
      value => { value.silhouettes = {path: 'other', sha256: '0'.repeat(64)}; },
      value => { value.extra = true; }, value => { value.schema = 'unsupported'; },
      value => { value.starts.push(...value.starts, value.starts[0]); },
      value => { value.starts[0].shape.pop(); }, value => { value.starts[0].shape[0] = report.model.shape[0].lower; },
      value => { value.starts[0].shape[0] = '0.5'; }, value => { value.starts[0].extra = true; },
      value => { value.starts[0].poses[0].frame_index = 9999; },
      value => { value.starts[0].poses[1].frame_index = value.starts[0].poses[0].frame_index; },
      value => { value.starts[0].poses[0].values.pop(); }, value => { value.starts[1] = value.starts[0]; }
    ]) { const value = structuredClone(expected); edit(value); badFiles.push(Buffer.from(JSON.stringify(value))); }
    for (const bytes of badFiles) {
      const before = await form(), oldUrl = await page.locator('#startDownload').getAttribute('href');
      const oldDraft = await page.locator('#startDraftDownload').getAttribute('href');
      await load(bytes); assert.match(await page.locator('#startStatus').innerText(), /Draft not loaded/);
      assert.deepEqual(await form(), before); assert.equal(await appliedCount(page), 2);
      await hidden(page); assert(await page.locator('#startDraftDownload').isHidden());
      await revoked(page, oldUrl); await revoked(page, oldDraft);
      await page.locator('#cancelStart').click(); await visible(page);
    }
    for (const action of ['focus', 'chooser', 'empty']) {
      const before = await form(), oldUrl = await page.locator('#startDownload').getAttribute('href');
      if (action === 'focus') await page.locator('#startDraftFile').focus();
      else if (action === 'chooser') {
        const chooser = page.waitForEvent('filechooser');
        await page.locator('#startDraftFile').click(); await chooser;
      } else await page.locator('#startDraftFile').setInputFiles([]);
      await hidden(page); assert(await page.locator('#startDraftDownload').isHidden());
      await revoked(page, oldUrl); assert.deepEqual(await form(), before);
      // Selecting an applied start must not silently restore the unresolved download.
      await page.locator('#startList').selectOption('1'); await hidden(page);
      assert(await page.locator('#startDraftDownload').isHidden());
      await page.locator('#cancelStart').click(); await visible(page);
    }
    await control(page, 'shape', 0).fill(''); const incomplete = await form();
    await load('{'); assert.deepEqual(await form(), incomplete); await hidden(page);
    assert(await page.locator('#startDraftDownload').isHidden());
    await page.locator('#cancelStart').click(); await visible(page);

    for (const pendingEdit of [false, true]) {
      await load(JSON.stringify(expected)); await page.locator('#applyStart').click();
      const oldUrl = await page.locator('#startDownload').getAttribute('href');
      if (pendingEdit) await control(page, 'shape', 0).fill('');
      const before = await form();
      await page.evaluate(() => {
        window.dispatchEvent(new PageTransitionEvent('pagehide', {persisted: true}));
        window.dispatchEvent(new PageTransitionEvent('pageshow', {persisted: true}));
      });
      await hidden(page); assert(await page.locator('#startDraftDownload').isHidden());
      await revoked(page, oldUrl); assert.deepEqual(await form(), before);
      assert(await page.locator('#cancelStart').isEnabled());
      assert.match(await page.locator('#startStatus').innerText(), /cancel.*page|page.*cancel/i);
      await page.locator('#cancelStart').click(); await visible(page);
      assert.equal(Number(await control(page, 'shape', 0).inputValue()), expected.starts[0].shape[0]);
    }

    await page.evaluate(() => {
      const original = File.prototype.arrayBuffer;
      globalThis.__holdStartRead = false; globalThis.__startRead = null;
      File.prototype.arrayBuffer = function () {
        if (!globalThis.__holdStartRead) return original.call(this);
        globalThis.__holdStartRead = false;
        return new Promise((resolve, reject) => { globalThis.__startRead = {resolve, reject}; });
      };
    });
    for (const reject of [false, true]) {
      await load(JSON.stringify(expected)); await page.locator('#applyStart').click();
      const before = await form(), oldUrl = await page.locator('#startDownload').getAttribute('href');
      await page.evaluate(() => { globalThis.__holdStartRead = true; globalThis.__startRead = null; });
      await page.locator('#startDraftFile').setInputFiles({name: 'delayed.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(partial))});
      await page.waitForFunction(() => globalThis.__startRead !== null);
      await hidden(page); assert(await page.locator('#startDraftDownload').isHidden()); await revoked(page, oldUrl);
      await page.evaluate(({reject, text}) => {
        if (reject) globalThis.__startRead.reject(new Error('current read failed'));
        else globalThis.__startRead.resolve(new TextEncoder().encode(text).buffer);
      }, {reject, text: JSON.stringify(partial)});
      await page.waitForFunction(() => !document.getElementById('startStatus').textContent.startsWith('Reading saved starts'));
      await hidden(page);
      if (reject) {
        assert.match(await page.locator('#startStatus').innerText(), /Draft not loaded: current read failed/);
        assert.deepEqual(await form(), before); assert.equal(await appliedCount(page), 2);
        assert(await page.locator('#startDraftDownload').isHidden());
        await page.locator('#cancelStart').click(); await visible(page);
      } else {
        assert.equal(await appliedCount(page), 1); assert(await page.locator('#startDraftDownload').isVisible());
      }
    }
    for (const reject of [false, true]) for (const action of ['edit', 'focus', 'select', 'new', 'duplicate', 'midpoint', 'apply', 'remove', 'cancel', 'reset', 'replace', 'pagehide']) {
      await load(JSON.stringify(expected)); await page.locator('#applyStart').click();
      await page.evaluate(() => { globalThis.__holdStartRead = true; globalThis.__startRead = null; });
      await page.locator('#startDraftFile').setInputFiles({name: 'delayed.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(partial))});
      await page.waitForFunction(() => globalThis.__startRead !== null);
      await hidden(page); assert(await page.locator('#startDraftDownload').isHidden());
      if (action === 'edit') await control(page, 'shape', 0).fill('');
      else if (action === 'focus') await page.locator('#startParameters label').first().click();
      else if (action === 'select') await page.locator('#startList').selectOption('1');
      else if (action === 'replace') await load(JSON.stringify(partial));
      else if (action === 'pagehide') await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide')));
      else await page.locator('#' + {new: 'newStart', duplicate: 'duplicateStart', midpoint: 'midpointStart', apply: 'applyStart',
        remove: 'removeStart', cancel: 'cancelStart', reset: 'resetStarts'}[action]).click();
      const before = await form(), count = await appliedCount(page), message = await page.locator('#startStatus').innerText();
      assert(!message.startsWith('Reading saved starts'));
      if (action === 'pagehide') assert(await page.locator('#cancelStart').isEnabled());
      const links = await page.locator('#startDownload,#startDraftDownload').evaluateAll(nodes => nodes.map(node => [node.hidden, node.getAttribute('href')]));
      await page.evaluate(({reject, text}) => {
        if (reject) globalThis.__startRead.reject(new Error('obsolete read error'));
        else globalThis.__startRead.resolve(new TextEncoder().encode(text).buffer);
      }, {reject, text: JSON.stringify(partial)});
      assert.deepEqual(await form(), before); assert.equal(await appliedCount(page), count);
      assert.equal(await page.locator('#startStatus').innerText(), message);
      assert.deepEqual(await page.locator('#startDownload,#startDraftDownload').evaluateAll(nodes => nodes.map(node => [node.hidden, node.getAttribute('href')])), links);
    }
    await load(JSON.stringify(expected)); await page.locator('#startList').selectOption('1');
    await page.locator('#applyStart').click(); await visible(page);

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
    if (sourceScript) assert(sourceLoads > 0, 'The source-script override must actually execute.');
    assert.deepEqual(errors, []);
    assert(requests.every(request => request.method === "GET" && request.url.startsWith("file:")));
    console.log(JSON.stringify({status: "PASS", authored_starts: expected.starts.length, fit: fitted.status,
      editor_script: {mode: sourceScript ? 'source override' : 'compiled review',
        sha256: sourceScript ? hash(sourceScript) : report.start_editor.script.sha256, source_loads: sourceLoads},
      exercised: ["empty bootstrap", "named bounded parameters", "midpoint", "copy", "distinctness", "partial edit", "boundary", "cancel", "remove", "maximum starts", "reset", "one-start save/reload/resume", "partial native refusal", "invalid file preservation", "file-choice URL revocation", "slow-read withdrawal", "late-read ownership", "cached-page recovery", "frozen native previews", "exact native frame identities", "download to ordinary fit"],
      download: destination, browser_errors: errors.length, local_requests: requests.length}));
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });

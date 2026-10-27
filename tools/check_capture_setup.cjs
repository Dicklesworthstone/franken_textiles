// Actual browser SETUP -> native constructor -> existing CLI target/observation/start pipeline.
// Later editors are not driven here. Prepare with FTX_CAPTURE_SETUP_DIR and the setup CLI test.
// Synthetic software acceptance only; no physical/anatomical/real-wearer fit claim.
const assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const {spawnSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');
const {chromium} = require(process.env.FTX_PLAYWRIGHT_MODULE || 'playwright');
if (process.argv.length !== 5) throw Error('Usage: node tools/check_capture_setup.cjs CASE_DIR NEW_OUTPUT_DIR FTX_BINARY');
const root = path.resolve(process.argv[2]), out = path.resolve(process.argv[3]), binary = path.resolve(process.argv[4]);
const read = file => JSON.parse(fs.readFileSync(file));
const fixture = name => read(path.join(root, name));
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const write = (name, value) => fs.writeFileSync(path.join(out, name), JSON.stringify(value, null, 2) + '\n');
const report = fixture('setup-review/review.json'), setup = fixture('setup-review/setup.json');
const seed = fixture('setup-review/capture-choices.json'), expected = fixture('expected-choices.json');
const truth = fixture('truth.json'), model = path.join(root, 'model.ftxbody');
assert.equal(report.status, 'CONFIGURATION_REQUIRED'); assert.equal(report.body_fitted, false);
assert.equal(seed.source_setup_sha256, hash(fs.readFileSync(seed.source_setup)));
assert.deepEqual(seed.frame_indices, []); assert.equal(seed.age_years, null);
for (const entry of [report.setup, report.annotations, report.editor.viewer, report.editor.script]) {
  const bytes = fs.readFileSync(path.join(root, 'setup-review', entry.file));
  assert.equal(bytes.length, entry.bytes); assert.equal(hash(bytes), entry.sha256);
}
// The later CLI fixture transport below uses JSON.parse; refuse wide timestamps rather than round them.
for (const frame of truth.fit.frames) assert(Number.isSafeInteger(frame.source.metadata.frame.timestamp_us));
fs.mkdirSync(out, {mode: 0o700});
function command(args, cwd = out) {
  const result = spawnSync(binary, ['measure', ...args, '--json'],
    {cwd, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, timeout: 120000});
  assert.ifError(result.error); assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  return JSON.parse(result.stdout).result;
}
const row = (page, group, name) => page.locator(`[data-group="${group}"][data-name=${JSON.stringify(name)}]`);
const field = (row, name) => row.locator(`[data-field="${name}"]`);
async function hidden(page) {
  assert(await page.locator('#setupDownload').isHidden());
  assert.equal(await page.locator('#setupDownload').getAttribute('href'), null);
}
async function visible(page) {
  assert(await page.locator('#setupDownload').isVisible());
  assert.match(await page.locator('#setupDownload').getAttribute('href'), /^blob:/);
}
async function revoked(page, url) { assert((await page.evaluate(() => globalThis.__revoked)).includes(url)); }
async function choice(control, value) {
  await field(control, 'mode').selectOption(value.mode);
  for (const key of value.mode === 'fixed' ? ['value'] : ['lower', 'upper']) await field(control, key).fill(String(value[key]));
}
async function fill(page) {
  for (const index of expected.frame_indices) await page.locator(`#setupFrames input[data-frame-index="${index}"]`).check();
  for (const [id, value] of [['setupAge', expected.age_years], ['setupGender', expected.gender],
    ['setupScale', expected.metric_scale_source], ['setupEvidence', expected.observation_evidence]]) await page.locator('#' + id).fill(String(value));
  for (const item of expected.shapes) {
    await page.locator('#setupShapeName').selectOption(item.name); await page.locator('#addSetupShape').click();
    const control = row(page, 'shape', item.name);
    assert((await control.locator('input').evaluateAll(nodes => nodes.map(node => node.value))).every(value => value === ''));
    await choice(control, item.choice);
  }
  for (const item of expected.roots) await choice(row(page, 'root', item.name), item.choice);
  for (const item of expected.joints) {
    await page.locator('#setupJointName').selectOption(item.bone); await page.locator('#addSetupJoint').click();
    const control = row(page, 'joint', item.bone);
    for (const [i, axis] of ['x', 'y', 'z'].entries()) {
      assert.equal(await field(control, 'axis-' + axis).inputValue(), '');
      await field(control, 'axis-' + axis).fill(String(item.axis[i]));
    }
    await choice(control, item.choice);
  }
  for (const item of expected.measurements) {
    await page.locator('#setupMeasurementName').selectOption(item.name); await page.locator('#addSetupMeasurement').click();
    const control = row(page, 'measurement', item.name);
    assert.equal(await field(control, 'partition').inputValue(), '');
    for (const key of ['value_m', 'sigma_m', 'evidence']) await field(control, key).fill(String(item[key]));
    await field(control, 'partition').selectOption(item.partition);
  }
}
async function saveDownload(page, name) {
  const event = page.waitForEvent('download'); await page.locator('#setupDownload').click();
  const download = await event; assert.equal(download.suggestedFilename(), 'capture-choices.json');
  const destination = path.join(out, name); await download.saveAs(destination); return read(destination);
}

(async () => {
  const browser = await chromium.launch({headless: true});
  const errors = [], requests = [];
  try {
    const context = await browser.newContext({viewport: {width: 1200, height: 900},
      userAgent: 'OpenAI File Downloader, XaiImageApiFetch/1.0'});
    context.on('request', request => requests.push({url: request.url(), method: request.method()}));
    await context.addInitScript(() => {
      globalThis.__revoked = [];
      const revoke = URL.revokeObjectURL.bind(URL);
      URL.revokeObjectURL = url => { globalThis.__revoked.push(url); return revoke(url); };
    });
    const page = await context.newPage();
    page.on('pageerror', error => errors.push(String(error)));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.goto(pathToFileURL(path.join(root, 'setup-review/index.html')).href);
    await page.waitForFunction(() => !document.getElementById('setupEditor').hidden);
    const data = await page.locator('#setupData').evaluate(node => JSON.parse(node.textContent));
    assert.deepEqual(data.annotations, seed);
    assert(!JSON.stringify(data).includes('"timestamp_us"'));
    assert.equal(await page.locator('script').count(), 2);
    assert.equal(await page.evaluate(() => globalThis.setupInjection), undefined);
    assert.match(await page.locator('body').innerText(), /rotation-vector components/);
    assert.equal(await page.locator('[data-group="root"]').count(), 6);
    for (const item of seed.roots) {
      assert.equal(await field(row(page, 'root', item.name), 'mode').inputValue(), 'fixed');
      assert.equal(await field(row(page, 'root', item.name), 'value').inputValue(), '0');
    }
    for (const [index, frame] of data.frames.entries()) {
      const card = page.locator('#setupFrames .frame-card').nth(index), image = card.locator('img');
      const original = setup.frames.find(item => item.frame_index === frame.frame_index);
      const bytes = fs.readFileSync(path.join(root, 'setup-review', frame.image.file));
      assert.deepEqual(bytes, fs.readFileSync(path.resolve(path.dirname(setup.manifest.path), original.source.image)));
      assert.equal(hash(bytes), frame.image.sha256); assert.equal(bytes.length, frame.image.bytes);
      assert((await card.innerText()).includes(frame.label)); // hostile markup stays literal when present
      assert.equal(await card.locator('input').isDisabled(), !frame.eligible);
      await image.scrollIntoViewIfNeeded();
      await image.evaluate(node => node.decode());
      assert.deepEqual(await image.evaluate(node => [node.naturalWidth, node.naturalHeight]), frame.image_size_px);
    }
    await hidden(page); await page.locator('#applySetup').click(); await hidden(page);
    assert.match(await page.locator('#setupStatus').innerText(), /incomplete|invalid/);
    await fill(page); await page.locator('#applySetup').click(); await visible(page);
    // Used catalogue names cannot be added twice, and clicking an empty chooser changes no rows.
    for (const [group, chooser, button, used] of [
      ['shape', 'setupShapeName', 'addSetupShape', expected.shapes.map(item => item.name)],
      ['joint', 'setupJointName', 'addSetupJoint', expected.joints.map(item => item.bone)],
      ['measurement', 'setupMeasurementName', 'addSetupMeasurement', expected.measurements.map(item => item.name)]]) {
      const options = await page.locator('#' + chooser + ' option').evaluateAll(nodes => nodes.map(node => node.value));
      assert(used.every(name => !options.includes(name)));
      if (await page.locator('#' + button).isEnabled()) await page.locator('#' + button).click();
      assert.equal(await page.locator(`[data-group="${group}"]`).count(), used.length);
    }
    let url = await page.locator('#setupDownload').getAttribute('href');
    await page.locator('#setupAge').fill(''); await hidden(page); await revoked(page, url);
    await page.locator('#applySetup').click(); await hidden(page);
    await page.locator('#cancelSetup').click(); await visible(page);
    assert.equal(Number(await page.locator('#setupAge').inputValue()), expected.age_years);
    const badEdits = [
      async () => page.locator(`#setupFrames input[data-frame-index="${expected.frame_indices[0]}"]`).uncheck(),
      async () => field(row(page, 'shape', expected.shapes[0].name), 'lower').fill(String(expected.shapes[0].choice.upper)),
      async () => field(row(page, 'root', expected.roots[0].name), 'mode').selectOption(''),
      async () => field(row(page, 'root', expected.roots[0].name), 'lower').fill('-1000001'),
      async () => { for (const name of ['root_rx_rad', 'root_ry_rad']) await choice(row(page, 'root', name), {mode: 'fixed', value: Math.PI}); },
      async () => { for (const axis of ['x', 'y', 'z']) await field(row(page, 'joint', expected.joints[0].bone), 'axis-' + axis).fill('0'); },
      async () => field(row(page, 'measurement', expected.measurements[0].name), 'partition').selectOption(''),
      async () => page.locator('#setupEvidence').fill('é'.repeat(2049))
    ];
    for (const edit of badEdits) {
      url = await page.locator('#setupDownload').getAttribute('href');
      await edit(); await hidden(page); await revoked(page, url);
      await page.locator('#applySetup').click(); await hidden(page);
      assert.match(await page.locator('#setupStatus').innerText(), /incomplete|invalid/);
      await page.locator('#cancelSetup').click(); await visible(page);
    }
    url = await page.locator('#setupDownload').getAttribute('href');
    await row(page, 'shape', expected.shapes[0].name).locator('[data-action="remove"]').click();
    await hidden(page); await revoked(page, url);
    assert((await page.locator('#setupShapeName option').evaluateAll(nodes => nodes.map(node => node.value))).includes(expected.shapes[0].name));
    await page.locator('#cancelSetup').click(); await visible(page);
    // Explicit fixed controls have their own serialized value, with no stale fitting bounds.
    await choice(row(page, 'shape', expected.shapes[0].name), {mode: 'fixed', value: 0.6});
    await choice(row(page, 'root', expected.roots[0].name), {mode: 'fixed', value: 0.125});
    await choice(row(page, 'joint', expected.joints[0].bone), {mode: 'fixed', value: -0.2});
    await page.locator('#applySetup').click(); await visible(page);
    const fixed = await saveDownload(page, 'fixed-choices.json');
    assert.deepEqual(fixed.shapes[0].choice, {mode: 'fixed', value: 0.6});
    assert.deepEqual(fixed.roots[0].choice, {mode: 'fixed', value: 0.125});
    assert.deepEqual(fixed.joints[0].choice, {mode: 'fixed', value: -0.2});
    url = await page.locator('#setupDownload').getAttribute('href');
    await page.locator('#resetSetup').click(); await hidden(page); await revoked(page, url);
    assert.equal(await page.locator('#setupFrames input:checked').count(), 0);
    assert.equal(await page.locator('#setupAge').inputValue(), '');
    assert.equal(await page.locator('[data-group="shape"],[data-group="joint"],[data-group="measurement"]').count(), 0);
    await fill(page); await page.locator('#applySetup').click(); await visible(page);
    assert.deepEqual(await saveDownload(page, 'capture-choices.json'), expected);
    assert.deepEqual(fixture('setup-review/capture-choices.json'), seed);

    // Only setup was authored in the browser. The following is the existing native pipeline,
    // with independent synthetic truth supplied explicitly for targets, observations and starts.
    command(['capture-init', 'capture-choices.json', '--model', model, '--out', 'constructed']);
    assert.deepEqual(fs.readFileSync(path.join(out, 'constructed/input.json')), fs.readFileSync(path.join(root, 'constructed/input.json')));
    command(['body-target-review', 'constructed/input.json', '--model', model, '--out', 'target-review']);
    const targets = read(path.join(out, 'target-review/body-targets.json'));
    assert.deepEqual(targets.keypoints, {}); targets.keypoints = truth.model.keypoints; write('targets.json', targets);
    command(['body-targets', 'targets.json', '--model', model, '--out', 'target-authored']);
    const cameras = command(['capture-review', 'target-authored/input.json', '--out', 'capture-review']);
    for (const frame of cameras.capture_input.frames) {
      assert.equal(frame.sheet_acquisition.detection.detector.backend, 'rust_charuco');
      assert.equal(frame.sheet_pose.ok, true); assert.equal(frame.sheet_pose.held_out_points, 4);
    }
    const observed = read(path.join(out, 'capture-review/capture.json'));
    const identity = frame => JSON.stringify(frame.source.metadata.frame);
    for (const frame of observed.fit.frames) frame.keypoints = truth.fit.frames.find(item => identity(item) === identity(frame)).keypoints;
    write('observations.json', observed);
    command(['fit-start-review', 'observations.json', '--model', model, '--out', 'start-review']);
    const starts = read(path.join(out, 'start-review/body-starts.json'));
    assert.deepEqual(starts.starts, []);
    starts.starts = truth.fit.starts.map(start => ({shape: start.shape, poses: observed.fit.frames.map((frame, frame_index) =>
      ({frame_index, values: start.poses.find(pose => JSON.stringify(pose.frame) === identity(frame)).values}))}));
    write('starts.json', starts);
    command(['fit-starts', 'starts.json', '--model', model, '--out', 'start-authored']);
    const finalPath = path.join(out, 'start-authored/input.json'), referencePath = path.join(root, 'start-authored/input.json');
    const finalBytes = fs.readFileSync(finalPath), referenceBytes = fs.readFileSync(referencePath);
    // The explicit later CLI fixture transport normalizes JSON numbers such as 1.0 to 1.
    // All final values must match; the initial browser-setup construction above stays byte-exact.
    const finalInput = JSON.parse(finalBytes), referenceInput = JSON.parse(referenceBytes);
    assert.deepEqual(finalInput, referenceInput);
    const fitted = command(['fit-body', finalPath, '--model', model, '--out', 'fit.json']);
    const reference = fixture('start-authored/expected-fit.json');
    assert.equal(fitted.status, 'ACCEPTED'); assert.equal(reference.status, 'ACCEPTED');
    assert.equal(fitted.input_sha256, hash(finalBytes)); assert.equal(reference.input_sha256, hash(referenceBytes));
    assert.deepEqual(fitted.capture_input.input, finalInput); assert.deepEqual(reference.capture_input.input, referenceInput);
    assert.equal(fitted.capture_input.source, fs.realpathSync(finalPath)); assert.equal(reference.capture_input.source, fs.realpathSync(referencePath));
    reference.input_sha256 = fitted.input_sha256;
    reference.capture_input.source = fitted.capture_input.source;
    assert.deepEqual(fitted, reference);
    assert.deepEqual(errors, []); assert(requests.every(request => request.method === 'GET' && request.url.startsWith('file:')));
    console.log(JSON.stringify({status: 'PASS', browser_stage: 'capture setup only', subsequent_stage: 'existing native CLI pipeline with independent synthetic truth',
      fit: fitted.status, initial_construction: 'byte-exact', later_cli_replay: 'semantic equality with independently checked input-byte digests and paths',
      chosen_frames: expected.frame_indices.length, browser_errors: errors.length, local_requests: requests.length,
      exercised: ['source photographs', 'explicit defaults', 'blank choices', 'frame count', 'bounds', 'root choices and rotation-vector limit', 'joint axes', 'tape role', 'UTF-8 evidence budget',
        'duplicate prevention', 'remove', 'fixed controls', 'stale URL withdrawal', 'cancel', 'reset', 'byte-exact native construction', 'complete fit report equality']}));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });

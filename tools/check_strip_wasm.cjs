// Actual compiled browser WASM and the production material worker, compared with native commands.
// Build web/pkg (or set FTX_WASM_PKG to another generated package) and ftx-materials first, then:
// node --experimental-vm-modules tools/check_strip_wasm.cjs --native target/debug/ftx-materials
// This checks runtime equality and retained artifacts, not browser navigation or empirical fit.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const vm = require('node:vm');
const {pathToFileURL} = require('node:url');
const {execFileSync} = require('node:child_process');
const {createHash} = require('node:crypto');

const root = path.resolve(__dirname, '..');
const arg = process.argv.indexOf('--native');
if (arg >= 0 && !process.argv[arg + 1]) throw Error('--native requires the built ftx-materials path');
const native = path.resolve(arg < 0 ? path.join(root, 'target/debug/ftx-materials') : process.argv[arg + 1]);
const example = name => path.join(root, 'examples/strip-identification', name);
const acquisition = path.join(root, 'crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json');
const sha256 = value => createHash('sha256').update(value).digest('hex');

(async () => {
  const pkg = process.env.FTX_WASM_PKG ? path.resolve(process.env.FTX_WASM_PKG) : path.join(root, 'web/pkg');
  const ftx = await import(pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
  ftx.initSync({module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm'))});
  for (const name of ['material_strip_acquisition', 'material_strip_experiment', 'material_calibrate_strip']) {
    assert.equal(typeof ftx[name], 'function', `Rebuild web/pkg: missing ${name}`);
  }
  const sent = [];
  const context = vm.createContext({Uint8Array, TextEncoder, WebAssembly, JSON, Error, self: {}, postMessage: message => sent.push(message)});
  const imports = new vm.SyntheticModule(Object.keys(ftx), function () {
    for (const [name, value] of Object.entries(ftx)) this.setExport(name, value);
  }, {context});
  const worker = new vm.SourceTextModule(fs.readFileSync(path.join(root, 'web/materials-worker.js'), 'utf8'), {context});
  await worker.link(() => imports);
  await worker.evaluate();
  let sequence = 0;
  const send = async (type, raw) => {
    const request = ++sequence;
    await context.self.onmessage({data: {type, request, stripRequest: raw}});
    const result = sent.at(-1);
    assert.equal(result.request, request);
    assert.equal(result.type, 'result', JSON.stringify(result));
    return result;
  };
  const invoke = (action, file, allowEstimates) => execFileSync(native,
    [action, file, ...(action === 'calibrate-strip' && allowEstimates ? ['--allow-estimates'] : [])],
    {encoding: 'utf8', maxBuffer: 64 * 1024 * 1024});
  const artifact = (message, name) => {
    const found = message.files.filter(file => file.name === name);
    assert.equal(found.length, 1, `Expected exactly one ${name}`);
    return found[0].text;
  };
  const compare = async (action, operation, api, stem, input) => {
    const raw = fs.readFileSync(input, 'utf8');
    const originalHash = sha256(raw);
    const inverse = action === 'calibrate-strip';
    const allowEstimates = inverse && JSON.parse(raw).allow_estimates === true;
    const nativeReport = JSON.parse(invoke(action, input, allowEstimates));
    const wasmText = inverse ? ftx[api](raw, allowEstimates) : ftx[api](raw);
    const wasmReport = JSON.parse(wasmText);
    // Native and browser envelopes retain their existing readiness/detail contracts. Their
    // complete physical report and lossless reusable selections must agree exactly.
    if (inverse) {
      for (const field of ['calibration', 'selection', 'selection_json', 'fit_claim', 'empirical_validation_claim']) {
        assert.deepEqual(wasmReport[field], nativeReport[field], `${action}: native/WASM ${field} mismatch`);
      }
    } else assert.deepEqual(wasmReport, nativeReport, `${action}: actual native/WASM report mismatch`);
    const message = await send(operation, raw);
    assert.deepEqual(message.result, wasmReport, `${action}: actual worker changed the report`);
    assert.equal(artifact(message, `${stem}-report.json`), wasmText);
    assert.equal(artifact(message, `${stem}-input.json`), raw);
    assert.equal(sha256(fs.readFileSync(input)), originalHash);
    return {nativeReport, wasmReport, wasmText, message};
  };

  const original = await compare('strip-acquisition', 'stripAcquisition', 'material_strip_acquisition', 'strip-acquisition', acquisition);
  assert.equal(original.nativeReport.acquisition_sha256, sha256(fs.readFileSync(acquisition)));
  assert.equal(original.nativeReport.curves.length, 9);
  assert.equal(original.nativeReport.curves[0].observations.length, 916);
  assert.equal(original.nativeReport.curves[0].observations.at(-1).branch, 'unloading');
  assert.equal(original.nativeReport.transverse_curves.length, 9);
  assert.equal(original.nativeReport.transverse_readme.sha256, sha256(original.nativeReport.transverse_readme.text));
  for (const field of ['material', 'selection', 'selection_json']) assert.equal(field in original.nativeReport, false);
  for (const curve of original.nativeReport.transverse_curves) {
    assert.match(curve.measurement_scope, /not reported|unverified/);
    for (const point of curve.observations) {
      assert.equal('width_m' in point, false);
      assert.equal('local_strain' in point, false);
    }
  }

  const fitted = await compare('calibrate-strip', 'stripFit', 'material_calibrate_strip', 'strip-calibration', example('synthetic-calibration.json'));
  assert.equal(fitted.nativeReport.calibration.accepted, true, JSON.stringify(fitted.nativeReport.calibration.reasons));
  assert.equal(fitted.nativeReport.calibration.domain_replay_verified, true);
  assert.equal(fitted.nativeReport.selection.allow_estimates, true);
  assert.equal(artifact(fitted.message, 'strip-calibrated-selection.json'), fitted.nativeReport.selection_json);
  assert.equal(fitted.wasmReport.material.selection_json, fitted.nativeReport.selection_json);
  assert.equal(fitted.wasmReport.material.capabilities.mechanical_coefficients.ready, false);
  assert.equal(Object.keys(fitted.nativeReport.calibration.material.properties).length, 4);
  assert.ok(Object.values(fitted.nativeReport.calibration.material.properties).every(property => property.evidence === 'estimate'));
  assert.deepEqual(JSON.parse(ftx.material_inspect(fitted.nativeReport.selection_json)), fitted.wasmReport.material);
  const genericRequest = ++sequence;
  const fitInput = fs.readFileSync(example('synthetic-calibration.json'), 'utf8');
  await context.self.onmessage({data: {type: 'calibrate', request: genericRequest,
    calibrationMode: 'clamped_strip', calibrationRequest: fitInput, allowEstimates: true}});
  const generic = sent.at(-1);
  assert.equal(generic.request, genericRequest);
  assert.equal(generic.type, 'result', JSON.stringify(generic));
  assert.deepEqual(generic.result, fitted.wasmReport);
  assert.equal(artifact(generic, 'material-calibration.json'), fitted.wasmText);
  assert.equal(artifact(generic, 'calibrated-selection.json'), fitted.nativeReport.selection_json);

  const simulated = await compare('strip-experiment', 'stripExperiment', 'material_strip_experiment', 'strip-experiment', example('synthetic-forward.json'));
  assert.equal(simulated.nativeReport.input.material.expected_sha256, fitted.nativeReport.selection.expected_sha256);
  assert.equal(simulated.nativeReport.states.length, simulated.nativeReport.input.extensions.length);
  assert.ok(simulated.nativeReport.states.some(state => state.force_n > 0));
  for (const state of simulated.nativeReport.states) {
    assert.ok(state.free_force_residual_n <= simulated.nativeReport.input.controls.force_tolerance_n);
    assert.ok(state.grip_balance_residual_n <= simulated.nativeReport.input.controls.force_tolerance_n);
  }

  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'ftx-strip-wasm-'));
  const altered = JSON.parse(fs.readFileSync(example('synthetic-calibration.json'), 'utf8'));
  const check = altered.experiments.find(experiment => experiment.partition === 'check');
  assert.ok(check, 'The synthetic fixture must retain an independently held-out acquisition');
  check.observations[0].force_n += 10;
  const rejectedPath = path.join(work, 'rejected-check.json');
  fs.writeFileSync(rejectedPath, JSON.stringify(altered), {flag: 'wx'});
  const rejected = await compare('calibrate-strip', 'stripFit', 'material_calibrate_strip', 'strip-calibration', rejectedPath);
  assert.equal(rejected.nativeReport.calibration.accepted, false);
  assert.deepEqual(rejected.nativeReport.calibration.parameters, fitted.nativeReport.calibration.parameters);
  assert.ok(rejected.nativeReport.calibration.reasons.some(reason => reason.includes('CHECK')));
  assert.ok(rejected.nativeReport.calibration.check.peak_scaled > 90);
  for (const field of ['material', 'selection', 'selection_json']) {
    assert.equal(rejected.nativeReport.calibration[field], null);
    assert.equal(rejected.wasmReport[field], null);
  }
  assert.equal(rejected.message.files.some(file => file.name === 'strip-calibrated-selection.json'), false);

  // Inputs own their evidence permissions even when the actual worker executes the identification.
  altered.allow_estimated_observations = false;
  const request = ++sequence;
  await context.self.onmessage({data: {type: 'stripFit', request, stripRequest: JSON.stringify(altered)}});
  const refusal = sent.at(-1);
  assert.equal(refusal.request, request);
  assert.equal(refusal.type, 'error');
  assert.equal('result' in refusal, false);
  assert.equal('files' in refusal, false);

  console.log('PASS: compiled WASM + actual worker/native equality for complete source acquisitions, accepted inverse identification, exact frozen selections, production forward replay and rejected held-out CHECK reports.');
  console.log('Original source channels and explicit estimate permissions are preserved. Browser navigation and empirical material or garment fit are not asserted.');
  console.log('Synthetic rejection input retained at:', rejectedPath);
})().catch(error => { console.error(error.stack || String(error)); process.exitCode = 1; });

// Actual material page/worker modules with explicit DOM/WASM doubles: lifecycle tests, not physics.
// Run directly or through check_trace_frontend.cjs (the existing hosted frontend command).
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = name => fs.readFileSync(path.join(__dirname, '../web', name), 'utf8');
const plain = value => JSON.parse(JSON.stringify(value));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const calibrationSelection = admission => `{"id":"identified","expected_sha256":"locked","allow_estimates":${admission},"value":-0.0,"coefficient":1.0}`;
function calibrationReport(mode = 'nonlinear', admission = false, check = true) {
  if (mode === 'membrane_plasticity') return permanentCalibration('ACCEPTED', admission, check);
  const selection = calibrationSelection(admission), quality = { components: 12, rms_scaled: 0.02, peak_scaled: 0.04 };
  return { selection: JSON.parse(selection), selection_json: selection, fit_claim: false,
    material: { selection: JSON.parse(selection), selection_json: selection, snapshot: { id: 'identified' },
      capabilities: { mechanical_coefficients: admission ? {ready:true} : {ready:false,refusal:{message:'Fitted estimates need admission'}} } },
    calibration: { schema: mode === 'nonlinear' ? 'ftx-nonlinear-swatch-calibration-report/1' : 'ftx-relaxation-calibration-report/1',
      input: { orthotropic: mode === 'relaxation' ? {times_seconds:[0.3,2,8,32]} : undefined,
        strain_range: [[-0.1,0.2],[-0.2,0.3],[-0.4,0.4]], max_rms_scaled:0.05, max_peak_scaled:0.1 },
      fitted_strain_range: [[-0.1,0.2],[-0.2,0.3],[-0.4,0.4]],
      material: { membrane_relaxation: { orthotropic_branches: [{id:'warp'},{id:'shear'},{id:'weft'}] } },
      orthotropic_fit: mode === 'relaxation' ? {candidates:[
        {time_seconds:0.3,stiffness_n_m:[240,60,-30,0],active:true},
        {time_seconds:2,stiffness_n_m:[0,0,0,90],active:true},
        {time_seconds:8,stiffness_n_m:[0,120,0,0],active:true},
        {time_seconds:32,stiffness_n_m:[0,0,0,0],active:false}]} : undefined,
      fit: quality, check: check ? quality : null, input_sha256:'source-lock', missing_mechanics:[] } };
}
function recoveryComparison(status = 'AMBIGUOUS') {
  const selected = calibrationReport('relaxation', true);
  const comparison = { schema:'ftx-relaxation-model-comparison-report/1', status,
    objective_evaluations:2, projected_iterations:0,
    fit_decision:{equivalent_candidate_ids:['complete','extra'],ambiguous:true,
      parsimonious_candidate_id:'complete',chosen_candidate_id:status === 'AMBIGUOUS' ? null : 'complete'},
    candidates:['complete','extra'].map((id, i) => ({id,declared_parameters:12+4*i,
      fit_status:'ACCEPTED',calibration:selected.calibration,check_status:'ACCEPTED'})) };
  return status === 'ACCEPTED' ? {...selected,comparison} :
    {comparison,calibration:null,material:null,selection:null,selection_json:null,fit_claim:false};
}

function permanentCalibration(status = 'ACCEPTED', admission = false, check = true) {
  const selected = calibrationReport('nonlinear', admission, check), quality = selected.calibration.fit;
  const properties = Object.fromEntries(['warp','weft','shear'].flatMap(axis => [
    [`membrane_strain_limit_${axis}`,{value:0.25,unit:'1'}],
    [`membrane_accumulation_limit_${axis}`,{value:1,unit:'1'}]]));
  selected.calibration = {schema:'ftx-membrane-plasticity-calibration-report/1',status,
    input:{material:{properties},yield_bounds_n_m:[[1,20],[1,20],[1,10]],
      hardening_bounds_n_m:[[0,200],[0,100],[0,50]],max_rms_scaled:0.05,max_peak_scaled:0.1},
    parameters:{yield_n_m:[10,8,5],hardening_n_m:[100,50,25]},fit:quality,check:check?quality:null,
    check_status:check?(status==='REJECTED'?'REJECTED':'ACCEPTED'):'NOT_PROVIDED',check_refusal:null,
    decision_reasons:status==='ACCEPTED'?[]:['FIT coefficients are not independently resolved.'],
    input_sha256:'exact-cycle-source',predictions:[{plastic_strain:[0.03,0,0],stored_j_m2:2,plastic_dissipation_j_m2:1}],
    diagnostics:{starts_completed:4,starts_attempted:4,evaluations:400,evaluation_limit:4096,
      history_steps:3000,history_step_limit:2000000,trials_refused:1,excitation_pivot:0.2,sensitivity_evaluated:true}};
  if (status !== 'ACCEPTED') Object.assign(selected,{selection:null,selection_json:null,material:null});
  return selected;
}

async function workerModule({ failFold = false, failExport = false, trap = false, rejectLoad = false,
  failScenario = false, trapScenario = false, bindings = null } = {}) {
  const sent = [], called = [], freed = [];
  const imports = bindings || {
    default: () => rejectLoad ? Promise.reject(Error('bundle unavailable')) : Promise.resolve(),
    material_catalog: q => JSON.stringify({ materials: [], query: q }),
    material_inspect: s => JSON.stringify({ selection: JSON.parse(s) }),
    material_pressure_condition: r => {
      called.push(['pressure', r]);
      const selection = '{"id":"compressed","allow_estimates":true,"value":-0.0}';
      return `{"selection":${selection},"selection_json":${JSON.stringify(selection)},"input":{"pressure_pa":250.0},"interpolated":true}`;
    },
    material_calibrate_nonlinear: (r, a) => { called.push(['calibrate-nonlinear',r,a]); return JSON.stringify(calibrationReport('nonlinear',a)); },
    material_calibrate_relaxation: (r, a) => { called.push(['calibrate-relaxation',r,a]); return JSON.stringify(calibrationReport('relaxation',a)); },
    material_calibrate_membrane_plasticity: (r, a) => { called.push(['calibrate-membrane_plasticity',r,a]); return JSON.stringify(calibrationReport('membrane_plasticity',a)); },
    material_screen: r => { called.push(['screen', r]); return '{"alternatives":[],"coefficient":1.0}'; },
    material_study: (p, r) => { called.push(['study', p, r]); return '{"alternatives":[],"coordinate":-0.0}'; },
    material_scenario_study: (b, p, r, s, m) => {
      called.push(['scenario', b, p, r, s, m]);
      if (trapScenario) throw new WebAssembly.RuntimeError('scenario trap');
      if (failScenario) throw Error('scenario refused');
      return '{"manufacturing":{"alternatives":[]},"simulations":[{"key":"A","evaluation":{"status":"refused","refusal":{"message":"missing mechanics"}}}],"coordinate":-0.0}';
    },
    material_bind: (p, c) => { called.push(['bind', p, c]); return JSON.stringify({ pattern: JSON.parse(p), pattern_json:p }); },
    WebBody: class { constructor() { called.push(['body']); } free() { freed.push('body'); } },
    fold_up: (_b, p, m, f, contact) => {
      called.push(['fold', p, m, f, contact]);
      if (trap) throw new WebAssembly.RuntimeError('synthetic trap');
      return { report: () => { if (failFold) throw Error('report refused'); return '{"material":"new","coordinate":-0.0,"coefficient":1.0}'; },
        viewer: () => '<!doctype html>', free: () => freed.push('fold') };
    },
    thermal_steady_surface: (r, c) => { called.push(['heat', r, c]); return '{}'; },
    thermal_steady_coverage: r => { called.push(['coverage', JSON.parse(r), r]); return '{}'; },
    cut_plan: (p, o) => { called.push(['cut', p, o]); return '{"rolls":[{},{}]}'; },
    cut_svg: (p, o, i) => { called.push(['svg', p, o, i]); return '<svg/>'; },
    stock_plan: (p, r) => { called.push(['stock', p, r]); return '{"markers":[{"placements":[]},{"placements":[{}]}],"stock_value":1.0}'; },
    stock_residual: (p, r) => { called.push(['stock-residual', p, r]); return '{"schema":"ftx-finite-stock-request/1","mass":1.0}'; },
    stock_svg: (p, r, i) => { called.push(['stock-svg', p, r, i]); return '<svg/>'; },
    cut_price: (...args) => { called.push(['price', ...args]); return '{"complete_offer_coverage":false}'; },
    sew_stitches: (p, o) => { called.push(['sew', p, o]); return '{"stitches":[]}'; },
    run: (r, d, t) => {
      called.push(['exports', JSON.parse(r), d, t]);
      return { page: () => failExport ? '{"status":"BLOCKED","errors":["invalid"]}' : '{"status":"REVIEW"}',
        zip: () => new Uint8Array([1, 2]), zip_name: () => 'round.zip', free: () => freed.push('round') };
    }
  };
  const context = vm.createContext({ Uint8Array, TextEncoder, WebAssembly, JSON, Error, self: {}, postMessage: m => sent.push(m) });
  const stub = new vm.SyntheticModule(Object.keys(imports), function () {
    for (const [name, value] of Object.entries(imports)) this.setExport(name, value);
  }, { context });
  const module = new vm.SourceTextModule(read('materials-worker.js'), { context });
  await module.link(() => stub); await module.evaluate();
  const send = async data => {
    // Real wasm-bindgen imports execute in Node's module realm, while this worker is a VM realm.
    // Forward its worker-global progress import to the actual worker callback for this invocation.
    const previous = globalThis.ftxProgress;
    if (bindings) globalThis.ftxProgress = line => context.self.ftxProgress(line);
    try { await context.self.onmessage({ data }); return sent.at(-1); }
    finally {
      if (bindings) {
        if (previous === undefined) delete globalThis.ftxProgress;
        else globalThis.ftxProgress = previous;
      }
    }
  };
  return { send, sent, called, freed };
}

test('material worker calls the shared consumers with exact current inputs and complete outputs', async () => {
  const w = await workerModule();
  const base = { request: 1, pattern: '{"pieces":[]}', options: '{"copies":2}', report: '{}', conditions: '{}', map: '{}', quotes: '{}', market: '{}' };
  for (const type of ['bind', 'heat', 'coverage', 'cut', 'price', 'sew']) {
    const r = await w.send({ ...base, type, changes: '{}' });
    assert.equal(r.type, 'result'); assert.equal(r.request, 1); assert.ok(r.files.length);
  }
  assert.deepEqual(plain(w.called.find(c => c[0] === 'coverage')[1]), {
    schema: 'ftx-steady-coverage-request/1', report: {}, map: {}, conditions: {}
  });
  assert.deepEqual(w.called.find(c => c[0] === 'price'), ['price', base.pattern, base.options, '{}', '{}']);
  assert.equal(w.called.filter(c => c[0] === 'svg').length, 1); // bounded first-roll preview
  const r = await w.send({ ...base, type: 'exports', fit: '{}', measurements: '{}', today: '2026-10-15', created: '2026-10-15T12:00' });
  assert.equal(r.files[0].name, 'round.zip'); assert.deepEqual([...r.files[0].bytes], [1, 2]);
  assert.deepEqual(w.freed, ['round']);
});
test('material worker frees model and fold on success or report failure, without caching an old drape', async () => {
  for (const failFold of [false, true]) {
    const w = await workerModule({ failFold });
    const r = await w.send({ type: 'fold', request: 4, bytes: new ArrayBuffer(8), pattern: '{}', measurements: '{}', fit: '{}', selfContactMm: 2 });
    assert.equal(r.type, failFold ? 'error' : 'result');
    assert.deepEqual(w.freed, ['fold', 'body']);
    assert.equal((await w.send({ type: 'coverage', report: 'invalid', map: '{}', conditions: '{}' })).type, 'error');
    assert.equal(w.sent.filter(m => m.type === 'result').length, failFold ? 0 : 1);
  }
});
test('material worker rejects blocked exports, initialization failure and further work after a WASM trap', async () => {
  const e = await workerModule({ failExport: true });
  assert.equal((await e.send({ type: 'exports', pattern: '{}', fit: '{}', measurements: '{}' })).type, 'error');
  assert.deepEqual(e.freed, ['round']);
  const w = await workerModule({ trap: true });
  assert.equal((await w.send({ type: 'fold', bytes: new ArrayBuffer(8) })).fatal, true);
  assert.deepEqual(w.freed, ['body']);
  assert.equal((await w.send({ type: 'catalog', query: '' })).type, 'error');
  const bad = await workerModule({ rejectLoad: true });
  await bad.send({ type: 'catalog', query: '' });
  assert.ok(bad.sent.some(m => m.type === 'loadError')); assert.ok(!bad.sent.some(m => m.type === 'result'));
});
test('worker retains numerical JSON identities across binding, drape files and nested thermal requests', async () => {
  const w=await workerModule(), pattern='{"pieces":[],"coordinate":-0.0,"coefficient":1.0}';
  const bind=await w.send({type:'bind',pattern,changes:'{}'});
  assert.equal(bind.files[0].text,pattern);
  assert.equal(bind.files[2].name,'material-changes.json');
  assert.equal(bind.files[2].text,'{}');
  assert.equal(bind.files[3].text,pattern);
  const fold=await w.send({type:'fold',bytes:new ArrayBuffer(8)});
  assert.equal(fold.reportText,fold.files[0].text);assert.match(fold.reportText,/-0\.0/);
  await w.send({type:'coverage',report:fold.reportText,map:'{}',conditions:'{}'});
  assert.ok(w.called.find(c=>c[0]==='coverage')[2].includes(fold.reportText));
});

test('finite stock uses unmodified source text and previews the first allocated physical item', async () => {
  const w = await workerModule(), pattern = '{"coordinate":-0.0}', stockRequest = '{"mass":1.0}';
  const r = await w.send({ type: 'stock', pattern, stockRequest, request: 11 });
  assert.equal(r.type, 'result'); assert.equal(r.files[1].name, 'residual-stock-request.json'); assert.equal(r.files[2].name, 'stock-1.svg');
  assert.deepEqual(w.called.find(c => c[0] === 'stock'), ['stock', pattern, stockRequest]);
  assert.deepEqual(w.called.find(c => c[0] === 'stock-residual'), ['stock-residual', pattern, stockRequest]);
  assert.deepEqual(w.called.find(c => c[0] === 'stock-svg'), ['stock-svg', pattern, stockRequest, 1]);
  assert.match(r.files[0].text, /1\.0/); assert.match(r.files[1].text, /1\.0/);
});

test('screening and studies call production entry points and download unmodified source/request text', async () => {
  const w = await workerModule(), pattern = '{"x":-0.0}', request = '{"coefficient":1.0}';
  const screen = await w.send({ type: 'screen', request: 12, screenRequest: request });
  assert.equal(screen.type, 'result'); assert.equal(screen.request, 12);
  assert.deepEqual(w.called.at(-1), ['screen', request]);
  assert.equal(screen.files[1].text, request); assert.match(screen.files[0].text, /1\.0/);
  const study = await w.send({ type: 'study', pattern, studyRequest: request });
  assert.equal(study.type, 'result'); assert.deepEqual(w.called.at(-1), ['study', pattern, request]);
  assert.equal(study.files[1].text, pattern); assert.equal(study.files[2].text, request);
  assert.match(study.files[0].text, /-0\.0/);
});

test('pressure conditioning preserves exact request and frozen selection through the shared worker', async () => {
  const w = await workerModule(), request = '{"pressure_pa":250.0,"coordinate":-0.0}';
  const result = await w.send({ type: 'pressure', request: 24, compressionRequest: request });
  assert.equal(result.type, 'result');
  assert.deepEqual(w.called.at(-1), ['pressure', request]);
  assert.equal(result.files[2].name, 'compression-request.json');
  assert.equal(result.files[2].text, request);
  assert.equal(result.files[1].text, result.result.selection_json);
  assert.match(result.files[1].text, /-0\.0/);
  assert.equal(result.result.inspection.selection.id, 'compressed');
});

test('calibration worker routes all shared engines with explicit consent and lossless reusable selections', async () => {
  const w = await workerModule(), input = '{"coefficient":1.0,"origin":-0.0}';
  for (const mode of ['nonlinear','relaxation','membrane_plasticity']) for (const allowEstimates of [false,true]) {
    const result = await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest:input,allowEstimates,request:19});
    assert.equal(result.type,'result'); assert.equal(result.request,19);
    assert.deepEqual(w.called.at(-1),['calibrate-'+mode,input,allowEstimates]);
    assert.equal(result.result.material.capabilities.mechanical_coefficients.ready,allowEstimates);
    assert.equal(result.files[0].text,JSON.stringify(calibrationReport(mode,allowEstimates)));
    assert.equal(result.files[1].text,calibrationSelection(allowEstimates));
    assert.equal(result.files[2].text,input);
  }
  for (const bad of [{calibrationMode:'arbitrary',allowEstimates:false},{calibrationMode:'nonlinear',allowEstimates:'true'}]) {
    const count=w.called.length;
    assert.equal((await w.send({type:'calibrate',calibrationRequest:input,...bad})).type,'error');
    assert.equal(w.called.length,count);
  }
  const incomplete=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_nonlinear:()=>'{"selection":{"id":"old"}}'}});
  assert.match((await incomplete.send({type:'calibrate',calibrationMode:'nonlinear',allowEstimates:false,calibrationRequest:input})).message,/selection and capability bridge/);
});

test('unresolved recovery comparisons preserve reports and source without publishing a selection artifact', async () => {
  const input='{"schema":"ftx-relaxation-model-comparison/1","origin":-0.0}';
  for (const status of ['AMBIGUOUS','CHOSEN_CANDIDATE_REJECTED','BUDGET_STOP','ACCEPTED']) {
    const report=recoveryComparison(status), raw=JSON.stringify(report);
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_relaxation:()=>raw}});
    const result=await w.send({type:'calibrate',calibrationMode:'relaxation',calibrationRequest:input,allowEstimates:true});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,raw); assert.equal(result.files.at(-1).text,input);
    assert.equal(result.files.some(f=>f.name === 'calibrated-selection.json'),status === 'ACCEPTED');
  }
  const invalid={...recoveryComparison(),selection_json:calibrationSelection(true)};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_relaxation:()=>JSON.stringify(invalid)}});
  const result=await w.send({type:'calibrate',calibrationMode:'relaxation',calibrationRequest:input,allowEstimates:true});
  assert.equal(result.type,'error'); assert.match(result.message,/cannot publish a selection/);
});

test('permanent-stretch worker keeps rejected and ambiguous histories without selectable output', async () => {
  const input='{"schema":"ftx-membrane-plasticity-calibration/1","origin":-0.0}';
  for (const status of ['ACCEPTED','AMBIGUOUS','BUDGET_EXHAUSTED','REJECTED']) {
    const report=permanentCalibration(status,true), raw=JSON.stringify(report);
    const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_membrane_plasticity:()=>raw}});
    const result=await w.send({type:'calibrate',calibrationMode:'membrane_plasticity',calibrationRequest:input,allowEstimates:true});
    assert.equal(result.type,'result',result.message);
    assert.equal(result.files[0].text,raw); assert.equal(result.files.at(-1).text,input);
    assert.equal(result.files.some(f=>f.name==='calibrated-selection.json'),status==='ACCEPTED');
  }
  const invalid={...permanentCalibration('REJECTED',true),selection_json:calibrationSelection(true)};
  const w=await workerModule({bindings:{default:()=>Promise.resolve(),material_calibrate_membrane_plasticity:()=>JSON.stringify(invalid)}});
  const result=await w.send({type:'calibrate',calibrationMode:'membrane_plasticity',calibrationRequest:input,allowEstimates:true});
  assert.equal(result.type,'error'); assert.match(result.message,/cannot publish a selection/);
});

test('scenario studies retain partial failures and free the fresh model on success, refusal and trap', async () => {
  for (const mode of [{}, { failScenario: true }, { trapScenario: true }]) {
    const w = await workerModule(mode);
    const msg = { type: 'scenario', request: 13, bytes: new ArrayBuffer(8), pattern: '{"x":-0.0}',
      studyRequest: '{"value":1.0}', scenarioRequest: '{"temperature":293.0}', measurements: '{"length":10.0}' };
    const r = await w.send(msg);
    assert.deepEqual(w.freed, ['body']);
    assert.deepEqual(w.called.find(c => c[0] === 'scenario').slice(2),
      [msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements]);
    assert.equal(r.type, mode.failScenario || mode.trapScenario ? 'error' : 'result');
    if (r.type === 'result') {
      assert.equal(r.result.simulations[0].evaluation.status, 'refused');
      assert.equal(r.files.length, 5); assert.match(r.files[0].text, /-0\.0/);
      assert.deepEqual(plain(r.files.slice(1).map(f => f.text)),
        [msg.pattern, msg.studyRequest, msg.scenarioRequest, msg.measurements]);
      await w.send(msg); assert.equal(w.called.filter(c => c[0] === 'body').length, 2);
      assert.deepEqual(w.freed, ['body', 'body']);
    } else {
      assert.equal(w.sent.some(m => m.type === 'result'), false);
      assert.equal(r.fatal, !!mode.trapScenario);
    }
  }
});

test('aggregate study JSON budget refuses before calling any consumer or allocating a body', async () => {
  const w = await workerModule();
  const chunk = ' '.repeat(33 * 1024 * 1024);
  const r = await w.send({ type: 'scenario', pattern: chunk, studyRequest: chunk });
  assert.equal(r.type, 'error'); assert.match(r.message, /64 MiB/); assert.deepEqual(w.called, []);
});

class Element {
  constructor() { this.value = ''; this.files = []; this.children = []; this.handlers = {}; this.checked = false; this.disabled = false; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
  removeAttribute(name) { delete this[name]; }
  getContext(kind) {
    assert.equal(this.tag, 'canvas'); assert.equal(kind, '2d');
    this.paint = [];
    return { clearRect: () => { this.paint = []; }, beginPath() {}, closePath() {}, stroke() {},
      moveTo: (...point) => this.paint.push(['move', ...point]), lineTo: (...point) => this.paint.push(['line', ...point]) };
  }
}
async function pageModule() {
  const html = read('materials.html');
  const e = Object.fromEntries([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Element()]));
  const commands = [...html.matchAll(/<button([^>]*)>/g)].filter(m => m[1].includes('data-command')).map(m => {
    const el = e[/id="([^"]+)"/.exec(m[1])[1]]; el.dataset = { command: /data-command="([^"]+)"/.exec(m[1])[1] }; return el;
  });
  const workers = [], live = new Set(), blobs = new Map(), windowEvents = {}; let next = 0;
  class Worker {
    constructor() { this.messages = []; this.dead = false; workers.push(this); }
    postMessage(message) { assert.equal(this.dead, false); this.messages.push(message); }
    terminate() { this.dead = true; }
    emit(message) { this.onmessage({ data: message }); }
  }
  class LocalURL extends URL {}
  LocalURL.createObjectURL = blob => { const id = `blob:${++next}`; live.add(id); blobs.set(id, blob); return id; };
  LocalURL.revokeObjectURL = id => { assert.equal(live.delete(id), true); blobs.delete(id); };
  const context = vm.createContext({ Worker, URL: LocalURL, Blob, JSON, Date, document: {
    getElementById: id => { assert.ok(e[id], `HTML missing ${id}`); return e[id]; },
    querySelectorAll: () => commands, createElement: tag => Object.assign(new Element(), { tag }),
  }, window: { addEventListener: (name, fn) => { windowEvents[name] = fn; } } });
  const module = new vm.SourceTextModule(read('materials-page.js'), { context, initializeImportMeta: m => { m.url = 'https://unit.invalid/materials-page.js'; } });
  await module.link(() => { throw Error('unexpected import'); }); await module.evaluate();
  const current = () => workers.at(-1), ready = () => current().emit({ type: 'ready' }); ready();
  const select = (id, text = '{}') => { e[id].files = [{ name: `${id}.json`, size: text.length, text: async () => text }]; };
  const emit = (result, files = [], reportText = JSON.stringify(result)) => current().emit({ type: 'result', request: current().messages.at(-1).request, result, files, reportText });
  const pattern = { pieces: [{ id: 'front' }, { id: 'lining' }] };
  const load = async () => { select('patternFile', JSON.stringify(pattern)); await e.patternFile.onchange(); };
  const inspect = async id => { e.selection.value = JSON.stringify({ id }); e.selection.handlers.input(); await e.inspect.onclick(); emit({ selection: { id, expected_sha256: 'locked' }, capabilities: {}, snapshot: { id } }); };
  const bind = async () => { await e.bind.onclick(); emit({ pattern, pattern_json:JSON.stringify(pattern), pieces: [], pattern_sha256: 'test' }, [{ name: 'pattern.json', text: '{}', mime: 'application/json' }]); };
  return { e, current, workers, ready, select, emit, load, inspect, bind, live, blobs, windowEvents };
}
test('material page freezes named assignments, withdraws dependent outputs and ignores stale worker replies', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('linen');
  p.e.target.value = 'lining'; p.e.assign.onclick(); await p.e.bind.onclick();
  const request = p.current().messages.at(-1);
  assert.equal(JSON.parse(request.changes).pieces.lining.expected_sha256, 'locked');
  p.emit({ pattern: { pieces: [] }, pattern_json:'{"pieces":[]}', pieces: [], pattern_sha256: 'old' }, [{ name: 'pattern.json', text: '{}', mime: 'application/json' }]);
  assert.equal(p.live.size, 1);
  p.select('cutOptions'); await p.e.cut.onclick(); const old = p.current(); const id = old.messages.at(-1).request;
  p.e.cutOptions.handlers.change(); assert.equal(old.dead, true); p.ready();
  old.emit({ type: 'result', request: id, result: { rolls: [] }, files: [{ name: 'old.json', text: '{}' }] });
  assert.equal(p.live.size, 1); // only the still-current bound pattern
  await p.inspect('leather'); p.e.target.value = ''; p.e.assign.onclick();
  assert.equal(p.live.size, 0); assert.equal(p.e.cut.disabled, true);
});
test('material page cancels delayed files before transfer and frees all downloads on page exit', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  p.select('cutOptions'); const d = deferred(); p.e.cutOptions.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.cut.onclick(); p.e.cancel.onclick(); p.ready(); d.resolve('{}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'cut').length, 0);
  assert.equal(p.live.size, 1); p.windowEvents.pagehide(); assert.equal(p.live.size, 0); assert.equal(p.current().dead, true);
});
test('material page protects newer pattern and selection edits against delayed reads', async () => {
  const p = await pageModule(); const d = deferred(); p.select('selectionFile'); p.e.selectionFile.files[0].text = () => d.promise;
  const old = p.e.selectionFile.onchange(); p.e.selection.value = '{"id":"new"}'; p.e.selection.handlers.input();
  d.resolve('{"id":"old"}'); await old; assert.equal(p.e.selection.value, '{"id":"new"}');
  const a = deferred(); p.select('patternFile'); p.e.patternFile.files[0].text = () => a.promise;
  const loading = p.e.patternFile.onchange(); await p.load(); a.resolve('{"pieces":[{"id":"stale"}]}'); await loading;
  await p.e.bind.onclick(); assert.equal(JSON.parse(p.current().messages.at(-1).pattern).pieces[0].id, 'front');
});
test('catalog changes cannot publish another material inspection or silently enable estimates', async () => {
  const p = await pageModule(); await p.load(); p.e.selection.value = '{"id":"old"}'; await p.e.inspect.onclick();
  const old = p.current(), id = old.messages.at(-1).request;
  p.e.catalog.value = '<new>'; p.e.catalog.onchange(); p.ready();
  old.emit({ type: 'result', request: id, result: { selection: { id: 'old' }, capabilities: {}, snapshot: {} } });
  assert.equal(p.e.assign.disabled, true); assert.equal(JSON.parse(p.e.selection.value).allow_estimates, false);
  const d = deferred(); p.select('selectionFile'); p.e.selectionFile.files[0].text = () => d.promise;
  const reading = p.e.selectionFile.onchange(); p.e.catalog.value = 'latest'; p.e.catalog.onchange();
  d.resolve('{"id":"late"}'); await reading; assert.equal(JSON.parse(p.e.selection.value).id, 'latest');
});
test('interface replacements bind with exact material edits, retain numeric text and can explicitly clear pairs', async () => {
  const p=await pageModule(); await p.load(); await p.inspect('replacement-shell');
  p.e.assign.onclick();
  const pairs='[{"materials":["shell-hash","lining-hash"],"friction":{"value":1.0,"bounds":[-0.0,1.0]},"allow_estimates":false}]';
  p.select('interfacesFile',pairs); await p.e.interfacesFile.onchange();
  assert.equal(p.e.bind.disabled,false);
  await p.e.bind.onclick(); const request=p.current().messages.at(-1);
  assert.ok(request.changes.includes(pairs));
  assert.equal(JSON.parse(request.changes).default.id,'replacement-shell');
  assert.equal(JSON.parse(request.changes).interfaces[0].allow_estimates,false);
  p.emit({pattern_json:'{}',pieces:[]});
  p.select('interfacesFile','[]'); await p.e.interfacesFile.onchange();
  await p.e.bind.onclick(); assert.deepEqual(JSON.parse(p.current().messages.at(-1).changes).interfaces,[]);
  p.emit({pattern_json:'{}',pieces:[]});
  p.e.clearInterfaces.onclick(); await p.e.bind.onclick();
  assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('invalid and superseded interface reads cannot silently reuse the old table or attach to another pattern', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  p.select('interfacesFile','{"interfaces":[]}'); await p.e.interfacesFile.onchange();
  assert.equal(p.e.bind.disabled,true); assert.equal(p.e.cut.disabled,true); assert.equal(p.live.size,0);
  assert.match(p.e.error.textContent,/JSON list/);
  const old=p.current().messages.length; await p.e.bind.onclick();
  assert.equal(p.current().messages.length,old);
  p.e.clearInterfaces.onclick(); assert.equal(p.e.bind.disabled,false);
  const delay=deferred(); p.select('interfacesFile'); p.e.interfacesFile.files[0].text=()=>delay.promise;
  const reading=p.e.interfacesFile.onchange(); assert.equal(p.e.bind.disabled,true);
  await p.load(); delay.resolve('[{"materials":["stale","pair"]}]'); await reading;
  await p.e.bind.onclick(); assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('changing interface evidence withdraws bound consumers and rejects an in-flight old result', async () => {
  const p=await pageModule(); await p.load(); await p.bind();
  p.select('cutOptions'); await p.e.cut.onclick(); const old=p.current(), id=old.messages.at(-1).request;
  p.select('interfacesFile','[]'); await p.e.interfacesFile.onchange();
  assert.equal(old.dead,true); assert.equal(p.live.size,0); assert.equal(p.e.cut.disabled,true);
  p.ready(); old.emit({type:'result',request:id,result:{rolls:[]},files:[{name:'stale.svg',text:'old',mime:'image/svg+xml'}]});
  assert.equal(p.live.size,0); assert.equal(p.e.cut.disabled,true);
  p.e.reset.onclick(); await p.e.bind.onclick();
  assert.equal(Object.hasOwn(JSON.parse(p.current().messages.at(-1).changes),'interfaces'),false);
});
test('sewing setup results retain Rust order, proof scope and complete downloads until recipe changes', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  const options = '{"ordering":{"objective":"minimize_setup_changes","max_search_nodes":1000},"source":-0.0}';
  p.select('sewOptions', options); p.e.sewOptions.handlers.change(); p.ready();
  await p.e.sew.onclick(); assert.equal(p.current().messages.at(-1).options, options);
  const changes = n => ({needle_changes:n,thread_changes:n,total_changes:2*n});
  const report = {schema:'ftx-sewing-program/1',stitches:[{}],plan:{schema:'ftx-sewing-plan/6',
    options:JSON.parse(options),feed_seconds:12,hardware_execution_authorized:false,
    seams:['a','c','b'].map((id,i)=>({pass:{id,seam:'side',needle:i<2?'N1':'N2',thread:i<2?'T1':'T2',after:id==='c'?['a']:[]}})),
    ordering:{baseline_order:['a','b','c'],selected_order:['a','c','b'],baseline_changes:changes(2),
      selected_changes:changes(1),search_nodes:8,optimality_proven:true,budget_exhausted:false}}};
  const raw=JSON.stringify(report), files=[{name:'sewing.json',text:raw,mime:'application/json'}];
  p.emit(report, files); const s=section(p,'sew');
  assert.match(visibleText(s),/Minimum setup-change count proven for the declared operations/);
  assert.match(visibleText(s),/No initial setup was supplied/);
  const tables=descendants(s).filter(n=>n.tag==='table');
  assert.deepEqual(plain(tables[0].children[3].children.map(n=>n.textContent)),['Selected order','1','1','2']);
  assert.deepEqual(plain(tables[1].children[3].children.map(n=>n.textContent)),['2','c','side','N1','T1','a']);
  const download=descendants(s).find(n=>n.download==='sewing.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  await p.e.sew.onclick(); report.plan.ordering.optimality_proven=false; report.plan.ordering.budget_exhausted=true;
  p.emit(report); assert.match(visibleText(s),/minimum setup-change count has not been proven/);
  assert.match(visibleText(s),/search reached its declared work limit/);
  p.e.sewOptions.handlers.change(); assert.equal(s.hidden,true);
  assert.equal(p.live.size,1); // only the still-current bound pattern remains
});

test('pressure-conditioned selections enter ordinary piece bindings without a numeric JSON round trip', async () => {
  const p = await pageModule(); await p.load();
  const request = '{"pressure_pa":250.0,"source_value":-0.0}';
  p.select('compressionRequest', request); p.e.compressionRequest.handlers.change();
  assert.equal(p.e.pressure.disabled, false);
  await p.e.pressure.onclick();
  assert.equal(p.current().messages.at(-1).compressionRequest, request);
  const selection = '{"id":"compressed","expected_sha256":"locked","overrides":{"test":{"value":-0.0}},"coefficient":1.0}';
  p.emit({ selection: JSON.parse(selection), selection_json: selection, input: { pressure_pa: 250 },
    pressure_range_pa: [0, 500], bracket_pa: [0, 500], interpolated: true,
    discarded_properties: ['bulk_density'], discarded_responses: [], discarded_structure: [],
    inspection: { snapshot: { id: 'compressed' }, capabilities: {} } },
    [{ name: 'pressure-conditioned-selection.json', text: selection, mime: 'application/json' }]);
  assert.equal(p.e.selection.value, selection); assert.equal(p.e.assign.disabled, false);
  p.e.target.value = 'front'; p.e.assign.onclick(); await p.e.bind.onclick();
  const binding = p.current().messages.at(-1).changes;
  assert.ok(binding.includes(selection));
  assert.equal(JSON.parse(binding).pieces.front.id, 'compressed');
  p.emit({ pattern: {}, pattern_json: '{}', pieces: [], pattern_sha256: 'new' });
  p.e.compressionRequest.handlers.change();
  assert.equal(p.e.assign.disabled, true);
  assert.equal(p.live.size, 0);
});

test('a newer selection cancels a delayed pressure request before it can replace the inspected material', async () => {
  const p = await pageModule(), delay = deferred(); await p.load();
  p.select('compressionRequest'); p.e.compressionRequest.files[0].text = () => delay.promise;
  const old = p.current(), running = p.e.pressure.onclick();
  p.e.catalog.value = 'newer-material'; p.e.catalog.onchange(); p.ready();
  delay.resolve('{"pressure_pa":250.0}'); await running;
  assert.equal(old.dead, true);
  assert.equal(old.messages.filter(m => m.type === 'pressure').length, 0);
  assert.equal(JSON.parse(p.e.selection.value).id, 'newer-material');
  assert.equal(p.e.assign.disabled, true);
});

test('identified swatches show independent errors, retain their source across pattern loading and bind without numeric mutation', async () => {
  const p=await pageModule(), input='{"coefficient":1.0,"origin":-0.0}';
  p.e.calibrationMode.value='nonlinear'; p.select('calibrationRequest',input); p.e.calibrationRequest.handlers.change();
  assert.equal(p.e.calibrate.disabled,false); assert.equal(p.e.calibrationEstimates.checked,false);
  await p.e.calibrate.onclick();
  assert.equal(p.current().messages.at(-1).calibrationRequest,input);
  assert.equal(p.current().messages.at(-1).allowEstimates,false);
  const selection=calibrationSelection(false);
  p.emit(calibrationReport('nonlinear',false,false),[{name:'calibrated-selection.json',text:selection,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  assert.match(visibleText(s),/Held out: not supplied/);
  assert.match(visibleText(s),/Fitted estimates need admission/);
  assert.match(visibleText(s),/Strain domain from fitting observations/);
  assert.equal(p.e.selection.value,selection); assert.equal(p.e.assign.disabled,true);
  await p.load(); assert.equal(s.hidden,false); assert.equal(p.live.size,1); assert.equal(p.e.assign.disabled,false);
  p.e.target.value='front'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(selection));
  assert.equal(JSON.parse(p.current().messages.at(-1).changes).pieces.front.allow_estimates,false);
});

test('calibration consent and response changes withdraw only the prepared result and cannot revive stale work', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='relaxation'; p.e.calibrationEstimates.checked=true;
  p.select('calibrationRequest'); p.e.calibrationRequest.handlers.change(); await p.e.calibrate.onclick();
  assert.equal(p.current().messages.at(-1).calibrationMode,'relaxation');
  p.emit(calibrationReport('relaxation',true),[{name:'calibrated-selection.json',text:calibrationSelection(true),mime:'application/json'}]);
  assert.match(visibleText(section(p,'Identified material behavior')),/Directional relaxation/);
  const recovery=descendants(section(p,'Identified material behavior')).find(n=>n.tag==='table' &&
    n.children[0].textContent==='Directional recovery — published stiffness at supplied times');
  assert.deepEqual(plain(recovery.children[2].children.map(n=>n.textContent)),['0.3','Active','240','60','-30','0']);
  assert.deepEqual(plain(recovery.children[5].children.map(n=>n.textContent)),['32','Inactive','0','0','0','0']);
  assert.match(visibleText(section(p,'Identified material behavior')),/not optimized from the histories/);
  p.e.calibrationEstimates.checked=false; p.e.calibrationEstimates.handlers.change();
  assert.equal(p.live.size,0); assert.equal(p.e.assign.disabled,true);
  await p.e.calibrate.onclick(); const old=p.current(), id=old.messages.at(-1).request;
  p.e.calibrationMode.value='nonlinear'; p.e.calibrationMode.handlers.change(); p.ready();
  old.emit({type:'result',request:id,result:calibrationReport('relaxation',true),files:[]});
  assert.equal(old.dead,true); assert.equal(section(p,'Identified material behavior').hidden,true);
  assert.equal(p.e.assign.disabled,true);
  const delay=deferred(); p.e.calibrationRequest.files[0].text=()=>delay.promise;
  const before=p.current(), running=p.e.calibrate.onclick();
  p.e.catalog.value='newer'; p.e.catalog.onchange(); p.ready(); delay.resolve('{}'); await running;
  assert.equal(before.messages.length,0); assert.equal(JSON.parse(p.e.selection.value).id,'newer');
});

test('directional time search shows identified values inside declared windows with local convergence and excitation', async () => {
  const p=await pageModule(); p.e.calibrationMode.value='relaxation'; p.e.calibrationEstimates.checked=true;
  p.select('calibrationRequest'); await p.e.calibrate.onclick();
  const report=calibrationReport('relaxation',true), c=report.calibration;
  c.input.orthotropic={time_bounds_seconds:[[0.1,0.9],[1,4],[5,24]]};
  c.orthotropic_fit.candidates=c.orthotropic_fit.candidates.slice(0,3);
  c.starts_attempted=5;
  c.orthotropic_fit.time_search={starts_attempted:5,starts_converged:2,iterations:11,objective_evaluations:73,
    projected_iterations:135,gradient_scaled_max:0,time_sensitivity:[13,7,4],
    joint_excitation_pivot:0.02,boundary_distance_fraction:[0.5,0.5,0.3]};
  p.emit(report);
  const s=section(p,'Identified material behavior'), rendered=visibleText(s);
  const tables=descendants(s).filter(n=>n.tag==='table');
  const windows=tables.find(n=>n.children[0].textContent==='Directional time search — declared windows and local sensitivity');
  assert.deepEqual(plain(windows.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['0.1','0.3','0.9','13','0.5'],['1','2','4','7','0.5'],['5','8','24','4','0.3']
  ]);
  const diagnostics=tables.find(n=>n.children[0].textContent==='Directional time search — local optimizer diagnostics');
  assert.deepEqual(plain(diagnostics.children.slice(2).map(row=>row.children.map(n=>n.textContent))),[
    ['Converged starts / attempted','2 / 5'],['Iterations in selected solve','11'],
    ['Objective evaluations across starts and trials','73'],['Projected tensor iterations across all solves','135'],
    ['Maximum scaled gradient','0'],
    ['Joint stiffness / time excitation pivot','0.02']
  ]);
  assert.match(rendered,/Directional recovery — published stiffness at identified times/);
  assert.match(rendered,/do not establish a global optimum, a unique spectrum or confidence intervals/);
  assert.match(rendered,/Every identified time must retain an observable recovery branch/);
  assert.doesNotMatch(rendered,/at supplied times|not optimized from the histories|entirely inactive spectrum retains/);
  assert.equal(p.e.selection.value,calibrationSelection(true));
});

test('permanent-stretch results show bounded coefficients and withdraw selections for every unresolved outcome', async () => {
  const p=await pageModule(); await p.load();
  p.e.calibrationMode.value='membrane_plasticity'; p.e.calibrationEstimates.checked=true;
  const input='{"schema":"ftx-membrane-plasticity-calibration/1","source":-0.0}';
  p.select('calibrationRequest',input); p.e.calibrationRequest.handlers.change(); p.ready();
  await p.e.calibrate.onclick(); assert.equal(p.current().messages.at(-1).calibrationMode,'membrane_plasticity');
  const report=permanentCalibration('ACCEPTED',true,false), raw=JSON.stringify(report);
  p.emit(report,[{name:'material-calibration.json',text:raw,mime:'application/json'}]);
  const s=section(p,'Identified material behavior');
  assert.equal(p.e.assign.disabled,false); assert.equal(p.e.selection.value,report.selection_json);
  assert.match(visibleText(s),/Held-out replay: NOT_PROVIDED/);
  assert.match(visibleText(s),/second-Piola sheet resultants/);
  const parameters=descendants(s).find(n=>n.tag==='table'&&visibleText(n).includes('Yield search range'));
  assert.deepEqual(plain(parameters.children[2].children.map(n=>n.textContent)),['Warp / wale','10','[1, 20]','100','[0, 200]']);
  const download=descendants(s).find(n=>n.download==='material-calibration.json');
  assert.equal(await p.blobs.get(download.href).text(),raw);
  p.e.target.value='front'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(report.selection_json));
  p.emit({pattern_json:'{}',pieces:[]});
  for (const status of ['AMBIGUOUS','BUDGET_EXHAUSTED','REJECTED']) {
    await p.e.calibrate.onclick(); p.emit(permanentCalibration(status,true));
    assert.equal(p.e.assign.disabled,true); assert.equal(p.e.selection.value,'');
    assert.match(visibleText(s),new RegExp(`identification: ${status}`));
    assert.match(visibleText(s),/No identified selection is available/);
  }
  p.e.calibrationRequest.handlers.change(); assert.equal(s.hidden,true);
});

test('a refused replacement calibration leaves no previous selection or report available for assignment', async () => {
  const p=await pageModule(); await p.load(); p.e.calibrationMode.value='nonlinear'; p.select('calibrationRequest');
  await p.e.calibrate.onclick(); p.emit(calibrationReport(),[{name:'material-calibration.json',text:'{}',mime:'application/json'}]);
  assert.equal(p.live.size,1); assert.equal(p.e.assign.disabled,false);
  await p.e.calibrate.onclick(); const request=p.current().messages.at(-1).request;
  p.current().emit({type:'error',request,message:'held-out peak exceeds declared budget'});
  assert.match(p.e.error.textContent,/held-out peak/); assert.equal(p.live.size,0);
  assert.equal(p.e.assign.disabled,true); assert.equal(section(p,'Identified material behavior').hidden,true);
});

test('recovery ambiguity displays competing models and withdraws the previous assignable material', async () => {
  const p=await pageModule(); await p.load(); p.e.calibrationMode.value='relaxation'; p.select('calibrationRequest');
  await p.e.calibrate.onclick(); p.emit(recoveryComparison('ACCEPTED'));
  assert.equal(p.e.assign.disabled,false); assert.equal(p.e.selection.value,calibrationSelection(true));
  await p.e.calibrate.onclick(); p.emit(recoveryComparison(),[
    {name:'material-calibration.json',text:'{}',mime:'application/json'},
    {name:'calibration-input.json',text:'{}',mime:'application/json'}]);
  const s=section(p,'Identified material behavior'), t=descendants(s).find(n=>n.tag==='table');
  assert.match(visibleText(s),/Comparison outcome: AMBIGUOUS/);
  assert.match(visibleText(s),/Choice before held-out replay: unresolved/);
  assert.deepEqual(plain(t.children[2].children.map(n=>n.textContent)),
    ['complete','12','ACCEPTED','0.02','0.04','Yes','ACCEPTED']);
  assert.equal(p.e.selection.value,''); assert.equal(p.e.assign.disabled,true); assert.equal(p.live.size,2);
});

test('material changes withdraw drape and thermal outputs while environment edits preserve mechanics', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  for (const k of ['measurements', 'fit', 'body', 'heatConditions']) p.select(k);
  p.e.body.files[0].arrayBuffer = async () => new ArrayBuffer(8);
  p.e.measurements.handlers.change(); await p.e.fold.onclick();
  const raw='{"material":{"basis":"test"},"coordinate":-0.0,"coefficient":1.0}';
  p.emit(JSON.parse(raw), [{ name: 'sim_report.json', text: raw, mime: 'application/json' }, { name: 'viewer.html', text: '<html/>', mime: 'text/html' }],raw);
  await p.e.heat.onclick();assert.equal(p.current().messages.at(-1).report,raw);
  p.emit({ steady: {}, comfort_claim: false }, [{ name: 'heat.json', text: '{}', mime: 'application/json' }]);
  assert.equal(p.live.size, 4); p.e.heatConditions.handlers.change(); assert.equal(p.live.size, 3); assert.equal(p.e.viewer.hidden, false);
  await p.inspect('different'); p.e.assign.onclick(); assert.equal(p.live.size, 0); assert.equal(p.e.viewer.hidden, true); assert.equal(p.e.heat.disabled, true);
});

test('original drafting worker also retains replayable drape JSON and frees its fold handle', async () => {
  const sent=[], raw='{"coordinate":-0.0,"coefficient":1.0}', freed=[];
  const values={default:async()=>{},program_info:()=> '{}',
    run:()=>({page:()=>'{"status":"CONVERGED"}',file:n=>new TextEncoder().encode(n==='pattern.json'?'{"pieces":[{"placement":{"part":"torso"}}]}':'{}'),free(){}}),
    WebBody:class{},fold_up:()=>({report:()=>raw,viewer:()=>'<html/>',free:()=>freed.push('fold')})};
  const context=vm.createContext({JSON,Date,TextDecoder,Uint8Array,WebAssembly,Error,self:{},onmessage:null,postMessage:m=>sent.push(m)});
  const imports=new vm.SyntheticModule(Object.keys(values),function(){for(const[n,v]of Object.entries(values))this.setExport(n,v);},{context});
  await imports.link(()=>{});await imports.evaluate();
  const module=new vm.SourceTextModule(read('worker.js'),{context,importModuleDynamically:()=>imports});
  await module.link(()=>{});await module.evaluate();
  for(const msg of [{type:'init',pkg:'./pkg/'},{type:'run',req:{}},{type:'body',bytes:new ArrayBuffer(8)},{type:'fold',selfContactMm:2}]) await context.onmessage({data:msg});
  assert.equal(sent.at(-1).type,'folded');assert.equal(sent.at(-1).reportText,raw);assert.deepEqual(freed,['fold']);
  assert.ok(read('index.html').includes('new Blob([msg.reportText]'));
});

test('drafting worker discards refused replacements, frees failed rounds and checks fold sources', async () => {
  const sent=[], freed=[], calls=[]; let failure;
  const values={default:async()=>{},program_info:()=> '{}',
    run:()=> {
      if(failure==='run') throw Error('run refused');
      return {page:()=>{if(failure==='page') throw Error('page refused');return '{"status":"CONVERGED","zip":"round.zip"}';},
        zip:()=>{if(failure==='zip') throw Error('zip refused');return new Uint8Array([1]);},
        file:n=>{if(failure===n) throw Error('file refused');return new TextEncoder().encode(n==='pattern.json'?'{"pieces":[{"placement":{"part":"torso"}}]}':'{}');},
        free:()=>freed.push('round')};
    },
    WebBody:class {constructor(bytes){if(bytes[0]===0)throw Error('body refused');}free(){freed.push('body');}},
    fold_up:()=>{calls.push('fold');return {report:()=>'{"gating":[]}',viewer:()=>'<html/>',free:()=>freed.push('fold')};}};
  const context=vm.createContext({JSON,Date,TextDecoder,Uint8Array,WebAssembly,Error,self:{},onmessage:null,postMessage:m=>sent.push(m)});
  const imports=new vm.SyntheticModule(Object.keys(values),function(){for(const[n,v]of Object.entries(values))this.setExport(n,v);},{context});
  await imports.link(()=>{});await imports.evaluate();
  const module=new vm.SourceTextModule(read('worker.js'),{context,importModuleDynamically:()=>imports});
  await module.link(()=>{});await module.evaluate();
  const send=async data=>{await context.onmessage({data});return sent.at(-1);};
  await send({type:'init',pkg:'./pkg/'});
  const body=()=>send({type:'body',request:2,bytes:new Uint8Array([1]).buffer});
  const run=()=>send({type:'run',request:3,req:{}});
  const fold=extra=>send({type:'fold',request:4,round:3,body:2,...extra});
  await body();await run();assert.equal((await fold()).type,'folded');
  assert.equal((await fold({round:1})).type,'error');assert.equal(calls.length,1);
  for(const stage of ['run','page','zip','pattern.json','measurements.json','fit_spec.json']) {
    failure=undefined;await run();const count=freed.filter(x=>x==='round').length;
    failure=stage;const refused=await run();assert.equal(refused.type,'error');assert.equal(refused.request,3);
    assert.equal(freed.filter(x=>x==='round').length,count+(stage==='run'?0:1),stage);
    assert.match((await fold()).message,/run a round first/);
  }
  failure=undefined;await run();await send({type:'body',request:5,bytes:new Uint8Array([0]).buffer});
  assert.match((await fold()).message,/load the body model/); // no fallback to the prior model
  await body();assert.equal((await fold()).type,'folded');
  await send({type:'clearBody'});assert.match((await fold()).message,/load the body model/);
});

// The original inline workshop script, using a small DOM transport double. These tests inspect
// source/result association and handle lifetime, not browser rendering or computed garment physics.
function draftingPage() {
  const html=read('index.html'), e=Object.fromEntries([...html.matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
  for(const el of Object.values(e)) {
    el.addEventListener=(name,fn)=>{const previous=el.handlers[name];el.handlers[name]=(...args)=>{
      const a=previous?.(...args),b=fn(...args);return Promise.all([a,b]);};};
    const classes=new Set(['hidden']);el.classList={add:x=>classes.add(x),remove:x=>classes.delete(x),
      contains:x=>classes.has(x),toggle:(x,on)=>on?classes.add(x):classes.delete(x)};
    el.dataset={};el.querySelectorAll=()=>[];el.scrollIntoView=()=>{};
  }
  e.stature.value='92';e.age.value='30';e.style.value='{}';
  const modes=['chart','tape','file'].map(value=>Object.assign(new Element(),{value,checked:value==='chart'}));
  const messages=[], live=new Map();let worker, next=0;
  class Worker {constructor(){worker=this;}postMessage(m){messages.push(m);}}
  class LocalURL extends URL {}
  LocalURL.createObjectURL=blob=>{const u=`blob:${++next}`;live.set(u,blob);return u;};
  LocalURL.revokeObjectURL=u=>assert.equal(live.delete(u),true);
  const context=vm.createContext({Worker,URL:LocalURL,Blob,JSON,Date,location:{href:'https://unit.invalid/'},
    localStorage:{getItem:()=>null,setItem(){}},document:{body:{dataset:{}},getElementById:id=>{assert.ok(e[id],id);return e[id];},
      querySelector:()=>modes.find(m=>m.checked),querySelectorAll:()=>modes}});
  vm.runInContext(/<script>([\s\S]*?)<\/script>/.exec(html)[1],context);
  const emit=m=>worker.onmessage({data:m}), last=type=>messages.findLast(m=>m.type===type);
  emit({type:'ready',info:{zouave_bolero:{style:{}}},version:'test',dxf:'test'});
  const result=request=>emit({type:'result',request:request??last('run').request,foldable:true,
    res:{title:'current',status:'CONVERGED',basis:{},zip:'round.zip',files:[],validation:{}},blobs:{zip:new Uint8Array([1])}});
  const body=async()=>{e.bodyFile.files=[{arrayBuffer:async()=>new ArrayBuffer(1)}];await e.bodyFile.handlers.change();
    emit({type:'bodyReady',request:last('body').request});};
  const dispatch=(id,event)=>e[id].handlers[event]?.();
  return {e,modes,messages,live,emit,last,result,body,worker,dispatch};
}

test('primary workshop withdraws obsolete rounds and folds while retaining the latest positive workflow', async () => {
  const p=draftingPage();await p.e.go.handlers.click();p.result();await p.body();
  assert.equal(p.e.foldGo.disabled,false);p.e.foldGo.handlers.click();const fold=p.last('fold');
  assert.equal(fold.round,p.last('run').request);assert.equal(fold.body,p.last('body').request);
  p.emit({type:'folded',request:fold.request,report:{settled:true,gating:[]},reportText:'{"value":-0.0}',viewer:'<html/>'});
  assert.equal(p.live.size,3);assert.match(p.e.foldSummary.innerHTML,/no gating flag/);
  assert.ok([...p.live.values()].some(b=>b.type==='application/json'));
  p.e.stature.value='93';p.dispatch('stature','input');
  assert.equal(p.live.size,0);assert.equal(p.e.foldGo.disabled,true);assert.ok(p.e.results.classList.contains('hidden'));
  assert.equal(p.e.status.textContent,'Inputs changed; draft and check again.');
  p.result(fold.round);p.emit({type:'folded',request:fold.request,report:{settled:true},reportText:'{}',viewer:'old'});
  assert.equal(p.live.size,0);assert.equal(p.e.foldGo.disabled,true);
  await p.e.go.handlers.click();p.result();assert.equal(p.e.foldGo.disabled,false);
  p.e.foldGo.handlers.click();const current=p.last('fold');
  p.e.selfContact.handlers.change();assert.equal(p.live.size,1);assert.equal(p.e.foldGo.disabled,false);
  p.emit({type:'folded',request:current.request,reportText:'{}',report:{},viewer:'obsolete'});assert.equal(p.live.size,1);
  const oldRound=p.last('run').request;await p.e.go.handlers.click();
  assert.equal(p.e.foldGo.disabled,true);assert.equal(p.live.size,0);
  p.emit({type:'error',op:'run',request:oldRound,message:'obsolete failure'});assert.ok(p.e.errorBox.classList.contains('hidden'));
  p.emit({type:'error',op:'run',request:p.last('run').request,message:'replacement refused'});
  assert.equal(p.e.errorText.textContent,'replacement refused');assert.equal(p.e.foldGo.disabled,true);
});

test('primary workshop cancels delayed measurement reads and accepts only the latest fields', async () => {
  const p=draftingPage(), pending=deferred();p.modes[0].checked=false;p.modes[2].checked=true;
  p.e.measFile.files=[{text:()=>pending.promise}];const loading=p.e.go.handlers.click();
  p.dispatch('stature','input');pending.resolve('{}');await loading;
  assert.equal(p.last('run'),undefined);assert.equal(p.e.go.disabled,false);
  p.e.measFile.files=[{text:async()=>'{}'}];await p.e.go.handlers.click();p.result();assert.equal(p.live.size,1);
  const old=p.last('fields');p.e.stature.handlers.change();const fresh=p.last('fields');
  p.emit({type:'fields',request:old.request,program:'zouave_bolero',rows:[{name:'old'}]});assert.equal(p.e.tapeRows.innerHTML,undefined);
  p.emit({type:'fields',request:fresh.request,program:'zouave_bolero',rows:[]});assert.equal(p.e.tapeRows.innerHTML,'');
});

test('primary workshop cannot install a superseded body, revive a cleared model or use a dead worker', async () => {
  const p=draftingPage(), pending=deferred();await p.e.go.handlers.click();p.result();await p.body();
  p.e.bodyFile.files=[{arrayBuffer:()=>pending.promise}];const loading=p.e.bodyFile.handlers.change();
  assert.equal(p.e.foldGo.disabled,true);assert.equal(p.live.size,1);
  await p.body();const current=p.last('body').request;pending.resolve(new ArrayBuffer(2));await loading;
  assert.equal(p.last('body').request,current);assert.equal(p.e.foldGo.disabled,false);
  p.e.bodyFile.files=[];await p.e.bodyFile.handlers.change();p.emit({type:'bodyReady',request:current});assert.equal(p.e.foldGo.disabled,true);
  p.e.bodyFile.files=[{arrayBuffer:async()=>{throw Error('read refused');}}];await p.e.bodyFile.handlers.change();
  assert.equal(p.e.errorText.textContent,'read refused');assert.equal(p.e.foldGo.disabled,true);
  p.emit({type:'error',op:'fold',request:0,fatal:true,message:'trap'});assert.equal(p.live.size,0);assert.equal(p.e.go.disabled,true);
  p.result();assert.equal(p.live.size,0);assert.equal(p.e.go.textContent,'Reload the page');
});

test('finite-stock input and material edits invalidate only their current dependencies', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  p.select('stockRequest', '{"value":1.0}'); p.e.stockRequest.handlers.change();
  assert.equal(p.e.stock.disabled, false);
  await p.e.stock.onclick();
  assert.equal(p.current().messages.at(-1).stockRequest, '{"value":1.0}');
  p.emit({ markers: [], limitations: [] }, [
    { name: 'plan.json', text: '{}', mime: 'application/json' },
    { name: 'stock-0.svg', text: '<svg/>', mime: 'image/svg+xml' }
  ]);
  assert.equal(p.live.size, 3); assert.equal(p.e.stockMarker.hidden, false);
  p.select('quotes'); p.e.quotes.handlers.change(); assert.equal(p.live.size, 3);
  p.e.stockRequest.handlers.change(); assert.equal(p.live.size, 1); assert.equal(p.e.stockMarker.hidden, true);
  await p.e.stock.onclick(); const old = p.current(), id = old.messages.at(-1).request;
  p.e.stockRequest.handlers.change(); p.ready();
  old.emit({ type: 'result', request: id, files: [{ name: 'stale.json', text: '{}' }] });
  assert.equal(p.live.size, 1);
  await p.inspect('replacement'); p.e.assign.onclick(); assert.equal(p.live.size, 0);
  assert.equal(p.e.stock.disabled, true);
});

const descendants = el => [el, ...el.children.flatMap(descendants)];
const visibleText = el => descendants(el).map(n => n.textContent || '').join('\n');
const section = (p, title) => p.e.outputs.children.find(s => s.children[0].textContent === title);
const outcome = (result = {}) => ({ status: 'available', result });
const refused = message => ({ status: 'refused', refusal: { code: 'MISSING_DATA', message } });
const omitted = () => ({ status: 'not_requested' });
const studyRow = (key = 'A', pattern = '{"pieces":[],"x":-0.0,"value":1.0}') => ({
  key, binding: outcome({ pattern_json: pattern, pattern_sha256: 'pattern-lock', pieces: [{ material: 'retained source' }] }),
  frozen_changes: { default: { id: 'source-A', expected_sha256: 'source-lock' } },
  cutting: refused('missing stock width'), sewing: outcome(), purchasing: omitted(),
  metrics: { stock_area_m2: null, stock_mass_kg: null, stitch_intervals: 0, feed_seconds: 0 }
});
const studyReport = (rows = [studyRow()]) => ({ schema: 'ftx-material-study/1', alternatives: rows,
  source_pattern_sha256: 'original-lock', geometry_changed: false, physical_validation_claim: false, hardware_execution_authorized: false });
const screenReport = () => ({ schema: 'ftx-material-screen-report/1', basis: 'reported_bounds', allow_mixed_protocols: false, frontier: ['A'],
  alternatives: [{ key: 'A', eligible: true, dominated_by: [], exclusions: [],
    readings: { areal_density: { value: 0.2, unit: 'kg/m2', bounds: [0.19, 0.21] } }, selection: { id: 'source-A' } },
    { key: '<excluded>', eligible: false, dominated_by: [], exclusions: [{ message: 'missing upper bound' }],
      readings: {}, selection: { id: 'source-B', source: '<literal evidence>' } }] });
const scenarioReport = () => ({ schema: 'ftx-material-scenario-study/1', scenario: { model_sha256: 'model-lock' },
  manufacturing: studyReport([studyRow('A'), studyRow('B')]),
  // Deliberately not manufacturing order: the UI must join on stable keys, not array positions.
  simulations: [{ key: 'B', evaluation: refused('missing mechanics') }, { key: 'A', evaluation: outcome({
    metrics: { simulation_gating_clear: false, cloth_mass_kg: 0, independent_surface_power_w: -4.5, body_coverage_power_w: null },
    surface: outcome(), coverage: refused('missing conductivity'), drape: { settled: false, flags: ['unsettled'] },
    body_geometry_sha256: 'body-lock', positions_sha256: 'positions-lock' }) }] });
function scenarioInputs(p) {
  p.select('studyRequest', '{"value":1.0}'); p.select('scenarioRequest', '{"temperature":293.0}');
  p.select('measurements', '{"length":10.0}'); p.select('body');
  p.e.body.files[0].arrayBuffer = async () => new ArrayBuffer(8); p.e.body.handlers.change();
}

function addChoice(p, group, choice, pieces = ['front']) {
  p.e.combinationGroup.value = group; p.e.combinationChoice.value = choice;
  p.e.combinationScope.value = pieces === null ? 'default' : 'pieces';
  for (const option of p.e.combinationPieces.children) option.selected = pieces?.includes(option.value) || false;
  p.e.addCombination.onclick();
}

test('combination controls retain frozen selections and exact policy/pair numbers for native study evaluation', async () => {
  const p = await pageModule(); await p.load();
  p.e.combinationMode.checked = true; p.e.combinationMode.handlers.change();
  assert.equal(p.e.study.disabled, true);
  const policy = '{"schema":"ftx-material-study/1","assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],"seam_allowance_mm":-0.0}}';
  p.select('studyRequest', policy); p.e.studyRequest.handlers.change();
  await p.inspect('light'); addChoice(p, 'shell', 'light');
  await p.inspect('heavy'); addChoice(p, 'shell', 'heavy');
  await p.inspect('lining'); addChoice(p, 'lining', 'soft', ['lining']);
  assert.match(p.e.combinationCount.textContent, /^2 complete material combinations/);
  const pairs = '[{"materials":["a","b"],"friction":{"value":1.0,"bounds":[-0.0,1.0]}}]';
  p.select('combinationInterfaces', pairs); p.e.combinationInterfaces.handlers.change();
  assert.equal(p.e.study.disabled, false);
  await p.e.study.onclick();
  const sent = p.current().messages.at(-1), request = JSON.parse(sent.studyRequest);
  assert.ok(sent.studyRequest.includes('"seam_allowance_mm":-0.0'));
  assert.ok(sent.studyRequest.includes(pairs));
  assert.equal(request.combinations.groups[0].choices[0].selection.expected_sha256, 'locked');
  assert.deepEqual(plain(request.combinations.groups.map(g => [g.key, g.target])),
    [['shell', {scope:'pieces',ids:['front']}], ['lining',{scope:'pieces',ids:['lining']}]]);
  assert.equal(Object.hasOwn(request, 'variants'), false);
  assert.equal(sent.pattern, JSON.stringify({pieces:[{id:'front'},{id:'lining'}]}));
  const row = studyRow('generated'); row.choices = {lining:'soft', shell:'heavy'};
  p.emit(studyReport([row]));
  const result = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(result), /lining: soft; shell: heavy/);
  assert.equal(descendants(result).some(n => 'innerHTML' in n), false);
  await p.inspect('replacement'); addChoice(p, 'shell', 'heavy');
  assert.equal(result.hidden, true); assert.equal(p.live.size, 0);
  assert.match(p.e.combinationCount.textContent, /^2 complete/);
  await p.e.study.onclick();
  assert.equal(JSON.parse(p.current().messages.at(-1).studyRequest).combinations.groups[0].choices[1].selection.id, 'replacement');
});

test('combination controls reject overlapping targets, changed group targets, excessive products and ambiguous study sources', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('source');
  p.e.combinationMode.checked = true;
  addChoice(p, 'shell', 'one');
  addChoice(p, 'other', 'one'); assert.match(p.e.error.textContent, /disjoint/);
  addChoice(p, 'shell', 'two', ['lining']); assert.match(p.e.error.textContent, /same target/);
  for (let i = 2; i <= 32; i++) addChoice(p, 'shell', `choice-${i}`);
  assert.match(p.e.combinationCount.textContent, /^32 complete/);
  addChoice(p, 'shell', 'too many'); assert.match(p.e.error.textContent, /limit of 32/);
  assert.match(p.e.combinationCount.textContent, /^32 complete/);
  p.select('studyRequest','{"schema":"ftx-material-study/1","variants":[]}');
  const before = p.current().messages.length; await p.e.study.onclick();
  assert.equal(p.current().messages.length, before);
  assert.match(p.e.error.textContent, /must omit variants/);
  p.e.clearCombinations.onclick(); assert.equal(p.e.study.disabled, true);
  addChoice(p, 'base', 'one', null); addChoice(p, 'another base', 'two', null);
  assert.match(p.e.error.textContent, /Only one group/);
  assert.match(p.e.combinationCount.textContent, /^1 complete/);
});

test('combination changes and pattern replacement cancel pending reads and cannot publish stale alternatives', async () => {
  const p = await pageModule(); await p.load(); await p.inspect('source');
  p.e.combinationMode.checked = true; addChoice(p, 'shell', 'one');
  p.select('studyRequest','{"schema":"ftx-material-study/1"}');
  const delayed = deferred(); p.e.studyRequest.files[0].text = () => delayed.promise;
  const old = p.current(), running = p.e.study.onclick();
  p.e.combinationMode.checked = false; p.e.combinationMode.handlers.change(); p.ready();
  delayed.resolve('{"schema":"ftx-material-study/1"}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'study').length, 0);
  p.e.combinationMode.checked = true;
  p.select('studyRequest','{"schema":"ftx-material-study/1"}');
  await p.e.study.onclick(); const previous = p.current(), id = previous.messages.at(-1).request;
  await p.load(); p.ready();
  previous.emit({type:'result',request:id,result:studyReport(),files:[{name:'stale.json',text:'{}'}]});
  assert.equal(p.live.size, 0); assert.equal(p.e.study.disabled, true);
  assert.equal(p.e.combinationGroups.children.length, 0);
  assert.equal(p.e.combinationPieces.children.length, 2);
});

test('purchasing displays exact intraday evidence, excluded latest offers and date-only history without repricing', async () => {
  const p = await pageModule(); await p.load(); await p.bind();
  const quotes = '{"schema":"ftx-textile-quotes/2","price":1.0}';
  const market = '{"as_of":"2026-10-20","as_of_at":"2026-10-20T12:00:00.000000001Z"}';
  p.select('cutOptions'); p.select('quotes', quotes); p.select('market', market);
  p.e.market.handlers.change(); await p.e.price.onclick();
  const sent = p.current().messages.at(-1);
  assert.equal(sent.quotes, quotes); assert.equal(sent.market, market);
  const scope = { currency: 'USD', region: 'US', quality: 'reviewed', terms: 'merchandise only',
    as_of: '2026-10-20', as_of_at: '2026-10-20T12:00:00.000000001Z' };
  const quote = { offer_id: '<literal supplier>', observed_on: '2026-10-20',
    observed_at: '2026-10-20T12:00:00Z', valid_through_at: '2026-10-20T12:00:00.000000001Z', unit: 'linear_meter' };
  const report = { complete_offer_coverage: false, complete_observed_quantity_coverage: false,
    currency: 'USD', least_observed_independent_order_total: null, limits: ['Observed prices only'], roll_prices: [
      { roll_index: 0, required_stock_area_m2: 0.5, sample: { scope,
        offers: [{ quote, order_quantity: 0.5, order_cost: 7.25 }],
        excluded_offers: [{ quote: { ...quote, offer_id: 'expired', valid_through_at: '2026-10-20T12:00:00Z' }, reason: 'expired' },
          { quote: { ...quote, offer_id: 'limited' }, reason: 'insufficient_quantity', required_order_quantity: 1.5 }] } },
      { roll_index: 1, required_stock_area_m2: 1, sample: { scope: { ...scope, as_of_at: undefined }, offers: [],
        excluded_offers: [{ quote: { offer_id: 'daily', observed_on: '2026-10-20', valid_through: '2026-10-20' }, reason: 'unavailable' }] } }
    ] };
  p.emit(report, [{ name: 'quotes.json', text: quotes, mime: 'application/json' }]);
  const s = section(p, 'price'), tables = descendants(s).filter(n => n.tag === 'table');
  assert.deepEqual(plain(tables[0].children[2].children.map(n => n.textContent)),
    ['<literal supplier>', '2026-10-20T12:00:00Z', '2026-10-20T12:00:00.000000001Z', 'Eligible', '0.5 linear_meter', '7.25']);
  assert.equal(tables[0].children[3].children[3].textContent, 'Excluded: expired');
  assert.equal(tables[0].children[3].children[5].textContent, 'Unknown');
  assert.equal(tables[0].children[4].children[4].textContent, '1.5 linear_meter');
  assert.equal(tables[1].children[2].children[1].textContent, '2026-10-20 (date only)');
  assert.match(visibleText(s), /Evaluated at 2026-10-20T12:00:00\.000000001Z UTC/);
  assert.match(visibleText(s), /Evaluated at 2026-10-20 \(date only\) UTC/);
  assert.equal(JSON.parse(s.children[1].textContent).least_observed_independent_order_total, null);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  const link = descendants(s).find(n => n.download === 'quotes.json');
  assert.equal(await p.blobs.get(link.href).text(), quotes);
  p.select('market', market.replace('000000001', '000000002')); p.e.market.handlers.change();
  assert.equal(s.hidden, true); assert.equal(s.children[2].children.length, 0);
  assert.equal(p.live.has(link.href), false);
});

test('screening is available without a pattern and retains excluded source evidence for inspection', async () => {
  const p = await pageModule(); assert.equal(p.e.screen.disabled, true);
  p.select('screenRequest', '{"threshold":1.0}'); p.e.screenRequest.handlers.change();
  assert.equal(p.e.screen.disabled, false); assert.equal(p.e.study.disabled, true);
  await p.e.screen.onclick(); assert.equal(p.current().messages.at(-1).screenRequest, '{"threshold":1.0}');
  p.emit(screenReport());
  const s = section(p, 'Source-qualified screening');
  assert.match(visibleText(s), /reported_bounds/); assert.match(visibleText(s), /missing upper bound/);
  assert.match(visibleText(s), /0.2 kg\/m2; \[0.19, 0.21\] reported bounds/); assert.match(visibleText(s), /Unknown/);
  assert.match(visibleText(s), /not garment-level ranking/);
  const details = descendants(s).filter(n => n.tag === 'details');
  details[1].open = true; details[1].handlers.toggle();
  assert.match(visibleText(s), /<literal evidence>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
});

test('manufacturing studies use original lossless source, not bound previews, and download exact alternative patterns', async () => {
  const p = await pageModule(), original = '{"pieces":[{"id":"front"}],"x":-0.0,"coefficient":1.0}';
  p.select('patternFile', original); await p.e.patternFile.onchange();
  p.select('studyRequest', '{"coefficient":1.0}'); p.e.studyRequest.handlers.change();
  assert.equal(p.e.study.disabled, false); assert.equal(p.e.cut.disabled, true);
  await p.e.bind.onclick(); p.emit({ pattern_json: '{"different":"preview"}', pieces: [] });
  await p.e.study.onclick(); const request = p.current().messages.at(-1);
  assert.equal(request.pattern, original); assert.equal(request.studyRequest, '{"coefficient":1.0}');
  const row = studyRow('../<unsafe-name>'); p.emit(studyReport([row]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(s), /missing stock width/); assert.match(visibleText(s), /Available/);
  const t = descendants(s).find(n => n.tag === 'table'); const values = t.children[2].children.map(n => n.textContent);
  assert.equal(values[7], 'Unknown'); assert.equal(values[8], 'Unknown');
  assert.equal(values[9], '0'); assert.equal(values[10], '0');
  const a = descendants(s).find(n => n.download);
  assert.equal(a.download, 'alternative-1-pattern.json');
  assert.equal(await p.blobs.get(a.href).text(), row.binding.result.pattern_json);
  p.windowEvents.pagehide(); assert.equal(p.live.size, 0);
});

test('scenario comparisons join keys, preserve signed values, and distinguish gating and independent refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  assert.equal(p.e.scenario.disabled, false); assert.equal(p.e.fold.disabled, true); // no bind or single-preview fit
  await p.e.scenario.onclick(); const request = p.current().messages.at(-1);
  assert.equal(request.scenarioRequest, '{"temperature":293.0}'); assert.equal(request.measurements, '{"length":10.0}');
  assert.equal(request.bytes.byteLength, 8); assert.equal('fit' in request, false);
  p.emit(scenarioReport()); const s = section(p, 'Fresh drape and heat alternatives');
  const t = descendants(s).filter(n => n.tag === 'table')[1];
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
    ['A', 'Available', 'NO — review flags', '0', 'Available', '-4.5', 'Refused: missing conductivity', 'Unknown']);
  assert.match(t.children[3].children[1].textContent, /missing mechanics/);
  assert.equal(t.children[3].children[3].textContent, 'Unknown');
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: simulation'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /unsettled/); assert.match(visibleText(d), /body-lock/);
});

test('finite-stock studies expose used inventory and independent refusal without inventing mass', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.study.onclick();
  const a = studyRow('hide-A'), b = studyRow('hide-B');
  a.finite_stock = outcome({ markers: [{ id: 'physical-hide', defects_mm: [], placements: [{ instance: 'front' }] }] });
  a.metrics.used_stock_count = 1; a.metrics.used_stock_gross_area_m2 = 0.18;
  a.metrics.finite_stock_external_cut_area_m2 = 0.12; a.metrics.used_stock_mass_kg = null;
  b.finite_stock = refused('inventory material does not match the selected snapshot');
  p.emit(studyReport([a, b]));
  const s = section(p, 'Fixed-pattern manufacturing alternatives');
  const stock = descendants(s).find(n => n.tag === 'table' && visibleText(n).includes('Finite hides and remnants'));
  assert.deepEqual(plain(stock.children[2].children.map(n => n.textContent)),
    ['hide-A', 'Available', '1', '0.18', '0.12', 'Unknown']);
  assert.match(stock.children[3].children[1].textContent, /inventory material does not match/);
  assert.equal(stock.children[3].children.slice(2).every(n => n.textContent === 'Unknown'), true);
  assert.match(visibleText(s), /missing stock width/); // roll failure did not erase the valid hide plan
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('physical stock'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /physical-hide/);
  p.e.stockRequest.handlers.change(); assert.equal(s.hidden, false); // shared study inventory is authoritative
  p.e.studyRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

test('motion displays complete continuation separately from initial static heat and preserves endpoint refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const raw = '{"motion":{"time_s":1.0,"coordinate":-0.0}}';
  p.select('scenarioRequest', raw); await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).scenarioRequest, raw);
  const report = scenarioReport(); report.scenario.scenario = { motion: {} };
  const initial = report.simulations.find(r => r.key === 'A').evaluation.result;
  initial.motion = outcome({ duration_s: 1, samples: [{}, {}], trials: 3, initial_drape_gating: [],
    path_sha256: 'exact-motion-source', metrics: { max_absolute_edge_strain_pct: 7.25 },
    keyframes: [{ time_s: 1, arm_abduction_deg: 40, surface: outcome({ steady: { power_w: -2.5 } }),
      coverage: refused('<missing endpoint coverage>') }], thermal_basis: 'steady snapshots only' });
  p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  const table = label => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith(label));
  assert.equal(table('Initial static-drape').children[2].children[5].textContent, '-4.5');
  assert.deepEqual(plain(table('Prescribed motion').children[2].children.map(n => n.textContent)),
    ['A', 'Available', 'NO — review initial state', '1', '2', '3', '7.25']);
  assert.match(table('Prescribed motion').children[3].children[1].textContent, /missing mechanics/);
  assert.equal(table('Prescribed motion').children[3].children[3].textContent, 'Unknown');
  assert.deepEqual(plain(table('A: endpoint').children[2].children.map(n => n.textContent)),
    ['1', '40', 'Available', '-2.5', 'Refused: <missing endpoint coverage>', 'Unknown']);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: motion identities'));
  d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /exact-motion-source/);
  assert.match(visibleText(s), /not transient heat/); assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
  // A later refusal cannot retain a successful earlier trajectory or its endpoint heat table.
  await p.e.scenario.onclick(); initial.motion = refused('trajectory budget exhausted'); p.emit(report);
  assert.match(visibleText(table('Prescribed motion')), /trajectory budget exhausted/);
  assert.equal(table('A: endpoint'), undefined);
  assert.equal(table('Initial static-drape').children[2].children[5].textContent, '-4.5');
});

test('motion inspector selects exact adaptive states at one physical scale, including accepted-only reports', async () => {
  for (const includeInitial of [true, false]) {
    const p = await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
    const report = scenarioReport(); report.scenario.scenario = { motion: {} };
    const sample = (time_s, x, y) => ({ time_s,
      body_positions_m: [[x,y,0],[x+1,y,0],[x,y,1]], positions_m: [[x,y+1,0],[x+1,y+1,0],[x,y+1,1]] });
    const motion = { duration_s: 0.00375, trials: 2, keyframes: [], triangles: [[0,1,2]], body_triangles: [[0,1,2]],
      samples: [sample(0.001,1,0), sample(0.00375,3,1)] };
    if (includeInitial) motion.initial_sample = sample(0,0,0);
    report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome(motion); p.emit(report);
    const s = section(p, 'Fresh drape and heat alternatives');
    const inspector = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('inspect retained motion samples'));
    assert.equal(descendants(inspector).some(n => n.tag === 'canvas'), false);
    inspector.open = true; inspector.handlers.toggle();
    const slider = descendants(inspector).find(n => n.tag === 'input');
    const canvases = descendants(inspector).filter(n => n.tag === 'canvas');
    const stamp = descendants(inspector).find(n => n.tag === 'output');
    assert.equal(canvases.length, 2); assert.equal(slider.step, '1');
    assert.equal(slider.max, includeInitial ? '2' : '1');
    assert.equal(stamp.textContent, includeInitial ? 'Initial state · t = 0 s' : 'Accepted step 1 of 2 · t = 0.001 s');
    const first = plain(canvases.map(c => c.paint[0]));
    assert.deepEqual(first, includeInitial ? [['move',24,256.5],['move',117,256.5]] : [['move',24,272],['move',86,272]]);
    slider.value = slider.max; slider.handlers.input();
    assert.equal(stamp.textContent, 'Accepted step 2 of 2 · t = 0.00375 s');
    const last = plain(canvases.map(c => c.paint[0]));
    assert.deepEqual(last, includeInitial ? [['move',303,256.5],['move',210,256.5]] : [['move',272,272],['move',210,272]]);
    assert.equal(canvases[0].paint[1][1] - canvases[0].paint[0][1], includeInitial ? 93 : 124);
    inspector.handlers.toggle(); assert.equal(descendants(inspector).filter(n => n.tag === 'canvas').length, 2);
    p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);
    assert.equal(descendants(s).filter(n => n.tag === 'canvas').length, 0);
  }
});

test('invalid retained display geometry or timestamps cannot erase independent scenario outcomes', async () => {
  for (const badTime of [false, true]) {
    const p = await pageModule(); await p.load(); scenarioInputs(p); await p.e.scenario.onclick();
    const report = scenarioReport(); report.scenario.scenario = { motion: {} };
    const sample = { time_s: 0.001, positions_m: [[0,0,0],[1,0,0],[0,0,1]], body_positions_m: [[0,1,0],[1,1,0],[0,1,1]] };
    report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome({ keyframes: [], samples: [sample, sample],
      triangles: [[0,1,badTime ? 2 : 3]], body_triangles: [[0,1,2]] }); p.emit(report);
    const s = section(p, 'Fresh drape and heat alternatives');
    const inspector = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.includes('inspect retained motion samples'));
    inspector.open = true; inspector.handlers.toggle();
    assert.match(visibleText(inspector), badTime ? /physical sample times are invalid/ : /geometry is incomplete/);
    assert.equal(descendants(inspector).some(n => n.tag === 'canvas'), false);
    const initial = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('Initial static-drape'));
    assert.equal(initial.children[2].children[5].textContent, '-4.5');
    assert.match(visibleText(s), /missing mechanics/);
  }
});

test('study output invalidation follows the actual request/body dependencies, not unrelated preview controls', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.study.onclick(); p.emit(studyReport());
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  assert.equal(p.live.size, 3);
  for (const id of ['cutOptions', 'heatConditions', 'fit', 'selfContact', 'quotes']) p.e[id].handlers.change();
  assert.equal(p.live.size, 3);
  p.e.measurements.handlers.change(); assert.equal(p.live.size, 1);
  assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
  assert.equal(section(p, 'Fixed-pattern manufacturing alternatives').hidden, false);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.e.scenarioRequest.handlers.change(); assert.equal(p.live.size, 1);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size, 0);
  await p.e.study.onclick(); p.emit(studyReport()); await p.e.scenario.onclick(); p.emit(scenarioReport());
  await p.load(); assert.equal(p.live.size, 0); // no earlier-pattern results remain
});

test('study replacement/cancellation cannot publish late file reads or stale scenario success', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  await p.e.scenario.onclick(); p.emit(scenarioReport()); assert.equal(p.live.size, 2);
  const d = deferred(); p.e.scenarioRequest.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.scenario.onclick(); assert.equal(p.live.size, 0);
  p.e.cancel.onclick(); p.ready(); d.resolve('{}'); await running;
  assert.equal(old.messages.filter(m => m.type === 'scenario').length, 1);
  scenarioInputs(p); await p.e.scenario.onclick(); const stale = p.current(), id = stale.messages.at(-1).request;
  p.e.scenarioRequest.handlers.change(); p.ready();
  stale.emit({ type: 'result', request: id, result: scenarioReport(), files: [] });
  assert.equal(p.live.size, 0);
  await p.e.scenario.onclick(); p.emit(scenarioReport());
  p.select('scenarioRequest', '{malformed'); await p.e.scenario.onclick();
  assert.equal(p.live.size, 0); assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
  assert.equal(p.e.error.hidden, false);
});

test('computed objectives display Rust admission, signed units and incomparability without a browser scorer', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const comparison = {
    policy: { basis: 'nominal', allow_mixed_protocols: false,
      objectives: [{ metric: 'independent_surface_power_w', direction: 'minimize' }], constraints: [] },
    frontier: ['A'], motion_required: false, uncertainty_propagated: false,
    alternatives: [
      { key: 'A', eligible: true, dominated_by: [], exclusions: [], readings: { independent_surface_power_w: { value: -8, unit: 'W' } } },
      { key: 'B', eligible: false, dominated_by: [], exclusions: [{ message: 'not settled' }], readings: {} }],
    incomparable: [{ alternatives: ['A', '<C>'], reason: 'Different conditioning' }]
  };
  await p.e.scenario.onclick();
  const report = scenarioReport(); report.comparison = comparison;
  p.emit(report);
  const s = section(p, 'Fresh drape and heat alternatives');
  assert.match(visibleText(s), /Computed nominal frontier: A/);
  assert.match(visibleText(s), /Reported input protocols must match/);
  assert.match(visibleText(s), /does not require motion completion/);
  assert.match(visibleText(s), /A \/ <C>: Different conditioning/);
  const t = descendants(s).find(n => n.tag === 'table');
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)), ['A', 'Eligible', 'None reported', '-8 W']);
  assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)), ['B', 'Excluded: not settled', 'None reported', 'Unknown']);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);
  await p.e.study.onclick();
  const study = studyReport(); study.comparison = { ...comparison, frontier: [], policy: { ...comparison.policy, allow_mixed_protocols: true } };
  p.emit(study);
  const m = section(p, 'Fixed-pattern manufacturing alternatives');
  assert.match(visibleText(m), /No eligible non-dominated alternatives/);
  assert.match(visibleText(m), /Mixed source protocols explicitly admitted/);

  // A refused motion cannot look qualified merely because static heat is available. Display the
  // Rust frontier and signed, domain-specific readings verbatim, including below-zero temperatures.
  comparison.motion_required = true;
  comparison.policy = { basis: 'nominal', allow_mixed_protocols: false, constraints: [],
    required_consumers: ['transient_coverage'], objectives: [
      { metric: 'transient_surface_net_boundary_energy_j', direction: 'minimize' },
      { metric: 'transient_surface_final_min_temperature_c', direction: 'maximize' }] };
  comparison.alternatives[0].readings = {
    transient_surface_net_boundary_energy_j: { value: -3, unit: 'J', consumer: 'transient_surface' },
    transient_surface_final_min_temperature_c: { value: -5, unit: 'C', consumer: 'transient_surface' } };
  comparison.alternatives[1].exclusions = [{ message: 'complete motion coverage history required' }];
  await p.e.scenario.onclick(); p.emit(report);
  assert.match(visibleText(s), /Motion-qualified comparison/);
  const motionTable = descendants(s).find(n => n.tag === 'table');
  assert.deepEqual(plain(motionTable.children[2].children.map(n => n.textContent)),
    ['A', 'Eligible', 'None reported', '-3 J', '-5 C']);
  assert.deepEqual(plain(motionTable.children[3].children.map(n => n.textContent)),
    ['B', 'Excluded: complete motion coverage history required', 'None reported', 'Unknown', 'Unknown']);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true);
});

test('reported mass comparisons display admitted ranges and unresolved alternatives without nominal reranking', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const comparison = {
    policy: { basis: 'reported_bounds', allow_mixed_protocols: false,
      objectives: [{ metric: 'assembly_mass_kg', direction: 'minimize' }],
      constraints: [{ metric: 'assembly_mass_kg', maximum: 0.22 }] },
    frontier: ['A', 'B'], motion_required: false, uncertainty_propagated: false,
    alternatives: [
      { key: 'A', eligible: true, dominated_by: [], exclusions: [],
        readings: { assembly_mass_kg: { value: 0.2, unit: 'kg', reported_bounds: [0.19, 0.21] } } },
      { key: 'B', eligible: true, dominated_by: [], exclusions: [],
        readings: { assembly_mass_kg: { value: 0.21, unit: 'kg', reported_bounds: [0.2, 0.22] } } },
      { key: 'C', eligible: false, dominated_by: [], exclusions: [{ message: 'No complete reported bounds' }],
        readings: { assembly_mass_kg: { value: 0.18, unit: 'kg' } } }],
    incomparable: [{ alternatives: ['A', 'B'], reason: 'Reported mass ranges overlap; no strict dominance' }]
  };
  const manufacturing = studyReport(['A', 'B', 'C'].map(key => studyRow(key)));
  for (const op of ['study', 'scenario']) {
    await p.e[op].onclick();
    p.emit(op === 'study' ? { ...manufacturing, comparison } : { ...scenarioReport(), manufacturing, comparison });
    const s = section(p, op === 'study' ? 'Fixed-pattern manufacturing alternatives' : 'Fresh drape and heat alternatives');
    assert.match(visibleText(s), /Computed reported_bounds frontier: A, B/);
    assert.match(visibleText(s), /complete reported mass ranges at fixed authored geometry and inventory/);
    assert.match(visibleText(s), /Declared assembly mass ranges evaluated by Rust/);
    assert.doesNotMatch(visibleText(s), /Declared nominal objectives/);
    assert.match(visibleText(s), /A \/ B: Reported mass ranges overlap/);
    const t = descendants(s).find(n => n.tag === 'table');
    assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
      ['A', 'Eligible', 'None reported', '0.2 kg; reported range [0.19, 0.21] kg']);
    assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
      ['B', 'Eligible', 'None reported', '0.21 kg; reported range [0.2, 0.22] kg']);
    assert.deepEqual(plain(t.children[4].children.map(n => n.textContent)),
      ['C', 'Excluded: No complete reported bounds', 'None reported', '0.18 kg; reported range unknown']);
  }
});

test('assembly mass and finite stock retain independent totals and refusals in manufacturing and scenario studies', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const a = studyRow('complete'), b = studyRow('unknown'), c = studyRow('refused');
  a.metrics.stock_mass_kg = 9;
  a.assembly_mass = outcome({ request: { inventory_complete: true, extras: [{ id: '<retained source>' }] },
    sheet_mass: { kg: 1 }, assembly_mass: { kg: 1.2, reported_bounds_kg: [1.1, 1.3] } });
  b.assembly_mass = outcome({ request: { inventory_complete: false }, sheet_mass: { kg: 1 },
    assembly_mass: { kg: null, reported_bounds_kg: null, unavailable: { message: 'extra inventory incomplete' } } });
  c.assembly_mass = refused('cutout geometry refused');
  a.finite_stock = outcome({ markers: [{ id: 'complete-hide' }] });
  Object.assign(a.metrics, { used_stock_count: 1, used_stock_gross_area_m2: 2,
    finite_stock_external_cut_area_m2: 0.5, used_stock_mass_kg: 7 });
  b.finite_stock = refused('physical inventory mismatch');
  c.finite_stock = outcome({ markers: [{ id: 'untrimmed-remnant' }] });
  Object.assign(c.metrics, { used_stock_count: 1, used_stock_gross_area_m2: 1,
    finite_stock_external_cut_area_m2: 0.25, used_stock_mass_kg: 4 });
  const report = studyReport([a, b, c]);
  for (const op of ['study', 'scenario']) {
    await p.e[op].onclick();
    p.emit(op === 'study' ? report : { ...scenarioReport(), manufacturing: report });
    const s = section(p, op === 'study' ? 'Fixed-pattern manufacturing alternatives' : 'Fresh drape and heat alternatives');
    const t = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Declared assembly mass — complete or unknown');
    assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
      ['complete', 'Complete declared inventory', '1', '1.2', '[1.1, 1.3]', 'Declared complete']);
    assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
      ['unknown', 'Report available; total unknown', '1', 'Unknown', 'Not reported', 'Incomplete']);
    assert.equal(t.children[4].children[1].textContent, 'Refused: cutout geometry refused');
    const manufacturing = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Independent manufacturing outcomes');
    assert.equal(manufacturing.children[2].children[7].textContent, '9');
    const stock = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'Finite hides and remnants — used physical inventory');
    assert.deepEqual(plain(stock.children[2].children.map(n => n.textContent)),
      ['complete', 'Available', '1', '2', '0.5', '7']);
    assert.equal(stock.children[3].children[1].textContent, 'Refused: physical inventory mismatch');
    assert.deepEqual(plain(stock.children[4].children.map(n => n.textContent)),
      ['refused', 'Available', '1', '1', '0.25', '4']);
    assert.match(visibleText(s), /not confidence intervals/);
    const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('complete: cut-piece mass'));
    d.open = true; d.handlers.toggle(); assert.match(visibleText(d), /<retained source>/);
    assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  }
  assert.equal(p.live.size, 6);
  p.e.studyRequest.handlers.change(); assert.equal(p.live.size, 0);
  assert.equal(section(p, 'Fixed-pattern manufacturing alternatives').hidden, true);
  assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
});

// Opt in after tools/build_web.sh. This executes the real production WASM through the actual
// worker module, with only the Node transport/module loader adapted. It is not HTTP/browser or
// native parity, successful anatomical body fitting, or empirical textile validation.
test('compiled WASM studies preserve source replay and optional model-backed motion equality',
  { skip: process.env.FTX_REAL_WASM !== '1' }, async () => {
    const pkg = path.join(__dirname, '../web/pkg');
    assert.ok(fs.existsSync(path.join(pkg, 'ftx_wasm_bg.wasm')), 'Build web/pkg before FTX_REAL_WASM=1');
    const ftx = await import(require('node:url').pathToFileURL(path.join(pkg, 'ftx_wasm.js')).href);
    ftx.initSync({ module: fs.readFileSync(path.join(pkg, 'ftx_wasm_bg.wasm')) });
    const w = await workerModule({ bindings: ftx });
    const fixture = n => fs.readFileSync(path.join(__dirname, '../tests/fixtures/materials', n), 'utf8');
    for (const admission of [false, true]) {
      const calibration = JSON.parse(ftx.material_calibrate_nonlinear(fixture('synthetic-nonlinear-swatch.json'), admission));
      assert.equal(calibration.material.capabilities.mechanical_coefficients.ready, admission);
      assert.deepEqual(calibration.material, JSON.parse(ftx.material_inspect(calibration.material.selection_json)));
    }
    for (const [mode, filename] of [['nonlinear','synthetic-nonlinear-swatch.json'],
      ['relaxation','synthetic-relaxation-history.json'], ['relaxation','synthetic-orthotropic-relaxation-history.json'],
      ['relaxation','synthetic-orthotropic-relaxation-search.json'],
      ['membrane_plasticity','synthetic-membrane-plasticity-calibration.json']]) {
      const calibrationRequest = fixture(filename);
      for (const allowEstimates of [false, true]) {
        const identified = await w.send({type:'calibrate',calibrationMode:mode,calibrationRequest,allowEstimates});
        assert.equal(identified.type,'result',identified.message);
        const expected = ftx[`material_calibrate_${mode}`](calibrationRequest,allowEstimates);
        assert.equal(identified.files[0].text,expected);
        assert.equal(identified.files[1].text,identified.result.selection_json);
        assert.equal(identified.files[2].text,calibrationRequest);
        assert.deepEqual(JSON.parse(ftx.material_inspect(identified.result.selection_json)),identified.result.material);
        assert.equal(identified.result.selection.allow_estimates,allowEstimates);
        assert.equal(identified.result.material.capabilities.mechanical_coefficients.ready,allowEstimates);
        if (JSON.parse(calibrationRequest).orthotropic?.time_bounds_seconds?.length) {
          const c=identified.result.calibration;
          assert.ok(c.orthotropic_fit.time_search.starts_converged>0);
          assert.equal(c.orthotropic_fit.time_search.starts_attempted,c.starts_attempted);
        }
        const changes = `{"schema":"ftx-material-bindings/1","default":${identified.result.selection_json}}`;
        const binding = await w.send({type:'bind',pattern:fixture('synthetic-sewing-pattern.json'),changes});
        assert.equal(binding.type,'result',binding.message);
        assert.equal(binding.result.geometry_changed,false);
        assert.equal(binding.result.pattern.fabric.material.expected_sha256,identified.result.selection.expected_sha256);
        assert.equal(binding.result.drape_materials_with_self_contact.ready,allowEstimates);
      }
    }
    const compressionRequest = fixture('synthetic-compression.json');
    const prepared = await w.send({ type: 'pressure', compressionRequest });
    assert.equal(prepared.type, 'result', prepared.message);
    const pressureReport = ftx.material_pressure_condition(compressionRequest);
    assert.equal(prepared.files[0].text, pressureReport);
    assert.equal(prepared.files[1].text, prepared.result.selection_json);
    assert.equal(prepared.files[2].text, compressionRequest);
    assert.equal(prepared.result.inspection.capabilities.steady_conduction.ready, true);
    assert.equal(prepared.result.inspection.capabilities.transient_thermal.ready, true);
    assert.equal(prepared.result.inspection.capabilities.sewing_thickness_m.ready, false);
    assert.equal(prepared.result.material.properties.thickness.value, 0.015);
    assert.equal(prepared.result.material.properties.thermal_resistance.value, 0.375);
    assert.equal((await w.send({ type: 'pressure', compressionRequest })).files[0].text, pressureReport);
    if (process.env.FTX_PRESSURE_REPLAY) {
      assert.deepEqual(JSON.parse(pressureReport), JSON.parse(fs.readFileSync(process.env.FTX_PRESSURE_REPLAY, 'utf8')));
    }
    const screenRequest = fixture('screen-insulation.json');
    const screened = await w.send({ type: 'screen', screenRequest });
    assert.equal(screened.type, 'result', screened.message);
    assert.deepEqual(screened.result, JSON.parse(ftx.material_screen(screenRequest)));
    assert.equal(screened.files[1].text, screenRequest);
    const knitRequest = JSON.stringify({ schema:'ftx-material-screen/1', basis:'nominal', query:'sperl2022/',
      forms:['knit'], allow_mixed_protocols:true,
      constraints:[{attribute:{key:'strip_force_30pct_weft',unit:'N',conditions:{branch:'loading',
        free_gauge_length_mm:'100',reference_strip_width_mm:'60'}},minimum:2}],
      objectives:[{attribute:{key:'areal_density',unit:'kg/m2'},direction:'minimize'}] });
    const knits = await w.send({type:'screen',screenRequest:knitRequest});
    assert.equal(knits.type,'result',knits.message);
    assert.equal(knits.files[0].text,ftx.material_screen(knitRequest));
    assert.deepEqual(knits.result.frontier,['sperl2022/SJ3']);
    assert.equal(knits.result.alternatives.filter(row => row.key.startsWith('sperl2022/')).length,3);
    const knitWinner = knits.result.alternatives.find(row => row.key === 'sperl2022/SJ3');
    assert.equal(knitWinner.readings.strip_force_30pct_weft.value,2.48);
    const knitInspection = JSON.parse(ftx.material_inspect(JSON.stringify(knitWinner.selection)));
    assert.equal(knitInspection.capabilities.mechanical_coefficients.ready,false);
    assert.equal(knitInspection.capabilities.steady_conduction.ready,false);
    assert.equal(knitInspection.capabilities.sewing_thickness_m.ready,false);
    const pattern = fixture('synthetic-sewing-pattern.json');
    const pressureChanges = `{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json}}`;
    const pressureBinding = await w.send({ type: 'bind', pattern, changes: pressureChanges });
    assert.equal(pressureBinding.type, 'result', pressureBinding.message);
    assert.equal(pressureBinding.result.geometry_changed, false);
    assert.equal(pressureBinding.result.drape_materials_with_self_contact.ready, true);
    assert.equal(pressureBinding.result.pattern.fabric.material.expected_sha256, prepared.result.selection.expected_sha256);
    // Different loaded-state selections require their own explicitly qualified interface.
    const highInput=JSON.parse(compressionRequest); highInput.pressure_pa=500;
    const high=JSON.parse(ftx.material_pressure_condition(JSON.stringify(highInput)));
    const pair={materials:[prepared.result.selection.expected_sha256,high.selection.expected_sha256],
      friction:{value:0.18,unit:'1',evidence:'estimate',source:'test',method:'Synthetic software pair observation'},
      sources:{test:{reference:'test:compiled-interface',accessed_on:'2026-10-19',note:'Synthetic test only'}},allow_estimates:true};
    const interfaceChanges=`{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json},"pieces":{"back":${high.selection_json}},"interfaces":${JSON.stringify([pair])}}`;
    const paired=await w.send({type:'bind',pattern,changes:interfaceChanges});
    assert.equal(paired.type,'result',paired.message);
    assert.equal(paired.result.drape_materials_with_self_contact.ready,true);
    assert.deepEqual(paired.result.pattern.fabric.interfaces,[pair]);
    assert.equal(paired.files[2].text,interfaceChanges);
    const pairedStudy=`{"schema":"ftx-material-study/1","variants":[{"key":"pair","changes":${interfaceChanges}}],"cutting":{"copies":1,"gap_mm":2,"edge_margin_mm":0,"end_margin_mm":0}}`;
    const pairRow=JSON.parse(ftx.material_study(pattern,pairedStudy)).alternatives[0];
    assert.deepEqual(pairRow.frozen_changes.interfaces,[pair]);
    assert.equal(pairRow.binding.result.pattern_json,paired.result.pattern_json);
    assert.equal(JSON.parse(ftx.material_bind(pattern,JSON.stringify(pairRow.frozen_changes))).pattern_json,paired.result.pattern_json);
    const combinationPattern = pressureBinding.result.pattern_json;
    const combinationGroups = ['front', 'back'].map(piece => `{"key":"${piece}","target":{"scope":"pieces","ids":["${piece}"]},"choices":[{"key":"low","selection":${prepared.result.selection_json}},{"key":"high","selection":${high.selection_json}}]}`).join(',');
    const combinationRequest = pool => `{"schema":"ftx-material-study/1","combinations":{"groups":[${combinationGroups}],"interfaces":${JSON.stringify(pool)}},"cutting":{"copies":1,"gap_mm":2,"edge_margin_mm":0,"end_margin_mm":0}}`;
    for (const supplied of [false, true]) {
      const request = combinationRequest(supplied ? [pair] : []);
      const combined = await w.send({ type:'study', pattern:combinationPattern, studyRequest:request });
      assert.equal(combined.type,'result',combined.message);
      assert.equal(combined.files[0].text,ftx.material_study(combinationPattern,request));
      assert.equal(combined.files[2].text,request);
      assert.equal(combined.result.alternatives.length,4);
      for (const row of combined.result.alternatives) {
        const mixed = row.choices.front !== row.choices.back;
        assert.equal(row.binding.status,'available');
        const replay = JSON.parse(ftx.material_bind(combinationPattern,JSON.stringify(row.frozen_changes)));
        assert.equal(replay.drape_materials_with_self_contact.ready,!mixed || supplied);
        assert.deepEqual(row.frozen_changes.interfaces,mixed && supplied ? [pair] : []);
        assert.equal(replay.pattern_json,row.binding.result.pattern_json);
      }
    }
    pair.allow_estimates=false;
    // Replace only the pair policy; each material keeps its independently authored consent.
    const deniedChanges=`{"schema":"ftx-material-bindings/1","default":${prepared.result.selection_json},"pieces":{"back":${high.selection_json}},"interfaces":${JSON.stringify([pair])}}`;
    const deniedInterface=await w.send({type:'bind',pattern,changes:deniedChanges});
    assert.equal(deniedInterface.type,'result',deniedInterface.message);
    assert.equal(deniedInterface.result.drape_materials_with_self_contact.ready,false);
    const variants = [['a-wide', 0.1, 0.4], ['b-narrow', 0.06, 2], ['c-unknown-width', null, 0.4]].map(([key, width, thickness]) => {
      const selection = JSON.parse(pattern).fabric.material;
      for (const [name, value, unit] of [['width', width, 'm'], ['thickness', thickness, 'mm'], ['areal_density', 0.2, 'kg/m2']]) {
        if (value !== null) selection.record.properties[name] = { value, unit, evidence: 'estimate', source: 'test',
          method: 'Synthetic software test; not measured textile data' };
      }
      return { key, changes: { schema: 'ftx-material-bindings/1', default: selection } };
    });
    const studyRequest = JSON.stringify({ schema: 'ftx-material-study/1', variants,
      cutting: { copies: 1, gap_mm: 2, edge_margin_mm: 0, end_margin_mm: 0 },
      sewing: JSON.parse(fixture('synthetic-sewing-options.json')), purchasing: { quotes: { schema: 'ftx-textile-quotes/1', quotes: [] },
        market: { currency: 'USD', region: 'US', quality: 'test', terms: 'test', as_of: '2026-10-15', max_age_days: 30 } } });
    const study = await w.send({ type: 'study', pattern, studyRequest });
    assert.equal(study.type, 'result', study.message);
    assert.deepEqual(study.result, JSON.parse(ftx.material_study(pattern, studyRequest)));
    assert.equal(study.files[1].text, pattern); assert.equal(study.files[2].text, studyRequest);
    const rows = study.result.alternatives;
    assert.equal(rows.length, 3); assert.ok(rows.every(r => r.binding.status === 'available'));
    assert.equal(rows[0].cutting.status, 'available'); assert.equal(rows[1].cutting.status, 'available');
    assert.ok(rows[1].cutting.result.rolls[0].length_m > rows[0].cutting.result.rolls[0].length_m);
    assert.equal(rows[0].sewing.status, 'available'); assert.equal(rows[1].sewing.status, 'refused');
    assert.equal(rows[2].cutting.status, 'refused'); assert.equal(rows[2].sewing.status, 'available');
    assert.ok(rows.every(r => r.metrics.least_observed_independent_order_total === null));
    for (const row of rows) {
      const replay = JSON.parse(ftx.material_bind(pattern, JSON.stringify(row.frozen_changes)));
      assert.equal(replay.pattern_json, row.binding.result.pattern_json);
    }
    assert.equal((await w.send({ type: 'study', pattern, studyRequest })).files[0].text, study.files[0].text);
    const compared = JSON.parse(studyRequest);
    compared.comparison = { basis: 'nominal', objectives: [{ metric: 'stock_mass_kg', direction: 'minimize' }], required_consumers: ['sewing'] };
    const comparisonRequest = JSON.stringify(compared);
    const comparison = await w.send({ type: 'study', pattern, studyRequest: comparisonRequest });
    assert.equal(comparison.type, 'result', comparison.message);
    assert.deepEqual(comparison.result.alternatives, study.result.alternatives);
    assert.deepEqual(comparison.result, JSON.parse(ftx.material_study(pattern, comparisonRequest)));
    assert.deepEqual(comparison.result.comparison.frontier, ['a-wide']);
    assert.equal(comparison.result.comparison.uncertainty_propagated, false);
    assert.equal(comparison.result.comparison.alternatives[0].readings.stock_mass_kg.value, rows[0].metrics.stock_mass_kg);
    assert.equal(comparison.files[2].text, comparisonRequest);
    compared.comparison.objectives = [{ metric: 'observed_order_cost', direction: 'minimize' }];
    const noOffers = await w.send({ type: 'study', pattern, studyRequest: JSON.stringify(compared) });
    assert.equal(noOffers.type, 'result', noOffers.message);
    assert.deepEqual(noOffers.result.comparison.frontier, []);
    // Authored, unlocked synthetic alternatives; source-locked results never go through stringify.
    const massOnly = { schema: 'ftx-material-study/1', variants: JSON.parse(studyRequest).variants,
      assembly_mass: JSON.parse(fixture('study-assembly-mass.json')).assembly_mass,
      comparison: { basis: 'nominal', objectives: [{ metric: 'assembly_mass_kg', direction: 'minimize' }] } };
    const densities = [0.2, 0.4, 0.1];
    massOnly.variants.forEach((v, i) => { v.changes.default.record.properties.areal_density.value = densities[i]; });
    const massRequest = JSON.stringify(massOnly);
    const mass = await w.send({ type: 'study', pattern, studyRequest: massRequest });
    assert.equal(mass.type, 'result', mass.message);
    assert.deepEqual(mass.result, JSON.parse(ftx.material_study(pattern, massRequest)));
    assert.deepEqual(mass.result.comparison.frontier, ['c-unknown-width']);
    for (const [i, row] of mass.result.alternatives.entries()) {
      assert.equal(row.cutting.status, 'not_requested');
      assert.equal(row.assembly_mass.status, 'available');
      assert.ok(Math.abs(row.metrics.assembly_mass_kg - (0.0096 * densities[i] + 0.015)) < 1e-12);
      assert.equal(row.assembly_mass.result.pattern_sha256, row.binding.result.pattern_sha256);
    }
    assert.equal(mass.files[2].text, massRequest);
    assert.equal((await w.send({ type: 'study', pattern, studyRequest: massRequest })).files[0].text, mass.files[0].text);
    massOnly.assembly_mass.extras[0].mass = null;
    const unknown = await w.send({ type: 'study', pattern, studyRequest: JSON.stringify(massOnly) });
    assert.equal(unknown.type, 'result', unknown.message);
    assert.deepEqual(unknown.result.comparison.frontier, []);
    assert.ok(unknown.result.alternatives.every(r => r.assembly_mass.result.sheet_mass.kg > 0 &&
      r.assembly_mass.result.assembly_mass.kg === null && r.metrics.assembly_mass_kg === undefined));
    const invalid = await w.send({ type: 'scenario', pattern, studyRequest, scenarioRequest: '{}', measurements: '{}', bytes: new ArrayBuffer(8) });
    assert.equal(invalid.type, 'error'); assert.match(invalid.message, /ftxbody/);
    // Optional positive source-model motion replay from the existing real-binary fixture producer.
    // It is synthetic geometry, not private wearer data or an anatomical acceptance experiment.
    if (process.env.FTX_MOTION_REPLAY) {
      const dir = process.env.FTX_MOTION_REPLAY;
      const input = name => fs.readFileSync(path.join(dir, name), 'utf8');
      const bytes = fs.readFileSync(path.join(dir, 'model.ftxbody'));
      const msg = { type: 'scenario', request: 93, pattern: input('pattern.json'), studyRequest: input('study.json'),
        scenarioRequest: input('scenario.json'), measurements: input('body-input.json'),
        bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
      const result = await w.send(msg);
      assert.equal(result.type, 'result', result.message);
      assert.deepEqual(result.result, JSON.parse(input('motion-report.json')));
      assert.equal(result.files.find(f => f.name === 'scenario-request.json').text, msg.scenarioRequest);
      assert.equal(result.files.find(f => f.name === 'body-input.json').text, msg.measurements);
      const a = result.result.simulations[0].evaluation.result.motion;
      const b = result.result.simulations[1].evaluation.result.motion;
      assert.equal(a.status, 'available'); assert.equal(b.status, 'available');
      assert.deepEqual(a.result.samples, b.result.samples);
      assert.notEqual(a.result.keyframes[1].surface.result.steady.power_w, b.result.keyframes[1].surface.result.steady.power_w);
      for (const name of ['transient_surface', 'transient_coverage']) {
        if (!JSON.parse(msg.scenarioRequest).motion[name]) continue;
        const h = a.result[name]; assert.equal(h.status, 'available');
        assert.equal(b.result[name].status, 'available');
        assert.equal(h.result.duration_s,a.result.duration_s);
        assert.equal(h.result.path_sha256,a.result.path_sha256);
        assert.equal(h.result.trace.length,a.result.samples.length);
        assert.ok(Math.abs(h.result.balance_error_j)<1e-8);
      }
      if (a.result.transient_surface) assert.notDeepEqual(a.result.transient_surface.result.final.temperature_c,
        b.result.transient_surface.result.final.temperature_c);
      if (a.result.transient_coverage) assert.notDeepEqual(a.result.transient_coverage.result.checkpoint.exposed_temperature_c,
        b.result.transient_coverage.result.checkpoint.exposed_temperature_c);
      assert.equal((await w.send(msg)).files[0].text, result.files[0].text);
      assert.ok(w.sent.some(m => m.type === 'progress' && m.request === 93 && m.text.includes('motion endpoint')));
      // Exhaust only the aggregate motion allowance; every independent static result survives.
      const limited = JSON.parse(msg.scenarioRequest); limited.max_motion_trials = 1;
      const exhausted = await w.send({ ...msg, request: 94, scenarioRequest: JSON.stringify(limited) });
      assert.equal(exhausted.type, 'result', exhausted.message); assert.equal(exhausted.request, 94);
      assert.deepEqual(exhausted.result.manufacturing, result.result.manufacturing);
      assert.equal(exhausted.result.simulations.length, result.result.simulations.length);
      exhausted.result.simulations.forEach((row, i) => {
        assert.equal(row.evaluation.status, 'available');
        const actual = row.evaluation.result, original = result.result.simulations[i].evaluation.result;
        assert.equal(actual.motion.status, 'refused'); assert.equal(actual.motion.result, undefined);
        for (const key of ['drape', 'surface', 'coverage', 'positions_m', 'positions_sha256', 'metrics']) {
          assert.deepEqual(actual[key], original[key], `Motion allowance changed independent ${key}`);
        }
      });
    }
    // Separate native fixture keeps the original uncoupled motion replay intact. All material
    // observations and body geometry here are synthetic software inputs, not measured textiles.
    if (process.env.FTX_PLASTIC_HEAT_REPLAY) {
      const dir = process.env.FTX_PLASTIC_HEAT_REPLAY;
      const input = name => fs.readFileSync(path.join(dir, name), 'utf8');
      const bytes = fs.readFileSync(path.join(dir, 'model.ftxbody'));
      const msg = { type: 'scenario', request: 95, pattern: input('pattern.json'), studyRequest: input('study.json'),
        scenarioRequest: input('scenario.json'), measurements: input('body-input.json'),
        bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
      const replay = await w.send(msg);
      assert.equal(replay.type, 'result', replay.message);
      assert.deepEqual(replay.result, JSON.parse(input('motion-report.json')));
      assert.equal(replay.files.find(f => f.name === 'scenario-request.json').text, msg.scenarioRequest);
      assert.equal(replay.files.find(f => f.name === 'body-input.json').text, msg.measurements);
      assert.equal(replay.result.simulations.length, 2);
      assert.deepEqual(replay.result.comparison.frontier, ['unconverted']);
      assert.deepEqual(replay.result.comparison.incomparable, []);
      assert.ok(replay.result.comparison.alternatives.every(row => row.eligible));
      const motions = new Map(replay.result.simulations.map(row => {
        assert.equal(row.evaluation.status, 'available');
        const result = row.evaluation.result;
        assert.equal(result.drape.settled, true); assert.deepEqual(result.drape.gating, []);
        assert.equal(result.metrics.simulation_gating_clear, true);
        assert.equal(result.motion.status, 'available');
        const motion = result.motion.result;
        assert.equal(motion.transient_surface.status, 'available');
        const heat = motion.transient_surface.result, work = heat.plastic_dissipation_heat;
        assert.equal(work.distribution, 'uniform_reference_thickness');
        assert.equal(work.temporal_distribution, 'uniform_over_accepted_interval');
        assert.ok(work.initial_plastic_dissipation_j > 0 && work.plastic_dissipation_j > 0);
        assert.ok(Math.abs(work.initial_plastic_dissipation_j -
          result.drape.membrane_plasticity_history.plastic_dissipation_j) < 1e-12);
        assert.ok(Math.abs(work.initial_plastic_dissipation_j + work.plastic_dissipation_j -
          motion.membrane_plasticity_history.plastic_dissipation_j) < 1e-12);
        assert.ok(['unconverted', 'converted'].includes(row.key));
        const fraction = row.key === 'unconverted' ? 0 : 0.8;
        assert.equal(work.material_fractions[0].fraction, fraction);
        assert.ok(Math.abs(work.supplied_heat_j - fraction * work.plastic_dissipation_j) < 1e-12);
        assert.ok(Math.abs(work.supplied_heat_j + work.unconverted_dissipation_j - work.plastic_dissipation_j) < 1e-12);
        const balance = heat.stored_change_j - heat.net_boundary_energy_j - work.supplied_heat_j;
        assert.ok(Math.abs(balance) < 1e-9);
        assert.ok(Math.abs(balance - heat.balance_error_j) < 1e-12);
        if (fraction === 0) {
          assert.equal(work.supplied_heat_j, 0);
          assert.ok(heat.final.temperature_c.flat().every(t => Math.abs(t - 20) < 1e-10));
        } else assert.ok(work.supplied_heat_j > 0);
        assert.equal(heat.trace.length, motion.samples.length);
        assert.equal(motion.transient_coverage.status, 'available');
        const coverage = motion.transient_coverage.result;
        assert.deepEqual(coverage.plastic_dissipation_heat, work);
        assert.equal(coverage.trace.length, motion.samples.length);
        for (const value of [coverage, ...coverage.trace]) {
          const residual = value.physical_stored_change_j - value.net_boundary_energy_j -
            value.plastic_dissipation_heat.supplied_heat_j;
          assert.ok(Math.abs(residual) < 1e-9);
          assert.ok(Math.abs(residual - value.balance_error_j) < 1e-12);
        }
        return [row.key, motion];
      }));
      const cold = motions.get('unconverted'), warm = motions.get('converted');
      assert.deepEqual(cold.samples, warm.samples);
      assert.notDeepEqual(cold.transient_surface.result.final.temperature_c,
        warm.transient_surface.result.final.temperature_c);
      assert.notDeepEqual(cold.transient_coverage.result.checkpoint.exposed_temperature_c,
        warm.transient_coverage.result.checkpoint.exposed_temperature_c);
    }
  });

test('explicit body sources replace only scenario evidence and retain exact replay text', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const raw = '{"schema":"ftx-scenario-body/1","arm_abduction_deg":50.0,"accepted_body_fit":{"test":-0.0}}';
  p.select('scenarioBody', raw); p.e.measurements.files = [];
  p.e.scenarioBody.handlers.change(); assert.equal(p.e.scenario.disabled, false);
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).measurements, raw);
  p.emit(scenarioReport()); assert.equal(p.live.size, 2);
  p.select('measurements', '{"wearer":{"stature_mm":1000.0}}'); p.e.measurements.handlers.change();
  assert.equal(p.live.size, 2); // overridden tapes are not a dependency of this result
  p.e.scenarioBody.handlers.change(); assert.equal(p.live.size, 0);
  p.e.scenarioBody.files = []; p.e.scenarioBody.handlers.change();
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.at(-1).measurements, '{"wearer":{"stature_mm":1000.0}}');
  const w = await workerModule();
  const r = await w.send({type:'scenario',request:55,bytes:new ArrayBuffer(8),
    pattern:'{}',studyRequest:'{}',scenarioRequest:'{}',measurements:raw});
  const artifact = r.files.find(f => f.name === 'body-input.json');
  assert.equal(artifact.text, raw); assert.equal(w.called.find(c => c[0] === 'scenario').at(-1), raw);
});

test('invalid or superseded body-source files cannot fall back to tapes or publish stale outcomes', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  p.select('scenarioBody', '{malformed'); p.e.scenarioBody.handlers.change();
  const oldCount = p.current().messages.length;
  await p.e.scenario.onclick();
  assert.equal(p.current().messages.length, oldCount); assert.equal(p.e.error.hidden, false);
  assert.equal(p.live.size, 0);
  const d = deferred(); p.select('scenarioBody', '{}'); p.e.scenarioBody.files[0].text = () => d.promise;
  const old = p.current(), running = p.e.scenario.onclick();
  p.select('scenarioBody', '{"schema":"ftx-scenario-body/1"}'); p.e.scenarioBody.handlers.change();
  p.ready(); d.resolve('{"schema":"ftx-scenario-body/1","arm_abduction_deg":35.0}'); await running;
  assert.equal(old.messages.length, oldCount);
  await p.e.scenario.onclick(); const stale = p.current(), id = stale.messages.at(-1).request;
  p.e.body.handlers.change(); p.ready();
  stale.emit({type:'result',request:id,result:scenarioReport(),files:[]});
  assert.equal(p.live.size, 0); assert.equal(section(p, 'Fresh drape and heat alternatives').hidden, true);
});

test('transient heat displays signed energy and remap separately, preserving refusal and invalidation', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const r = scenarioReport();
  r.scenario = { scenario: { motion: { transient_surface: {}, transient_coverage: {} } } };
  const motion = { duration_s: 1, samples: [{}], trials: 1, keyframes: [], metrics: {},
    transient_surface: outcome({ duration_s: 1, thermal_steps: 2, trace: [{temperature_range_c:[-5,0]}],
      net_boundary_energy_j: -3, stored_change_j: -3, balance_error_j: 0 }),
    transient_coverage: outcome({ duration_s: 1, thermal_steps: 4, trace: [{temperature_range_c:[0,5]}],
      net_boundary_energy_j: 4, physical_stored_change_j: 4, balance_error_j: 0, signed_remap_residual_j: -1e-12,
      checkpoint: { source: '<synthetic locked source>' } }) };
  r.simulations = [{key:'A',evaluation:outcome({drape:{settled:false},motion:outcome(motion)})},
    {key:'B',evaluation:outcome({drape:{settled:true},motion:refused('motion exhausted')})}];
  await p.e.scenario.onclick(); p.emit(r);
  const s = section(p, 'Fresh drape and heat alternatives');
  const t = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: transient heat — complete time history or refusal');
  assert.deepEqual(plain(t.children[2].children.map(n => n.textContent)),
    ['Independent sheets','Available','1','2','[-5, 0]','-3','-3','0','Not applicable']);
  assert.deepEqual(plain(t.children[3].children.map(n => n.textContent)),
    ['Body coverage + exposed cloth','Available','1','4','[0, 5]','4','4','0','-1e-12']);
  const failed = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('B: transient heat'));
  assert.match(visibleText(failed), /Unavailable: Refused: motion exhausted/);
  assert.equal(failed.children[2].children[5].textContent, 'Unknown');
  assert.match(visibleText(s), /Do not add their energy totals/);
  const d = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: Body coverage + exposed cloth temperature'));
  d.open=true; d.handlers.toggle(); assert.match(visibleText(d), /<synthetic locked source>/);
  motion.transient_coverage = refused('missing heat capacity');
  await p.e.scenario.onclick(); p.emit(r);
  assert.match(visibleText(s), /Refused: missing heat capacity/);
  assert.match(visibleText(s), /Independent sheets/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden,true); assert.equal(p.live.size,0);
});

test('plastic heat displays each domain source separately and preserves independent refusals', async () => {
  const p = await pageModule(); await p.load(); scenarioInputs(p);
  const report = scenarioReport(); report.scenario.scenario = { motion: { transient_surface: {}, transient_coverage: {} } };
  const surface = { duration_s: 1, thermal_steps: 2, trace: [{temperature_range_c:[20,22]}],
    net_boundary_energy_j: -3, stored_change_j: 6, balance_error_j: 0 };
  const coverage = { net_boundary_energy_j: 4, physical_stored_change_j: 4, balance_error_j: 0 };
  const motion = { samples: [{}], keyframes: [], transient_surface: outcome(surface),
    transient_coverage: outcome(coverage) };
  report.simulations.find(row => row.key === 'A').evaluation.result.motion = outcome(motion);
  const s = section(p, 'Fresh drape and heat alternatives');
  const work = () => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: plastic work supplied to independent-sheet heat');
  const coverageWork = () => descendants(s).find(n => n.tag === 'table' && n.children[0].textContent === 'A: plastic work supplied to coverage and exposed-cloth heat');
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work(), undefined);
  surface.plastic_dissipation_heat = { distribution: 'uniform_reference_thickness', temporal_distribution: 'uniform_over_accepted_interval',
    plastic_dissipation_j: 12, supplied_heat_j: 9, unconverted_dissipation_j: 3, initial_plastic_dissipation_j: 5,
    material_fractions: [{ source: '<qualified material fraction>', fraction: 0.75 }] };
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(work().children[2].children.map(n => n.textContent)), ['12','9','3','5']);
  const heat = descendants(s).find(n => n.tag === 'table' && n.children[0].textContent.startsWith('A: transient heat'));
  assert.deepEqual(plain(heat.children[2].children.slice(5,8).map(n => n.textContent)), ['-3','6','0']);
  assert.deepEqual(plain(heat.children[3].children.slice(5,8).map(n => n.textContent)), ['4','4','0']);
  assert.match(visibleText(s), /stored change minus net boundary energy minus supplied plastic heat/);
  assert.match(visibleText(s), /initial drape is excluded/);
  assert.equal(coverageWork(), undefined);
  coverage.plastic_dissipation_heat = {...surface.plastic_dissipation_heat, supplied_heat_j: 6, unconverted_dissipation_j: 6};
  coverage.physical_stored_change_j = 10;
  await p.e.scenario.onclick(); p.emit(report);
  assert.deepEqual(plain(coverageWork().children[2].children.map(n => n.textContent)), ['12','6','6','5']);
  assert.match(visibleText(s), /Source heat follows covered and exposed reference material; bare body receives none/);
  const source = descendants(s).find(n => n.tag === 'details' && n.children[0].textContent.startsWith('A: Independent sheets temperature'));
  source.open = true; source.handlers.toggle(); assert.match(visibleText(source), /<qualified material fraction>/);
  assert.equal(descendants(s).some(n => 'innerHTML' in n), false);
  surface.plastic_dissipation_heat.unconverted_dissipation_j = null;
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work().children[2].children[2].textContent, 'Unknown');
  motion.transient_surface = refused('missing source-qualified plastic heat fraction');
  await p.e.scenario.onclick(); p.emit(report); assert.equal(work(), undefined);
  assert.deepEqual(plain(coverageWork().children[2].children.map(n => n.textContent)), ['12','6','6','5']);
  assert.match(visibleText(s), /Refused: missing source-qualified plastic heat fraction/);
  assert.match(visibleText(s), /Body coverage \+ exposed cloth/);
  p.e.scenarioRequest.handlers.change(); assert.equal(s.hidden, true); assert.equal(p.live.size, 0);
});

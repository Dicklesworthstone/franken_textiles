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

async function workerModule({ failFold = false, failExport = false, trap = false, rejectLoad = false,
  failScenario = false, trapScenario = false, bindings = null } = {}) {
  const sent = [], called = [], freed = [];
  const imports = bindings || {
    default: () => rejectLoad ? Promise.reject(Error('bundle unavailable')) : Promise.resolve(),
    material_catalog: q => JSON.stringify({ materials: [], query: q }),
    material_inspect: s => JSON.stringify({ selection: JSON.parse(s) }),
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
    frontier: ['A'], uncertainty_propagated: false,
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
    const screenRequest = fixture('screen-insulation.json');
    const screened = await w.send({ type: 'screen', screenRequest });
    assert.equal(screened.type, 'result', screened.message);
    assert.deepEqual(screened.result, JSON.parse(ftx.material_screen(screenRequest)));
    assert.equal(screened.files[1].text, screenRequest);
    const pattern = fixture('synthetic-sewing-pattern.json');
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
      assert.equal((await w.send(msg)).files[0].text, result.files[0].text);
      assert.ok(w.sent.some(m => m.type === 'progress' && m.request === 93 && m.text.includes('motion endpoint')));
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

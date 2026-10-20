// Compression stack UI lifecycle using actual modules and explicit DOM/WASM doubles.
// This verifies selection identity and stale-result handling, not rendering or compiled WASM.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = name => fs.readFileSync(path.join(__dirname, '../web', name), 'utf8');
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };

async function workerModule() {
  const sent = [], called = [];
  const imports = {
    default: () => Promise.resolve(),
    material_inspect: s => JSON.stringify({selection: JSON.parse(s)}),
    material_compression_stack: r => {
      called.push(['compression-stack', r]);
      const layers = ['outer', 'inner'].map(id => {
        const selection = `{"id":"${id}","allow_estimates":true,"value":-0.0}`;
        return { id, conditioned: { selection: JSON.parse(selection), selection_json: selection } };
      });
      return JSON.stringify({ schema: 'ftx-compression-stack-report/1', layers, pressure_pa: 250 });
    },
  };
  const context = vm.createContext({ TextEncoder, WebAssembly, JSON, Error, self: {}, postMessage: m => sent.push(m) });
  const stub = new vm.SyntheticModule(Object.keys(imports), function () {
    for (const [name, value] of Object.entries(imports)) this.setExport(name, value);
  }, { context });
  const module = new vm.SourceTextModule(read('materials-worker.js'), { context });
  await module.link(() => stub); await module.evaluate();
  const send = async data => { await context.self.onmessage({ data }); return sent.at(-1); };
  return {send, called};
}

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
  return { e, current, workers, ready, select, emit, load, live, blobs, windowEvents };
}
test('stack closure requires an explicit layer and preserves every frozen selection byte', async () => {
  const w = await workerModule(), request = '{"schema":"ftx-compression-stack/1","gap_m":0.030,"coordinate":-0.0}';
  const msg = { type: 'pressure', request: 25, compressionRequest: request };
  const missing = await w.send(msg);
  assert.equal(missing.type, 'error'); assert.match(missing.message, /Name the stack layer/);
  assert.equal(w.called.length, 0);
  const bad = await w.send({ ...msg, compressionLayer: 'unknown' });
  assert.equal(bad.type, 'error'); assert.equal(bad.files, undefined);
  const result = await w.send({ ...msg, compressionLayer: 'inner' });
  assert.equal(result.type, 'result'); assert.deepEqual(w.called.at(-1), ['compression-stack', request]);
  assert.equal(result.result.chosen_layer_id, 'inner');
  assert.equal(result.result.selection_json, '{"id":"inner","allow_estimates":true,"value":-0.0}');
  assert.equal(result.files.length, 4);
  assert.equal(result.files[0].name, 'compression-stack-report.json');
  assert.equal(result.files[1].text, request);
  assert.equal(result.files[2].text, '{"id":"outer","allow_estimates":true,"value":-0.0}');
  assert.equal(result.files[3].text, result.result.selection_json);
  assert.equal(JSON.parse(result.files[0].text).chosen_layer_id, undefined); // unmodified core receipt
  const stale = await w.send({ ...msg, compressionRequest: '{"pressure_pa":250}', compressionLayer: 'inner' });
  assert.equal(stale.type, 'error'); assert.match(stale.message, /only valid/);
});

test('stack layer selection uses ordinary bindings and edits cancel stale in-flight preparations', async () => {
  const p = await pageModule(); await p.load();
  p.select('compressionRequest', '{"schema":"ftx-compression-stack/1"}');
  p.e.compressionLayer.value = 'inner'; p.e.compressionLayer.handlers.input();
  await p.e.pressure.onclick();
  assert.equal(p.current().messages.at(-1).compressionLayer, 'inner');
  const selection = '{"id":"inner","expected_sha256":"locked","value":-0.0}';
  p.emit({ schema: 'ftx-compression-stack-report/1', chosen_layer_id: 'inner', pressure_pa: 250,
    input: { gap_m: 0.03 }, layers: [{ id: 'inner', thickness_m: 0.015, conditioned: {} }],
    selection: JSON.parse(selection), selection_json: selection,
    inspection: { snapshot: { id: 'inner' }, capabilities: {} } });
  assert.equal(p.e.assign.disabled, false);
  p.e.target.value = 'lining'; p.e.assign.onclick(); await p.e.bind.onclick();
  assert.ok(p.current().messages.at(-1).changes.includes(selection));
  p.emit({ pattern: {}, pattern_json: '{}', pieces: [], pattern_sha256: 'new' });
  p.e.compressionLayer.value = 'outer'; p.e.compressionLayer.handlers.input();
  assert.equal(p.e.assign.disabled, true);
  const delay = deferred(); p.e.compressionRequest.files[0].text = () => delay.promise;
  const old = p.current(), running = p.e.pressure.onclick();
  p.e.compressionLayer.value = 'inner'; p.e.compressionLayer.handlers.input(); p.ready();
  delay.resolve('{"schema":"ftx-compression-stack/1"}'); await running;
  assert.equal(old.dead, true);
  assert.equal(old.messages.filter(m => m.type === 'pressure').length, 1); // earlier completed request only
});


// Unit checks of the actual page/worker modules with explicit DOM/WASM doubles.
// This is NOT a browser-rendering or WASM-execution test.
// node --experimental-vm-modules --test tools/check_trace_frontend.cjs
require('./check_materials_frontend.cjs');
require('./check_stock_job_frontend.cjs');
require('./check_compression_frontend.cjs');
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..');
const read = n => fs.readFileSync(path.join(root, 'web', n), 'utf8');

async function workerModule({ rejectLoad = false, failFiles = false } = {}) {
  const sent = [], instances = [], reviews = [];
  class WebTrace {
    constructor(bytes) {
      if (bytes[0] === 0) throw Error('bad source');
      this.freed = false; instances.push(this);
    }
    free() { assert.equal(this.freed, false); this.freed = true; }
    review(names) {
      assert.equal(this.freed, false);
      JSON.parse(names);
      const r = { freed: false,
        report: () => JSON.stringify({ scale_status: 'UNSCALED' }),
        names: () => JSON.stringify(['pattern.json']),
        file: () => { if (failFiles) throw Error('file failure'); return new Uint8Array([1, 2]); },
        zip: () => new Uint8Array([3, 4]),
        free: () => { assert.equal(r.freed, false); r.freed = true; }
      }; reviews.push(r); return r;
    }
  }
  const context = vm.createContext({ Uint8Array, JSON, Error, self: {}, postMessage: m => sent.push(m) });
  const imports = new vm.SyntheticModule(['default', 'WebTrace'], function () {
    this.setExport('default', () => rejectLoad ? Promise.reject(Error('load failure')) : Promise.resolve());
    this.setExport('WebTrace', WebTrace);
  }, { context });
  const module = new vm.SourceTextModule(read('trace-worker.js'), { context });
  await module.link(() => imports); await module.evaluate();
  const send = async data => { await context.self.onmessage({ data }); return sent.at(-1); };
  return { send, sent, instances, reviews };
}

test('worker transfers complete products and frees temporary reviews', async () => {
  const w = await workerModule();
  const r = await w.send({ type: 'trace', request: 7, bytes: new Uint8Array([1]), options: '{}' });
  assert.equal(r.type, 'result'); assert.equal(r.request, 7);
  assert.deepEqual([...r.files[0].bytes], [1, 2]);
  assert.equal(w.reviews[0].freed, true); assert.equal(w.instances[0].freed, false);
});
test('failed replacement destroys the old source; failed edits preserve the original', async () => {
  const w = await workerModule();
  await w.send({ type: 'trace', bytes: new Uint8Array([1]), options: '{}' });
  assert.equal((await w.send({ type: 'review', names: '{', targets: '[]', labels: '[]' })).type, 'error');
  assert.equal(w.instances[0].freed, false);
  assert.equal((await w.send({ type: 'trace', bytes: new Uint8Array([0]), options: '{}' })).type, 'error');
  assert.equal(w.instances[0].freed, true);
  assert.match((await w.send({ type: 'review' })).message, /Trace the selected image first/);
});
test('failed file materialization frees both initial review and candidate', async () => {
  const w = await workerModule({ failFiles: true });
  assert.equal((await w.send({ type: 'trace', bytes: new Uint8Array([1]), options: '{}' })).type, 'error');
  assert.equal(w.instances[0].freed, true); assert.equal(w.reviews[0].freed, true);
});
test('initialization rejection is visible without any user request', async () => {
  const w = await workerModule({ rejectLoad: true });
  await w.send({ type: 'trace' });
  assert.ok(w.sent.some(m => m.type === 'loadError'));
  assert.ok(!w.sent.some(m => m.type === 'ready' || m.type === 'result'));
});

class Element {
  constructor() { this.value = ''; this.files = []; this.children = []; this.handlers = {}; this.checked = false; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
}
async function pageModule() {
  const elements = Object.fromEntries([...read('trace.html').matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Element()]));
  const source = ['image', 'channel', 'threshold', 'invert', 'stroke', 'area', 'crop', 'dashes'].map(k => elements[k]);
  const edits = ['names', 'targets', 'labels'].map(k => elements[k]);
  Object.assign(elements.channel, { value: 'auto' });
  elements.threshold.value = 'otsu'; elements.invert.value = 'auto'; elements.area.value = '0.01';
  elements.names.value = '{}'; elements.targets.value = elements.labels.value = '[]';
  const workers = [], live = new Set(); let next = 0;
  class Worker {
    constructor() { this.messages = []; this.dead = false; workers.push(this); }
    postMessage(message) { assert.equal(this.dead, false); this.messages.push(message); }
    terminate() { this.dead = true; }
    emit(message) { this.onmessage({ data: message }); }
  }
  class LocalURL extends URL {}
  LocalURL.createObjectURL = () => { const id = `blob:${++next}`; live.add(id); return id; };
  LocalURL.revokeObjectURL = id => { assert.equal(live.delete(id), true); };
  const context = vm.createContext({ Worker, URL: LocalURL, Blob, TextEncoder, Uint8Array, JSON, Number,
    document: {
      getElementById: id => { assert.ok(elements[id], `missing HTML id ${id}`); return elements[id]; },
      querySelectorAll: q => q.startsWith('#source') ? source : edits,
      createElement: () => new Element(),
    }, window: { addEventListener() {} }
  });
  const module = new vm.SourceTextModule(read('trace-page.js'), {
    context, initializeImportMeta: m => { m.url = 'https://unit.invalid/trace-page.js'; }
  });
  await module.link(() => { throw Error('unexpected page import'); }); await module.evaluate();
  const current = () => workers.at(-1);
  const select = () => {
    elements.image.files = [{ size: 2, arrayBuffer: async () => new Uint8Array([1, 2]).buffer }];
    elements.image.handlers.change(); current().emit({ type: 'ready' });
  };
  const result = request => ({ type: 'result', request,
    report: { scale_status: 'UNSCALED', audit: { summary: { ok: false } },
      pieces: [{ id: '<tag>', edges: ['e1'], vertices: ['c1'] }], no_claim: 'review only' },
    files: [{ name: 'pattern.json', bytes: new Uint8Array([1, 2]) }]
  });
  return { e: elements, workers, current, select, result, live };
}

test('page invalidates edited downloads and ignores wrong request identities', async () => {
  const p = await pageModule(); p.select();
  await p.e.trace.onclick(); const id = p.current().messages.at(-1).request;
  p.current().emit(p.result(id + 1)); assert.equal(p.e.results.hidden, true);
  p.current().emit(p.result(id)); assert.equal(p.e.results.hidden, false);
  assert.equal(p.live.size, 1); assert.match(p.e.pieces.children[0].textContent, /<tag>/);
  p.e.names.handlers.input(); assert.equal(p.live.size, 0); assert.equal(p.e.results.hidden, true);
  p.e.apply.onclick(); const edit = p.current().messages.at(-1).request;
  p.current().emit({ type: 'error', request: edit, message: 'bad edit' });
  assert.equal(p.e.results.hidden, true); assert.equal(p.e.apply.disabled, false);
});
test('cancelling a pending file read prevents transfer or late-result publication', async () => {
  const p = await pageModule(); p.select();
  let finish;
  p.e.image.files[0].arrayBuffer = () => new Promise(resolve => { finish = resolve; });
  const old = p.current(), running = p.e.trace.onclick();
  p.e.cancel.onclick(); finish(new Uint8Array([1]).buffer); await running;
  assert.equal(old.dead, true); assert.equal(old.messages.length, 0);
  old.emit(p.result(1)); old.emit({ type: 'ready' });
  assert.equal(p.e.results.hidden, true); assert.equal(p.e.trace.disabled, true);
  p.current().emit({ type: 'ready' }); assert.equal(p.e.apply.disabled, true);
});
test('source changes reset edits and failed replacement cannot reuse a sheet', async () => {
  const p = await pageModule(); p.select(); await p.e.trace.onclick();
  p.current().emit(p.result(p.current().messages.at(-1).request));
  p.e.names.value = '{"previous":"edits"}'; p.e.channel.handlers.change();
  assert.equal(p.e.names.value, '{}'); assert.equal(p.e.apply.disabled, true); assert.equal(p.live.size, 0);
  p.current().emit({ type: 'ready' }); await p.e.trace.onclick();
  p.current().emit({ type: 'error', request: p.current().messages.at(-1).request, message: 'bad PNG' });
  assert.equal(p.e.apply.disabled, true); assert.equal(p.e.error.textContent, 'bad PNG');
});
test('load errors and image read limits do not hang the page or publish files', async () => {
  const p = await pageModule(); p.select();
  p.e.image.files[0].size = 16 * 1024 * 1024 + 1;
  await p.e.trace.onclick(); assert.equal(p.current().messages.length, 0);
  assert.match(p.e.error.textContent, /16 MiB/); assert.equal(p.e.cancel.disabled, true);
  p.current().emit({ type: 'loadError', message: 'missing module' });
  assert.equal(p.e.trace.disabled, true); assert.match(p.e.error.textContent, /missing module/);
});

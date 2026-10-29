// Unit checks of the actual page/worker modules with explicit DOM/WASM doubles.
// This is NOT a browser-rendering or WASM-execution test.
// node --experimental-vm-modules --test tools/check_trace_frontend.cjs
require('./check_materials_frontend.cjs');
require('./check_stock_job_frontend.cjs');
require('./check_compression_frontend.cjs');
require('./check_photo_frontend.cjs');
require('./check_video_masks_frontend.cjs');
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
  constructor() { this._value = ''; this.files = []; this.children = []; this.handlers = {}; this.checked = false; }
  get value() { return this._value; }
  set value(v) { this._value=this.type==='textarea'?v.replace(/\r\n?/g,'\n'):v; if(this.type==='file' && v==='')this.files=[]; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children = items; }
}
async function pageModule() {
  const elements = Object.fromEntries([...read('trace.html').matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Element()]));
  const source = ['image', 'channel', 'threshold', 'invert', 'stroke', 'area', 'crop', 'dashes'].map(k => elements[k]);
  const edits = ['names', 'targets', 'labels'].map(k => elements[k]);
  edits.forEach(e=>{e.type='textarea';});if(elements.draftFile)elements.draftFile.type='file';
  Object.assign(elements.channel, { value: 'auto' });
  elements.threshold.value = 'otsu'; elements.invert.value = 'auto'; elements.area.value = '0.01';
  elements.names.value = '{}'; elements.targets.value = elements.labels.value = '[]';
  const workers = [], live = new Set(), blobs = new Map(), events={},popups=[]; let next = 0,popupBlocked=false;
  class Worker {
    constructor() { this.messages = []; this.dead = false; workers.push(this); }
    postMessage(message) { assert.equal(this.dead, false); this.messages.push(message); }
    terminate() { this.dead = true; }
    emit(message) { this.onmessage({ data: message }); }
  }
  class LocalURL extends URL {}
  LocalURL.createObjectURL = blob => { const id = `blob:${++next}`; live.add(id);blobs.set(id,blob);return id; };
  LocalURL.revokeObjectURL = id => { assert.equal(live.delete(id), true);blobs.delete(id); };
  const context = vm.createContext({ Worker, URL: LocalURL, Blob, TextEncoder, TextDecoder, Uint8Array, JSON, Number,
    crypto:{getRandomValues:a=>{a.fill(popups.length+1);return a;}},
    document: {
      getElementById: id => { assert.ok(elements[id], `missing HTML id ${id}`); return elements[id]; },
      querySelectorAll: q => q.startsWith('#source') ? source : edits,
      createElement: () => new Element(),
    }, window: { addEventListener:(name,fn)=>{events[name]=fn;},open:(url,target)=>{
      if(popupBlocked)return null;
      const child={url,target,messages:[],postMessage(m,origin){this.messages.push({data:m,origin});}};popups.push(child);return child;
    } }
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
    report: { source_sha256:'a'.repeat(64),scale_status: 'UNSCALED', audit: { summary: { ok: false } },
      pieces: [{ id: '<tag>', edges: ['e1'], vertices: ['c1'] }], no_claim: 'review only' },
    files: [{ name: 'pattern.json', bytes: new Uint8Array([1, 2]) }]
  });
  const trace=async()=>{await elements.trace.onclick();current().emit(result(current().messages.at(-1).request));};
  return { e: elements, workers, current, select, result, live,blobs,trace,events,popups,blockPopups:()=>{popupBlocked=true;} };
}

test('trace pattern handoff sends only current review to its same-origin requesting window',async()=>{
  const p=await pageModule();assert.ok(p.e.openPattern,'Check traced pattern without download/reupload');p.select();await p.trace();
  p.e.openPattern.onclick();const child=p.popups[0],token=new URL(child.url).searchParams.get('traceImport');
  assert.equal(child.target,'_blank');assert.equal(new URL(child.url).origin,'https://unit.invalid');assert.ok(token);
  p.e.openPattern.onclick();assert.equal(p.popups.length,1,'One pending handoff cannot orphan an earlier tab');
  const ready={type:'ftx-trace-pattern-ready',token};
  for(const [origin,source,data] of [['https://wrong.invalid',child,ready],['https://unit.invalid',{},ready],['https://unit.invalid',child,{...ready,token:'wrong'}]]){
    p.events.message({origin,source,data});assert.equal(child.messages.length,0);
  }
  p.events.message({origin:'https://unit.invalid',source:child,data:ready});
  assert.equal(child.messages.length,1);const sent=child.messages[0];assert.equal(sent.origin,'https://unit.invalid');
  assert.equal(sent.data.type,'ftx-trace-pattern');assert.equal(sent.data.source_sha256,'a'.repeat(64));
  assert.deepEqual([...new Uint8Array(await sent.data.pattern.arrayBuffer())],[1,2]);
  p.events.message({origin:'https://unit.invalid',source:child,data:ready});assert.equal(child.messages.length,1);
  p.e.openPattern.onclick();const stale=p.popups[1],oldToken=new URL(stale.url).searchParams.get('traceImport');
  p.e.names.handlers.input();p.events.message({origin:'https://unit.invalid',source:stale,data:{...ready,token:oldToken}});
  assert.equal(stale.messages[0].data.type,'ftx-trace-pattern-cancelled');assert.equal(stale.messages[0].data.pattern,undefined);
  await p.trace();p.blockPopups();p.e.openPattern.onclick();assert.match(p.e.status.textContent,/popup blocked/);
});

test('tracing edit drafts retain exact text before applying and after native refusal',async()=>{
  const p=await pageModule();assert.ok(p.e.saveDraft,'Save unfinished tracing edits');p.select();await p.trace();
  p.e.names.value='{';p.e.names.handlers.input();p.e.saveDraft.onclick();
  const a=p.e.draftDownloads.children[0];assert.ok(a);const text=await p.blobs.get(a.href).text(),draft=JSON.parse(text);
  assert.equal(draft.schema,'ftx-trace-edits/1');assert.equal(draft.source_sha256,'a'.repeat(64));assert.equal(draft.names,'{');
  assert.equal(p.e.results.hidden,true);p.e.names.value='{}';p.e.names.handlers.input();assert.equal(p.e.draftDownloads.children.length,0);
  const raw=' {\r\n"pieces":{}\r\n}\r\n';draft.names=raw;
  const input=s=>({size:Buffer.byteLength(s),arrayBuffer:async()=>new TextEncoder().encode(s).buffer});
  p.e.draftFile.files=[input(JSON.stringify(draft))];await p.e.draftFile.handlers.change();
  assert.equal(p.e.names.value,raw.replace(/\r\n/g,'\n'));assert.equal(p.e.draftFile.files.length,0);
  p.e.labels.value='[ ]';p.e.labels.handlers.input();p.e.apply.onclick();
  assert.equal(p.current().messages.at(-1).names,raw,'Editing another field preserves original source');
  p.current().emit({type:'error',request:p.current().messages.at(-1).request,message:'bad edit'});
  p.e.saveDraft.onclick();assert.equal(JSON.parse(await p.blobs.get(p.e.draftDownloads.children[0].href).text()).names,raw);
  p.e.names.handlers.input();p.e.apply.onclick();assert.equal(p.current().messages.at(-1).names,p.e.names.value);
});

test('tracing edit draft reads are atomic, bounded and discard late completions',async()=>{
  const p=await pageModule();assert.ok(p.e.draftFile,'Restore tracing edits');p.select();await p.trace();
  p.e.saveDraft.onclick();const draft=JSON.parse(await p.blobs.get(p.e.draftDownloads.children[0].href).text());
  const input=s=>({size:Buffer.byteLength(s),arrayBuffer:async()=>new TextEncoder().encode(s).buffer});
  for(const bad of [null,[],{}, {...draft,source_sha256:'b'.repeat(64)},{...draft,options:'{}'},
    {...draft,names:7},{...draft,extra:true},{...draft,names:'x'.repeat(256*1024+1)}]){
    p.e.draftFile.files=[input(JSON.stringify(bad))];await p.e.draftFile.handlers.change();
    assert.equal(p.e.names.value,'{}');assert.equal(p.e.targets.value,'[]');assert.equal(p.e.labels.value,'[]');assert.equal(p.e.error.hidden,false);
  }
  let reads=0;p.e.draftFile.files=[{size:8*1024*1024+1,arrayBuffer:async()=>{reads++;return new ArrayBuffer(0);}}];await p.e.draftFile.handlers.change();assert.equal(reads,0);
  for(const bytes of [new Uint8Array([255]),new TextEncoder().encode('\ufeff{}'),new TextEncoder().encode('{')]){
    p.e.draftFile.files=[{size:bytes.length,arrayBuffer:async()=>bytes.buffer}];await p.e.draftFile.handlers.change();assert.equal(p.e.error.hidden,false);
  }
  for(const reject of [false,true])for(const change of ['source','cancel','edit','replacement']){
    const q=await pageModule();q.select();await q.trace();let finish;
    const read=new Promise((resolve,fail)=>{finish=()=>reject?fail(Error('late read')):resolve(new TextEncoder().encode(JSON.stringify({...draft,names:'{"old":true}'})).buffer);});
    q.e.draftFile.files=[{size:200,arrayBuffer:()=>read}];const old=q.e.draftFile.handlers.change();
    if(change==='source'){q.e.channel.handlers.change();q.current().emit({type:'ready'});await q.trace();}
    if(change==='cancel'){q.e.cancel.onclick();q.current().emit({type:'ready'});await q.trace();}
    if(change==='edit'){q.e.names.value='{"new":true}';q.e.names.handlers.input();}
    if(change==='replacement'){q.e.draftFile.files=[input(JSON.stringify({...draft,names:'{"new":true}'}))];await q.e.draftFile.handlers.change();}
    q.e.apply.onclick();const worker=q.current(),before=q.e.names.value,id=worker.messages.at(-1).request;
    finish();await old;assert.equal(q.e.names.value,before,change);assert.equal(q.e.cancel.disabled,false,change);
    worker.emit(q.result(id));assert.equal(q.e.results.hidden,false,change);
  }
});

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

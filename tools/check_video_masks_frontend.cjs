// Exercise the actual offline editor with small DOM doubles; native tests own mask geometry/pixels.
const test = require('node:test'), assert = require('node:assert/strict');
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const root = path.resolve(__dirname, '..'), base = path.join(root, 'crates/ftx-cli/src/measure/video_masks');
class Element {
  constructor() { this.handlers = {}; this.children = []; this.attrs = {}; this.dataset = {}; this.files = []; this.value = ''; this.disabled = false; this.hidden = false; }
  addEventListener(name, fn) { this.handlers[name] = fn; }
  append(...items) { this.children.push(...items); }
  add(item) { this.append(item); }
  replaceChildren(...items) { this.children = items; }
  setAttribute(key, value) { this.attrs[key] = String(value); }
  removeAttribute(key) { delete this.attrs[key]; if (key === 'href') delete this.href; }
  getScreenCTM() { return {inverse: () => ({})}; }
}
function setup() {
  const ids = [...fs.readFileSync(path.join(base, 'editor.rs'), 'utf8').matchAll(/id="([^"]+)"/g)].map(m => m[1]);
  const elements = Object.fromEntries(ids.map(id => [id, new Element()])), windows = {}, live = new Map();
  const original = {schema: 'ftx-video-mask-polygons/1', source_manifest: '/local/frames.json', source_manifest_sha256: 'a'.repeat(64),
    frames: [0, 1].map(index => ({index, image_sha256: String(index).repeat(64), decoded_gray_sha256: 'b'.repeat(64),
      image_size_px: [64, 48], person_regions: null, sheet_regions: null}))};
  elements.annotations.textContent = JSON.stringify(original);
  elements['frame-images'].textContent = JSON.stringify(original.frames.map((f, i) => ({file: `source-000${i}.png`, label: `stream <literal> @ 9007199254740993 µs; lens ${i}`})));
  let next = 0;
  const context = vm.createContext({JSON, Number, Math, Set, Error, Blob, structuredClone,
    URL: {createObjectURL(blob) { const url = 'blob:' + ++next; live.set(url, blob); return url; },
      revokeObjectURL(url) { assert.equal(live.delete(url), true, 'only live downloads are revoked'); }},
    Option: class extends Element { constructor(label, value) { super(); this.textContent = label; this.value = value; } },
    DOMPoint: class { constructor(x, y) { this.x = x; this.y = y; } matrixTransform() { return {x: (this.x - 10) / 2, y: (this.y - 20) / 2}; } },
    document: {getElementById(id) { assert.ok(elements[id], 'HTML control exists: ' + id); return elements[id]; },
      createElementNS(namespace, name) { const element = new Element(); element.localName = name; return element; }},
    window: {addEventListener(name, fn) { windows[name] = fn; }}
  });
  vm.runInContext(fs.readFileSync(path.join(base, 'editor.js'), 'utf8'), context);
  const emit = (id, name, event = {}) => elements[id].handlers[name](event);
  emit('frame-image', 'load');
  const click = id => { assert.equal(elements[id].disabled, false, id + ' is enabled'); return emit(id, 'click'); };
  const change = (id, value) => { elements[id].value = String(value); return emit(id, 'change'); };
  const pixel = (x, y) => emit('canvas', 'click', {clientX: x * 2 + 10, clientY: y * 2 + 20});
  const read = async () => { assert.equal(live.size, 1); return JSON.parse(await [...live.values()][0].text()); };
  const importDraft = value => { elements.import.files = [{size: 100, text: async () => JSON.stringify(value)}]; return emit('import', 'change'); };
  return {elements, original, windows, live, emit, click, change, pixel, read, importDraft};
}

test('scaled image clicks author components and holes while unfinished polygons revoke downloads', async () => {
  const p = setup(), first = p.elements['mask-download'].href;
  assert.match(p.elements.label.textContent, /9007199254740993/);
  assert.match(p.elements.label.textContent, /<literal>/);
  p.click('new-region'); assert.equal(p.live.size, 0); assert.equal(p.live.has(first), false);
  assert.equal(p.elements.import.disabled, true);
  p.pixel(2.2, 2.4); p.pixel(25, 2); p.pixel(25, 25); p.pixel(2, 25); p.click('finish');
  assert.deepEqual((await p.read()).frames[0].person_regions[0], {outer: [[2, 2], [25, 2], [25, 25], [2, 25]], holes: []});
  p.click('new-hole'); p.pixel(6, 6); p.pixel(10, 6); p.pixel(8, 10); p.click('finish');
  p.click('new-region'); p.pixel(30, 2); p.pixel(40, 2); p.pixel(35, 12); p.click('finish');
  const output = await p.read();
  assert.equal(output.frames[0].person_regions.length, 2);
  assert.deepEqual(output.frames[0].person_regions[0].holes, [[[6, 6], [10, 6], [8, 10]]]);
  assert.equal(output.frames[0].sheet_regions, null); assert.equal(output.frames[1].person_regions, null);
  assert.equal(output.source_manifest_sha256, p.original.source_manifest_sha256);
  p.change('target', 'sheet_regions'); p.click('not-visible');
  assert.deepEqual((await p.read()).frames[0].sheet_regions, []);
  p.click('new-region'); p.pixel(2, 2); p.pixel(10, 2); p.click('cancel');
  assert.deepEqual((await p.read()).frames[0].sheet_regions, []);
  p.click('next'); assert.equal(p.live.size, 0); p.emit('frame-image', 'load');
  assert.equal((await p.read()).frames[1].sheet_regions, null);
  p.windows.pagehide(); assert.equal(p.live.size, 0); p.emit('frame-image', 'load'); assert.equal(p.live.size, 0);
});

test('draft import preserves complete bindings and rejects stale or malformed edits without a stale link', async () => {
  const p = setup(), valid = structuredClone(p.original);
  valid.frames[0].person_regions = [{outer: [[1, 1], [12, 1], [1, 12]], holes: []}];
  valid.frames[1].sheet_regions = [];
  await p.importDraft(valid); assert.deepEqual(await p.read(), valid);
  for (const edit of [
    value => { value.source_manifest_sha256 = 'changed'; },
    value => { value.frames[0].image_sha256 = 'changed'; },
    value => { value.frames[0].index = 1; },
    value => { value.frames[0].person_regions[0].outer[0] = [100, 0]; },
    value => { value.frames[0].person_regions[0].outer[0] = [1.5, 0]; },
    value => { value.frames[0].unexpected = 'field'; }
  ]) {
    const changed = structuredClone(valid); edit(changed); await p.importDraft(changed);
    assert.equal(p.live.size, 0); assert.equal(p.elements['mask-download'].hidden, true);
    assert.match(p.elements.status.textContent, /Draft refused/);
  }
  await p.importDraft(valid); assert.deepEqual(await p.read(), valid);
  p.click('reset'); assert.deepEqual(await p.read(), p.original);
});

test('reset cancels an in-flight import and incomplete or out-of-bounds drawing never publishes a polygon', async () => {
  const p = setup(); let resolve;
  p.elements.import.files = [{size: 100, text: () => new Promise(done => { resolve = done; })}];
  const pending = p.emit('import', 'change'); assert.equal(p.live.size, 0);
  p.click('reset'); resolve(JSON.stringify({...p.original, source_manifest: '/wrong/frames.json'})); await pending;
  assert.deepEqual(await p.read(), p.original);
  p.click('new-region'); p.pixel(-2, 1); assert.match(p.elements.status.textContent, /inside/); assert.equal(p.live.size, 0);
  p.pixel(2, 2); p.pixel(5, 5); p.pixel(8, 8); p.click('finish');
  assert.match(p.elements.status.textContent, /nonzero area/); assert.equal(p.live.size, 0);
  p.click('undo'); p.pixel(8, 3); p.click('finish'); assert.equal((await p.read()).frames[0].person_regions.length, 1);
  p.click('remove-region'); assert.equal((await p.read()).frames[0].person_regions, null);
});

test('preview subtracts overlapping and outside holes independently within each component', async () => {
  const p = setup(), draft = structuredClone(p.original);
  const outer = [[1, 1], [30, 1], [30, 30], [1, 30]];
  const holes = [[[5, 5], [20, 5], [20, 20], [5, 20]], [[10, 10], [25, 10], [25, 25], [10, 25]],
    [[40, 10], [50, 10], [50, 20], [40, 20]]];
  const restored = [[12, 12], [15, 12], [15, 15], [12, 15]];
  draft.frames[0].person_regions = [{outer, holes}, {outer: restored, holes: []}];
  await p.importDraft(draft);
  const overlays = p.elements.regions.children, definitions = overlays.find(node => node.localName === 'defs');
  assert.equal(definitions.children.length, 2);
  const [first, second] = definitions.children;
  for (const mask of [first, second]) {
    assert.equal(mask.localName, 'mask'); assert.equal(mask.attrs.maskUnits, 'userSpaceOnUse');
    assert.equal(mask.attrs.maskContentUnits, 'userSpaceOnUse'); assert.equal(mask.attrs.style, 'mask-type:luminance');
    assert.equal(mask.attrs.width, '64'); assert.equal(mask.attrs.height, '48');
  }
  const ringPath = points => 'M' + points.map(point => point.join(',')).join('L') + 'Z';
  assert.deepEqual(first.children.map(node => node.attrs.fill), ['white', 'black', 'black', 'black']);
  assert.deepEqual(first.children.map(node => node.attrs.d), [outer, ...holes].map(ringPath));
  assert.deepEqual(second.children.map(node => node.attrs.fill), ['white']);
  assert.equal(second.children[0].attrs.d, ringPath(restored));
  assert.deepEqual(overlays.filter(node => node.localName === 'rect').map(node => node.attrs.mask),
    ['url(#' + first.attrs.id + ')', 'url(#' + second.attrs.id + ')']);
  assert.ok(overlays.filter(node => node.localName === 'path').every(node => node.attrs.fill === 'none'));
  assert.deepEqual(await p.read(), draft);
});

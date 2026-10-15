// Execute generated WASM, not a mock or just a wasm32 compilation.
// node tools/check_sewing_wasm.cjs /absolute/node-bindings/ftx_wasm.js /absolute/ftx-materials
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');

if (process.argv.length !== 4) {
  throw new Error('Usage: node check_sewing_wasm.cjs NODE-BINDINGS.js FTX-MATERIALS');
}
const wasm = require(path.resolve(process.argv[2]));
const binary = path.resolve(process.argv[3]);
const root = path.resolve(__dirname, '..');
const fixtures = path.join(root, 'tests/fixtures/materials');
const basePattern = JSON.parse(fs.readFileSync(path.join(fixtures, 'synthetic-sewing-pattern.json')));
const baseOptions = JSON.parse(fs.readFileSync(path.join(fixtures, 'synthetic-sewing-topstitch-options.json')));
// Synthetic inputs only. Retain this temporary directory for reproducing any failing case.
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'ftx-sewing-wasm-'));
const cases = [
  ['topstitch', () => {}],
  ['centerline', (_p, o) => { delete o.passes[1].stitch_path; }],
  ['zero-offset', (_p, o) => { o.passes[1].stitch_path.offset_mm = [0, 0]; }],
  ['allowance-side', (_p, o) => { o.passes[1].stitch_path.offset_mm = [-3, -4]; }],
  ['forward-locks', (_p, o) => {
    o.passes[1].direction = 'forward';
    o.passes[1].backtack = {start_stitches: 2, end_stitches: 3};
  }],
  ['reverse-locks', (_p, o) => {
    o.passes[1].backtack = {start_stitches: 2, end_stitches: 3};
  }],
  ['thicker-material', (p) => { p.fabric.material.record.properties.thickness.value = 0.6; }],
  ['mirrored', (p) => {
    p.pieces = [p.pieces[0]];
    p.pieces[0].cut.qty = 2;
    p.seams[0].a = 'front.R/right';
    p.seams[0].b = 'front.L/right';
  }],
  ['corners', (p, o) => {
    p.seams[0].a = 'front/bottom+right';
    p.seams[0].b = 'back/top+left';
    o.machine.stitch_mm[0] = o.machine.reverse.stitch_mm[0] = 0.01;
    o.passes[1].stitch_path.offset_mm = [5, 3];
  }],
  ['curve-notches', (p, o) => {
    p.pieces[0].edges[1].curve = {type: 'quad', c: [[0.5, 0.1]]};
    p.pieces[1].edges[3].curve = {type: 'quad', c: [[0.5, 0.1]]};
    p.pieces[0].notches = [{edge: 'right', s: 0.3}];
    p.pieces[1].notches = [{edge: 'left', s: 0.6}];
    o.seam_tolerance_mm = 12;
    o.machine.stitch_mm[0] = o.machine.reverse.stitch_mm[0] = 0.01;
    o.passes[1].stitch_path.offset_mm = [2, -2];
  }],
];
let comparisons = 0;
for (const [id, change] of cases) {
  const p = structuredClone(basePattern), o = structuredClone(baseOptions);
  change(p, o);
  const ps = JSON.stringify(p), opts = JSON.stringify(o);
  const pp = path.join(scratch, `${id}-pattern.json`), op = path.join(scratch, `${id}-options.json`);
  fs.writeFileSync(pp, ps); fs.writeFileSync(op, opts);
  for (const [command, fn] of [['sew-plan', wasm.sew_plan], ['sew-stitches', wasm.sew_stitches]]) {
    const native = cp.spawnSync(binary, [command, pp, op], {encoding: 'utf8', maxBuffer: 32 * 1024 * 1024});
    assert.equal(native.status, 0, `${id} ${command}: ${native.stderr}`);
    assert.deepEqual(JSON.parse(fn(ps, opts)), JSON.parse(native.stdout), `${id} ${command}`);
    comparisons++;
  }
  const svgPath = path.join(scratch, `${id}.svg`);
  const native = cp.spawnSync(binary, ['sew-svg', pp, op, 'topstitch', svgPath], {encoding: 'utf8'});
  assert.equal(native.status, 0, `${id} svg: ${native.stderr}`);
  assert.equal(wasm.sew_svg(ps, opts, 'topstitch'), fs.readFileSync(svgPath, 'utf8'), `${id} svg`);
  comparisons++;
  assert.equal(fs.readFileSync(pp, 'utf8'), ps);
  assert.equal(fs.readFileSync(op, 'utf8'), opts);
}
let refusals = 0;
for (const [id, change] of [
  ['outside-cut', (p, o) => { o.passes[1].stitch_path.offset_mm = [-11, 0]; }],
  ['missing-reverse', (_p, o) => { delete o.machine.reverse; }],
  ['thick-stack', (p) => { p.fabric.material.record.properties.thickness.value = 1.1; }],
]) {
  const p = structuredClone(basePattern), o = structuredClone(baseOptions);
  change(p, o);
  for (const fn of [wasm.sew_plan, wasm.sew_stitches, (a, b) => wasm.sew_svg(a, b, 'join')]) {
    assert.throws(() => fn(JSON.stringify(p), JSON.stringify(o)), undefined, id);
    refusals++;
  }
}
console.log(`${comparisons} native/WASM comparisons and ${refusals} WASM refusals passed; evidence ${scratch}`);

// Focused single-search stock worker checks, included by the existing frontend command.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..');
const source = fs.readFileSync(path.join(root, 'web/materials-worker.js'), 'utf8');
async function worker(bindings) {
  const messages = [];
  const context = vm.createContext({ Uint8Array, TextEncoder, WebAssembly, JSON, Error,
    self: {}, postMessage: m => messages.push(m) });
  const exports = { default: () => Promise.resolve(), ...bindings };
  const imports = new vm.SyntheticModule(Object.keys(exports), function () {
    for (const [name, value] of Object.entries(exports)) this.setExport(name, value);
  }, { context });
  const module = new vm.SourceTextModule(source, { context });
  await module.link(() => imports); await module.evaluate();
  return async msg => { await context.self.onmessage({ data: msg }); return messages.at(-1); };
}
const msg = { type: 'stock', request: 20, pattern: '{"coordinate":-0.0}', stockRequest: '{"mass":1.0}' };
const optimization = { objective:'minimize_gross_area', baseline:{stock_count:1,gross_area_m2:2},
  selected:{stock_count:2,gross_area_m2:1}, termination:'work_limit', budget_exhausted:true, lattice_optimal:false };
function job() {
  return { schema:'ftx-stock-job/1', plan_json:'{"markers":[{"placements":[]},{"placements":[{}]}],"value":-0.0,"optimization":'+JSON.stringify(optimization)+'}',
    residual_json:'{"schema":"ftx-finite-stock-request/1","value":1.0}', stock_index:1,
    preview_svg:'<svg/>', preview_refusal:null };
}
test('one worker search supplies complete source-preserving stock products and optimizer status', async () => {
  let searches = 0;
  const send = await worker({ stock_job:(p,r) => {
    ++searches; assert.equal(p,msg.pattern); assert.equal(r,msg.stockRequest); return JSON.stringify(job());
  }, stock_plan:()=>assert.fail('repeated plan search'), stock_residual:()=>assert.fail('repeated residual search'),
  stock_svg:()=>assert.fail('repeated preview search') });
  const result = await send(msg);
  assert.equal(result.type,'result',result.message); assert.equal(searches,1);
  assert.equal(result.files[0].text,job().plan_json); assert.equal(result.files[1].text,job().residual_json);
  assert.equal(result.files[2].name,'stock-1.svg'); assert.equal(result.files[2].text,'<svg/>');
  assert.equal(result.files[3].name,'stock-optimization.json');
  assert.deepEqual(JSON.parse(result.files[3].text),optimization);
  assert.equal(result.result.optimization.lattice_optimal,false);
});
test('a preview refusal retains the complete job, while malformed or refused jobs publish no files', async () => {
  const refused = job(); refused.preview_svg = null;
  refused.preview_refusal = {code:'BUDGET_EXCEEDED',message:'preview budget exhausted'};
  const send = await worker({stock_job:()=>JSON.stringify(refused)});
  const result = await send(msg);
  assert.equal(result.type,'result'); assert.equal(result.picture,null);
  assert.equal(result.files[0].text,job().plan_json);
  assert.equal(result.files[2].name,'stock-preview-refusal.json');
  assert.match(result.files[2].text,/preview budget/);
  for (const invalid of [ {...job(), stock_index:0}, {...job(), plan_json:null},
    {...job(), preview_svg:null}, {...job(), schema:'unknown'} ]) {
    const bad = await worker({stock_job:()=>JSON.stringify(invalid)});
    const outcome = await bad(msg); assert.equal(outcome.type,'error'); assert.equal(outcome.files,undefined);
  }
  const bad = await worker({stock_job:()=>{throw Error('no complete allocation');}});
  assert.equal((await bad(msg)).type,'error');
});
test('matching compiled WASM job and worker retain the selected stock and exact numeric JSON',
  {skip:process.env.FTX_REAL_WASM !== '1'}, async () => {
    const url = require('node:url');
    const pkg = path.join(root,'web/pkg');
    const ftx = await import(url.pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
    ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});
    const read = n => fs.readFileSync(path.join(root,'tests/fixtures/materials',n),'utf8');
    const pattern = read('synthetic-stock-pattern.json'), request = read('synthetic-stock-optimization.json');
    const send = await worker({...ftx, default:()=>Promise.resolve()});
    const result = await send({type:'stock',request:21,pattern,stockRequest:request});
    assert.equal(result.type,'result',result.message);
    const direct = JSON.parse(ftx.stock_job(pattern,request));
    assert.equal(result.files[0].text,direct.plan_json); assert.equal(result.files[1].text,direct.residual_json);
    assert.equal(result.files[2].text,direct.preview_svg); assert.equal(result.files[2].name,'stock-1.svg');
    assert.equal(result.result.used_stock_count,2);
    assert.ok(Math.abs(result.result.used_gross_area_m2-0.00125)<1e-14);
    assert.equal(result.result.optimization.lattice_optimal,true);
  });

test('cost-directed jobs preserve the native quote receipt without rerunning pricing', async () => {
  const priced = job();
  priced.purchasing_json = '{"currency":"USD","least_observed_independent_order_total":7.0,"pricing_sha256":"test"}';
  const plan = JSON.parse(priced.plan_json); plan.purchasing = JSON.parse(priced.purchasing_json);
  plan.optimization.objective = 'minimize_purchase_cost'; plan.optimization.selected.purchase_cost = 7;
  priced.plan_json = JSON.stringify(plan);
  let calls = 0;
  const send = await worker({stock_job:()=>{++calls; return JSON.stringify(priced);},
    stock_price:()=>assert.fail('pricing must come from the selected job')});
  const result = await send(msg);
  assert.equal(result.type, 'result'); assert.equal(calls, 1);
  const receipt = result.files.find(f=>f.name === 'stock-purchasing.json');
  assert.equal(receipt.text, priced.purchasing_json); assert.match(receipt.text, /7\.0/);
  for (const invalid of [{...priced,purchasing_json:null}, {...priced,purchasing_json:'bad'},
    {...job(),purchasing_json:priced.purchasing_json}]) {
    const bad = await worker({stock_job:()=>JSON.stringify(invalid)});
    const outcome = await bad(msg);
    assert.equal(outcome.type, 'error'); assert.equal(outcome.files, undefined);
  }
});

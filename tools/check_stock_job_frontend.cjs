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

function reviewJob() {
  const quote = { offer_id:'supplier/item', observed_on:'2026-10-20', observed_at:'2026-10-20T12:00:00.000000001Z', source:'test:quoted-item' };
  const chosen = { quote, order_cost:7, quantity_sufficiency_observed:false };
  const purchasing = { currency:'USD', comparison_scope:'admitted_whole_item_offers_only',
    least_observed_independent_order_total:7, complete_observed_quantity_coverage:false,
    stocks:[{ identity:{stock_index:0,stock_id:'selected-item',used:true}, offers:[chosen],
      least_observed_offer_id:quote.offer_id, least_observed_order_cost:7 },
      {identity:{stock_index:1,stock_id:'unpriced-item',used:false}, offers:[],
        least_observed_offer_id:null, least_observed_order_cost:null,
        binding_refusal:{message:'Stock snapshot changed; price is unknown.'},
        source_exclusions:[{quote:{...quote,offer_id:'old-price'},reason:'expired'}],
        stock_exclusions:[{quote:{...quote,offer_id:'wrong-item'},reason:'Exact material lock missing.'}],
        source_refusals:[{offer_id:'broken-source',refusal:{message:'Unqualified sale unit.'}}]}] };
  const plan = {schema:'ftx-finite-stock-plan/1',pattern_sha256:'pattern-lock',request_sha256:'request-lock',
    markers:[{id:'selected-item',placements:[{},{}],gross_area_m2:1}, {id:'unpriced-item',placements:[],gross_area_m2:0.5}],
    candidates:45,work_used:1603,purchasing,request:{pricing:{market:{currency:'USD',region:'declared market',quality:'A',
      terms:'merchandise only',as_of:'2026-10-20',as_of_at:'2026-10-20T12:30:00Z',max_age_days:1}}},
    optimization:{...optimization,objective:'minimize_purchase_cost',baseline:{stock_count:1,gross_area_m2:2,purchase_cost:100},
      selected:{stock_count:1,gross_area_m2:1,purchase_cost:7}}};
  return {...job(),stock_index:0,plan_json:JSON.stringify(plan),purchasing_json:JSON.stringify(purchasing)};
}
test('offline stock review distinguishes selected costs, excluded items and incomplete proof without changing receipts', async () => {
  const native = reviewJob();
  const send = await worker({stock_job:()=>JSON.stringify(native)});
  const result = await send(msg);
  assert.equal(result.type,'result',result.message);
  assert.equal(result.files[0].text,native.plan_json);
  assert.equal(result.files.find(f=>f.name === 'stock-purchasing.json').text,native.purchasing_json);
  const review = result.files.find(f=>f.name === 'stock-review.html');
  assert.equal(review.mime,'text/html');
  for (const text of ['Lower quoted purchase cost','100 USD','7 USD','Search budget reached',
    'Excluded from priced search','Stock snapshot changed; price is unknown.','Excluded: expired',
    'Exact material lock missing.','Unqualified sale unit.','Unknown','merchandise only',
    '2026-10-20T12:00:00.000000001Z','2026-10-20T12:30:00Z','not reserved']) assert.ok(review.text.includes(text),text);
  assert.ok(!review.text.includes('No better layout remains'));
  assert.ok(!review.text.includes('<td>0 USD</td>'));
  assert.ok(!/<script\b|<iframe\b|<img\b|<a\b/i.test(review.text));
  const plan = JSON.parse(native.plan_json);
  plan.optimization.budget_exhausted = false; plan.optimization.lattice_optimal = true;
  plan.optimization.termination = 'lattice_exhausted';
  plan.optimization.baseline.purchase_cost = 99.99999999999999;
  const proved = await worker({stock_job:()=>JSON.stringify({...native,plan_json:JSON.stringify(plan)})});
  const html = (await proved(msg)).files.find(f=>f.name === 'stock-review.html').text;
  assert.match(html,/No better layout remains on the declared placement grid/);
  assert.match(html,/not a continuous-space global optimum/);
  assert.match(html,/≈100 USD/);
  assert.match(html,/original JSON retains full precision/);
  assert.ok(!html.includes('Search budget reached'));
});

test('stock review escapes source text and keeps unavailable prices unknown on legacy jobs', async () => {
  const native = reviewJob(), plan = JSON.parse(native.plan_json);
  const hostile = '<script>alert("source")</script>&<img src="https://example.invalid/a" onerror="x()">';
  plan.purchasing.stocks[0].offers[0].quote.source = hostile;
  plan.request.pricing.market.terms = hostile;
  const send = await worker({stock_job:()=>JSON.stringify({...native,plan_json:JSON.stringify(plan)})});
  const review = (await send(msg)).files.find(f=>f.name === 'stock-review.html').text;
  assert.ok(!review.includes(hostile)); assert.ok(!/<script\b|<img\b/i.test(review));
  assert.match(review,/&lt;script&gt;alert\(&quot;source&quot;\)&lt;\/script&gt;/);
  assert.match(review,/Content-Security-Policy/);
  assert.match(review,/default-src 'none'/);
  delete plan.purchasing; delete plan.optimization; delete plan.request.pricing;
  const legacy = await worker({stock_plan:()=>JSON.stringify(plan),stock_residual:()=>native.residual_json,stock_svg:()=>'<svg/>'});
  const old = await legacy(msg);
  const html = old.files.find(f=>f.name === 'stock-review.html').text;
  assert.match(html,/Purchase costs were not evaluated/);
  assert.match(html,/not a minimum-cost or minimum-waste claim/);
  assert.match(html,/Unknown prices must not be interpreted as free stock/);
  assert.ok(!html.includes('USD'));
});

test('review limits or broken display correspondence preserve a complete stock job', async () => {
  for (const invalid of ['field','output','identity','currency']) {
    const native = reviewJob(), plan = JSON.parse(native.plan_json);
    if (invalid === 'field') plan.request.pricing.market.terms = 'x'.repeat(32769);
    if (invalid === 'output') plan.purchasing.stocks[0].offers = Array.from({length:160}, (_,i) => ({
      ...plan.purchasing.stocks[0].offers[0], quote:{offer_id:String(i),source:'x'.repeat(32768)}}));
    if (invalid === 'identity') plan.purchasing.stocks[0].identity.stock_id = 'different-item';
    if (invalid === 'currency') plan.request.pricing.market.currency = 'EUR';
    native.plan_json = JSON.stringify(plan);
    const send = await worker({stock_job:()=>JSON.stringify(native)});
    const result = await send(msg);
    assert.equal(result.type,'result',invalid);
    assert.equal(result.files[0].text,native.plan_json);
    assert.equal(result.files[1].text,native.residual_json);
    assert.ok(!result.files.some(f=>f.name === 'stock-review.html'));
    assert.ok(result.files.some(f=>f.name === 'stock-review-refusal.json'));
  }
});

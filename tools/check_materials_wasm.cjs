// Execute the actual browser WASM and worker under Node; this is not browser navigation/DOM proof.
// Build web/pkg and ftx-materials first, then:
// node --experimental-vm-modules tools/check_materials_wasm.cjs --native target/debug/ftx-materials
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const vm = require('node:vm');
const {pathToFileURL} = require('node:url');
const {execFileSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const root = path.resolve(__dirname,'..'), arg = process.argv.indexOf('--native');
const native = path.resolve(arg < 0 ? path.join(root,'target/debug/ftx-materials') : process.argv[arg+1]);
const fixture = name => path.join(root,'tests/fixtures/materials',name);
const read = name => fs.readFileSync(fixture(name),'utf8');
const parse = JSON.parse;
(async () => {
  const ftx = await import(pathToFileURL(path.join(root,'web/pkg/ftx_wasm.js')).href);
  ftx.initSync({module:fs.readFileSync(path.join(root,'web/pkg/ftx_wasm_bg.wasm'))});
  const sent = [], context = vm.createContext({ Uint8Array, TextEncoder, WebAssembly, JSON, Error, self:{}, postMessage:m=>sent.push(m) });
  const imports = new vm.SyntheticModule(Object.keys(ftx), function () {
    for (const [name,value] of Object.entries(ftx)) this.setExport(name,value);
  },{context});
  const worker = new vm.SourceTextModule(fs.readFileSync(path.join(root,'web/materials-worker.js'),'utf8'),{context});
  await worker.link(()=>imports); await worker.evaluate();
  const send = async (type,inputs={}) => {
    await context.self.onmessage({data:{type,request:sent.length+1,...inputs}});
    const result=sent.at(-1); assert.equal(result.type,'result',JSON.stringify(result)); return result;
  };
  const work = fs.mkdtempSync(path.join(os.tmpdir(),'ftx-materials-wasm-'));
  const write = (n,v) => {const p=path.join(work,n);fs.writeFileSync(p,JSON.stringify(v));return p;};
  const invoke = (...args) => execFileSync(native,args,{encoding:'utf8',maxBuffer:64*1024*1024});
  assert.ok((await send('catalog',{query:'cotton'})).result.materials.length > 0);
  const partial = (await send('inspect',{selection:JSON.stringify({id:'fsri/hfm/cotton-sheet/dry/220330-r1',allow_estimates:true})})).result;
  assert.equal(partial.capabilities.steady_conduction.ready,true);
  assert.equal(partial.capabilities.transient_thermal.ready,false);
  assert.equal(partial.capabilities.mechanical_coefficients.ready,false);
  const originalText=read('synthetic-sewing-pattern.json').replace('"a": [0, 0]','"a": [-0.0, 0.0]');
  const original=parse(originalText), originalPath=path.join(work,'original.json');fs.writeFileSync(originalPath,originalText);
  const options={copies:1,gap_mm:2,edge_margin_mm:0,end_margin_mm:0};
  const cutOptions=write('cut-options.json',options);
  let previousLength=0;
  for (const [width,thickness] of [[0.1,0.4],[0.06,2]]) {
    const selection=structuredClone(original.fabric.material);
    for (const [key,value,unit] of [['width',width,'m'],['thickness',thickness,'mm'],['areal_density',0.2,'kg/m2']]) {
      selection.record.properties[key]={value,unit,evidence:'estimate',source:'test',method:'Synthetic runtime test, not measured textile data'};
    }
    const bind=(await send('bind',{pattern:originalText,changes:JSON.stringify({schema:'ftx-material-bindings/1',default:selection})})).result;
    assert.deepEqual(bind.pattern.pieces,original.pieces);
    assert.match(bind.pattern_json,/-0\.0/);
    assert.equal(createHash('sha256').update(bind.pattern_json).digest('hex'),bind.pattern_sha256);
    const p=path.join(work,`pattern-${width}.json`);fs.writeFileSync(p,bind.pattern_json);
    const s=write(`selection-${width}.json`,selection), nativeBound=path.join(work,`native-${width}.json`);
    invoke('bind',originalPath,s,nativeBound);
    assert.deepEqual(bind.pattern,parse(fs.readFileSync(nativeBound,'utf8')));
    const marker=await send('cut',{pattern:bind.pattern_json,options:JSON.stringify(options)});
    assert.deepEqual(marker.result,parse(invoke('cut-plan',p,cutOptions)));
    const svg=path.join(work,`roll-${width}.svg`);invoke('cut-svg',p,cutOptions,'0',svg);
    assert.equal(marker.files[1].text,fs.readFileSync(svg,'utf8'));
    assert.ok(marker.result.rolls[0].length_m>previousLength);previousLength=marker.result.rolls[0].length_m;
    if(thickness<1) {
      const sew=await send('sew',{pattern:bind.pattern_json,options:read('synthetic-sewing-options.json')});
      assert.deepEqual(sew.result,parse(invoke('sew-stitches',p,fixture('synthetic-sewing-options.json'))));
      assert.equal(sew.result.plan.hardware_execution_authorized,false);
    } else {
      await context.self.onmessage({data:{type:'sew',pattern:bind.pattern_json,options:read('synthetic-sewing-options.json')}});
      assert.equal(sent.at(-1).type,'error');assert.match(sent.at(-1).message,/stack/);
    }
    const quotes={schema:'ftx-textile-quotes/1',quotes:[]},market={currency:'USD',region:'US',quality:'test',terms:'test',as_of:'2026-10-15',max_age_days:30};
    const price=await send('price',{pattern:bind.pattern_json,options:JSON.stringify(options),quotes:JSON.stringify(quotes),market:JSON.stringify(market)});
    assert.deepEqual(price.result,parse(invoke('cut-price',p,cutOptions,write(`quotes-${width}.json`,quotes),write(`market-${width}.json`,market))));
    assert.equal(price.result.complete_offer_coverage,false);assert.equal(price.result.least_observed_independent_order_total,null);
  }
  const report=read('synthetic-steady-surface-report.json');
  const heat=await send('heat',{report,conditions:read('steady-surface-conditions.json')});
  assert.deepEqual(heat.result,parse(invoke('thermal-steady-surface',fixture('synthetic-steady-surface-report.json'),fixture('steady-surface-conditions.json'))));
  const coverage=await send('coverage',{report,map:read('steady-coverage-map.json'),conditions:read('steady-coverage-conditions.json')});
  assert.deepEqual(coverage.result,parse(invoke('thermal-steady-coverage',fixture('synthetic-steady-surface-report.json'),fixture('steady-coverage-map.json'),fixture('steady-coverage-conditions.json'))));
  const opticalStack=parse(read('synthetic-asymmetric-exterior.json')), opticalResults=[];
  for (const reversed of [false,true]) {
    const input=structuredClone(opticalStack), faces=input.layers[0].record.optical_faces;
    if(reversed) [faces.positive,faces.negative]=[faces.negative,faces.positive];
    const actual=parse(ftx.thermal_steady(JSON.stringify(input)));
    assert.deepEqual(actual,parse(invoke('thermal-steady',write(`optical-${reversed}.json`,input))));
    assert.equal(actual.exterior_exchange.material_face.side,'positive');
    assert.equal(actual.exterior_exchange.material_face.face,reversed ? 'reverse' : 'finish');
    opticalResults.push(actual);
  }
  assert.notEqual(opticalResults[0].power_w,opticalResults[1].power_w);
  assert.notEqual(opticalResults[0].request_sha256,opticalResults[1].request_sha256);
  delete opticalStack.layers[0].record.optical_faces;
  assert.throws(()=>ftx.thermal_steady(JSON.stringify(opticalStack)),/no source-sheet optical face mapping/);
  await context.self.onmessage({data:{type:'fold',bytes:new ArrayBuffer(8),pattern:JSON.stringify(original),measurements:'{}',fit:'{}'}});
  assert.equal(sent.at(-1).type,'error');
  console.log('PASS: compiled WASM + actual worker/native equality for binding, stock, SVG, sewing, purchasing and frozen thermal surface/coverage; automatic asymmetric optical faces, material-driven changes and malformed model refusal.');
  console.log('Browser navigation and successful model-backed drape NOT asserted. Test outputs:',work);
})().catch(e=>{console.error(e);process.exitCode=1;});

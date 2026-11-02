// Request-authoring checks; the canonical Rust consumers still supply numerical evidence.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const modulePromise = (async () => {
  const module = new vm.SourceTextModule(fs.readFileSync(path.join(__dirname, '../web/materials-study-builder.js'), 'utf8'));
  await module.link(() => { throw Error('The builder must not import dependencies.'); });
  await module.evaluate();
  return module.namespace;
})();
const selection = (id = 'cloth', hash = 'a', policy = false) =>
  ` {"id":${JSON.stringify(id)},"expected_sha256":"${hash.repeat(64)}","allow_estimates":${policy},"record":{"id":${JSON.stringify(id)},"seed":18446744073709551615,"x":-0.0,"value":1.0,"label":"é 🧵"}}\n`;
const cutting = () => ({copies:1,gap_mm:2,edge_margin_mm:5,end_margin_mm:10});
const mass = (complete = true) => ({schema:'ftx-assembly-mass/1',inventory_complete:complete,extras:[]});
const state = () => ({candidates:[{key:'alternative',selection:selection()}],target:{scope:'default'},pieceIds:['front','lining'],
  cutting:cutting(),objectives:[{metric:'stock_area_m2',direction:'minimize'}],basis:'nominal'});

test('guided choices retain the unchanged baseline, exact frozen bytes and independent default assignment', async () => {
  const {buildMaterialStudyRequest:build,MAX_GUIDED_STUDY_CANDIDATES} = await modulePromise;
  assert.equal(MAX_GUIDED_STUDY_CANDIDATES,7);
  const input=state(), before=JSON.stringify(input), raw=build(input), result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.ok(raw.includes(input.candidates[0].selection));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"x":-0\.0/);assert.match(raw,/"value":1\.0/);
  assert.deepEqual(result.variants[0],{key:'baseline',changes:{schema:'ftx-material-bindings/1'}});
  assert.deepEqual(Object.keys(result.variants[1].changes),['schema','default']);
  assert.deepEqual(result.cutting,input.cutting);
  assert.equal(Object.hasOwn(result,'assembly_mass'),false);
  assert.deepEqual(result.comparison,{basis:'nominal',objectives:input.objectives,constraints:[],required_consumers:[],allow_mixed_protocols:false});
});

test('piece targets preserve exact source IDs and every inserted source snapshot', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();input.pieceIds=['__proto__','front "quoted"','lining'];input.target={scope:'pieces',ids:['__proto__','front "quoted"']};
  const raw=build(input), changes=JSON.parse(raw).variants[1].changes;
  assert.deepEqual(Object.keys(changes.pieces),input.target.ids);
  assert.equal(Object.hasOwn(changes,'default'),false);
  assert.equal(raw.split(input.candidates[0].selection).length-1,2);
  for(const target of [{scope:'pieces',ids:[]},{scope:'pieces',ids:['front','front']},{scope:'pieces',ids:['missing']},{scope:'default',ids:[]},{scope:'other'}]) {
    assert.throws(()=>build({...state(),target}));
  }
  assert.throws(()=>build({...state(),pieceIds:['front','front']}),/unique/);
});

test('reported bounds require the explicit complete assembly inventory and preserve raw extra evidence', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();delete input.cutting;
  input.assemblyMass=mass();input.basis='reported_bounds';input.objectives=[{metric:'assembly_mass_kg',direction:'minimize'}];
  const result=JSON.parse(build(input));assert.equal(Object.hasOwn(result,'cutting'),false);assert.deepEqual(result.assembly_mass,mass());
  assert.throws(()=>build({...input,assemblyMass:mass(false)}),/complete inventory/);
  const undeclared=mass();delete undeclared.inventory_complete;
  assert.throws(()=>build({...input,assemblyMass:undeclared}),/missing/);
  assert.throws(()=>build({...state(),basis:'reported_bounds'}),/assembly mass only/);
  const raw=' {"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[{"id":"closure","seed":18446744073709551615,"value":1.0,"x":-0.0}]}\n';
  assert.ok(build({...input,assemblyMass:raw}).includes(raw));
  assert.throws(()=>build({...input,assemblyMass:JSON.parse(raw)}),/original inventory JSON/);
  assert.throws(()=>build({...input,assemblyMass:raw.replace('"extras":[','"extras":[{"id":"closure"},')}),/unique/);
  const partial=JSON.parse(build({...state(),assemblyMass:mass(false)}));
  assert.equal(partial.assembly_mass.inventory_complete,false);
  assert.deepEqual(partial.comparison.required_consumers,[]);
});

test('request authoring refuses missing consumers, unsupported or duplicate objectives and inconsistent allowances', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const objectives of [[],[{metric:'assembly_mass_kg',direction:'minimize'}],[{metric:'wrong',direction:'minimize'}],
    [{metric:'stock_area_m2',direction:'lowest'}],[{metric:'stock_area_m2',direction:'minimize'},{metric:'stock_area_m2',direction:'maximize'}]]) {
    assert.throws(()=>build({...state(),objectives}));
  }
  const absent=state();delete absent.cutting;assert.throws(()=>build(absent),/Choose cutting/);
  assert.throws(()=>build({...absent,assemblyMass:mass()}),/requested cutting/);
  assert.throws(()=>build({...state(),assemblyMass:{...mass(),seam_allowance_mm:4}}),/same seam allowance/);
  const both={...state(),cutting:{...cutting(),seam_allowance_mm:4},assemblyMass:{...mass(),seam_allowance_mm:4},
    objectives:[{metric:'stock_area_m2',direction:'minimize'},{metric:'stock_mass_kg',direction:'minimize'},{metric:'assembly_mass_kg',direction:'maximize'}]};
  assert.equal(JSON.parse(build(both)).comparison.objectives.length,3);
});

test('numeric and object fields are explicit, finite and within the native copy limit', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const copies of [0,-1,4097,1.5,'1',NaN,Infinity]) assert.throws(()=>build({...state(),cutting:{...cutting(),copies}}),/copies/);
  for(const key of ['gap_mm','edge_margin_mm','end_margin_mm','seam_allowance_mm']) {
    for(const value of [-1,NaN,Infinity,'2',undefined]) assert.throws(()=>build({...state(),cutting:{...cutting(),[key]:value}}),/finite/);
  }
  assert.equal(JSON.parse(build({...state(),cutting:{copies:4096,gap_mm:0,edge_margin_mm:0,end_margin_mm:0,seam_allowance_mm:null}})).cutting.copies,4096);
  const inherited=Object.create({copies:1});Object.assign(inherited,{gap_mm:0,edge_margin_mm:0,end_margin_mm:0});
  assert.throws(()=>build({...state(),cutting:inherited}),/missing/);
  const inheritedState=Object.create({cutting:cutting()});Object.assign(inheritedState,state());delete inheritedState.cutting;
  assert.throws(()=>build(inheritedState),/Choose cutting/);
  assert.throws(()=>build({...state(),cutting:{...cutting(),typo:1}}),/unsupported/);
  assert.throws(()=>build({...state(),unexpected:true}),/unsupported/);
});

test('choices reject missing locks, mutated shapes, duplicates and requests beyond the bounded count', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();input.candidates=Array.from({length:7},(_,i)=>({key:`choice-${i}`,selection:selection(`cloth-${i}`,'abcdef0'[i])}));
  assert.equal(JSON.parse(build(input)).variants.length,8);
  assert.throws(()=>build({...input,candidates:[...input.candidates,{key:'eighth',selection:selection('eighth','1')}]}),/1–7/);
  assert.throws(()=>build({...input,candidates:[]}),/1–7/);
  for(const key of ['','baseline']) assert.throws(()=>build({...state(),candidates:[{key,selection:selection()}]}));
  assert.throws(()=>build({...state(),candidates:[{key:'one',selection:selection()},{key:'one',selection:selection('different','b')}]}),/names.*unique/);
  assert.throws(()=>build({...state(),candidates:[{key:'one',selection:selection()},{key:'two',selection:selection()}]}),/already included/);
  // Same source with different explicit estimate admission remains a distinguishable alternative.
  assert.equal(JSON.parse(build({...state(),candidates:[{key:'strict',selection:selection()},{key:'estimates',selection:selection('cloth','a',true)}]})).variants.length,3);
  for(const raw of ['null','[]','{bad','{}','{"id":"cloth"}',selection().replace('"record":','"wrong":'),
    selection().replace('"allow_estimates":false','"allow_estimates":"false"'),selection().replace('"record":{"id":"cloth"','"record":{"id":"different"'),
    selection().replace('a'.repeat(64),'not-a-lock')]) assert.throws(()=>build({...state(),candidates:[{key:'bad',selection:raw}]}));
});

test('repeated snapshots reserve UTF-8 bytes before constructing an oversized request', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state(),ids=Array.from({length:1024},(_,i)=>`piece-${i}`);
  input.pieceIds=ids;input.target={scope:'pieces',ids};
  input.candidates[0].selection=selection().replace('é 🧵','é'.repeat(8192));
  assert.throws(()=>build(input),/16 MiB/);
  input.target={scope:'default'};
  assert.ok(build(input).includes(input.candidates[0].selection));
});

const processSources = () => ({
  cutting: ' {"copies":2,"gap_mm":2,"edge_margin_mm":5,"end_margin_mm":10,"program":{"source":{"sequence":18446744073709551615,"value":1.0,"x":-0.0}}}\r\n',
  sewing: ' {"require_qualification":true,"production":{"copies":2},"consumables":{"purchasing":{"source":{"sequence":18446744073709551615,"x":-0.0}}}}\r\n',
  purchasing: {
    quotes: ' {"schema":"ftx-textile-quotes/1","source":{"label":"é 🧵","sequence":18446744073709551615,"value":1.0}}\r\n',
    market: ' {"currency":"USD","as_of":"2026-10-01","source":{"sequence":18446744073709551615,"x":-0.0}}\r\n',
  },
});

test('raw cutting preserves omitted native optional defaults and requires the four declared roll controls', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const source=' {\r\n"copies":1,"gap_mm":0.0,"edge_margin_mm":0.0,"end_margin_mm":0.0\r\n}\r\n';
  const raw=build({...state(),cutting:source}),result=JSON.parse(raw);
  assert.ok(raw.includes(source));
  assert.deepEqual(Object.keys(result.cutting),['copies','gap_mm','edge_margin_mm','end_margin_mm']);
  for(const key of ['width_basis','seam_allowance_mm','maximum_marker_length_m','nesting','program']) {
    assert.equal(Object.hasOwn(result.cutting,key),false,'Native optional defaults are not inserted into the original request');
  }
  for(const policy of ['{}','{"program":{}}',source.replace('"copies":1,','')]) {
    assert.throws(()=>build({...state(),cutting:policy}),/missing/);
  }
  for(const selection of ['{}','{"schema":"ftx-sewing-selection/6"}','{"options":[]}']) {
    assert.throws(()=>build({...state(),sewingSelection:selection}),/complete template options/);
  }
});

test('guided production and purchasing reuse every original policy without rewriting source numbers', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),...processSources(),cutAndSewPurchasing:true,
    objectives:[{metric:'cut_and_sew_selected_order_cost',direction:'minimize'},
      {metric:'sewing_modeled_production_seconds',direction:'minimize'},{metric:'cutting_modeled_path_seconds',direction:'minimize'}],
    constraints:[{metric:'selected_observed_order_cost',minimum:0,maximum:50,basis:'nominal'}],
    requiredConsumers:['sewing','cutting_program','cut_and_sew_purchasing']};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  for(const source of [input.cutting,input.sewing,input.purchasing.quotes,input.purchasing.market])assert.ok(raw.includes(source));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"value":1\.0/);assert.match(raw,/"x":-0\.0/);
  assert.deepEqual(result.cutting,JSON.parse(input.cutting));assert.deepEqual(result.sewing,JSON.parse(input.sewing));
  assert.deepEqual(result.purchasing,{quotes:JSON.parse(input.purchasing.quotes),market:JSON.parse(input.purchasing.market)});
  assert.equal(result.cut_and_sew_purchasing,true);assert.equal(Object.hasOwn(result,'sewing_selection'),false);
  assert.deepEqual(result.comparison,{basis:'nominal',objectives:input.objectives,constraints:input.constraints,
    required_consumers:input.requiredConsumers,allow_mixed_protocols:false});
});

test('sewing-only selection studies preserve exact recipe searches and do not infer a cutting consumer', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();delete input.cutting;
  input.sewingSelection=' {"schema":"ftx-sewing-selection/6","options":{"consumables":{"purchasing":{}}},"max_plan_evaluations":8,"source":{"seed":18446744073709551615,"value":1.0}}\r\n';
  input.objectives=[{metric:'sewing_parallel_makespan_seconds',direction:'minimize'},
    {metric:'thread_required_length_m',direction:'minimize'},{metric:'thread_observed_order_cost',direction:'minimize'}];
  input.requiredConsumers=['sewing','sewing_consumables','sewing_purchasing'];
  const raw=build(input),result=JSON.parse(raw);assert.ok(raw.includes(input.sewingSelection));
  assert.equal(Object.hasOwn(result,'cutting'),false);assert.equal(Object.hasOwn(result,'sewing'),false);
  assert.equal(Object.hasOwn(result,'purchasing'),false);assert.equal(Object.hasOwn(result,'cut_and_sew_purchasing'),false);
  assert.deepEqual(result.sewing_selection,JSON.parse(input.sewingSelection));
  assert.throws(()=>build({...input,sewing:'{}'}),/not both/);
  assert.throws(()=>build({...input,sewingSelection:'{"options":null}'}),/complete template options/);
});

test('manufacturing objectives and required consumers cannot omit their source inputs', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const goal=metric=>[{metric,direction:'minimize'}];
  for(const metric of ['cutting_modeled_path_seconds','sewing_modeled_production_seconds','sewing_parallel_makespan_seconds',
    'selected_observed_order_cost','thread_required_length_m','thread_observed_order_cost','cut_and_sew_selected_order_cost']) {
    assert.throws(()=>build({...state(),objectives:goal(metric)}),/requires requested/);
  }
  for(const consumer of ['cutting_program','sewing','purchasing','sewing_consumables','sewing_purchasing','cut_and_sew_purchasing','assembly_mass']) {
    assert.throws(()=>build({...state(),requiredConsumers:[consumer]}),/requires requested/);
  }
  const sewingOnly={...state(),sewing:'{}',objectives:goal('thread_required_length_m')};
  assert.throws(()=>build(sewingOnly),/sewing_consumables/);
  assert.throws(()=>build({...sewingOnly,sewing:'{"consumables":{}}',objectives:goal('thread_observed_order_cost')}),/sewing_purchasing/);
  for(const extra of [{},{sewing:'{"consumables":{"purchasing":{}}}'},{purchasing:processSources().purchasing}]) {
    assert.throws(()=>build({...state(),...extra,cutAndSewPurchasing:true}),/requires roll purchasing and sewing thread purchasing/);
  }
  const noCutting={...state(),...processSources()};delete noCutting.cutting;
  assert.throws(()=>build(noCutting),/purchasing requires roll cutting/);
  assert.throws(()=>build({...state(),...processSources(),cutAndSewPurchasing:false,objectives:goal('cut_and_sew_selected_order_cost')}),/cut_and_sew_purchasing/);
  assert.equal(JSON.parse(build({...state(),cutAndSewPurchasing:false})).cut_and_sew_purchasing,false);
});

test('reported mass objectives compose with explicit nominal manufacturing guards and required qualification', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),...processSources(),assemblyMass:mass(),basis:'reported_bounds',
    objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:[{metric:'sewing_modeled_production_seconds',maximum:3600,basis:'nominal'},
      {metric:'selected_observed_order_cost',minimum:0,maximum:50,basis:'nominal'}],requiredConsumers:['sewing','purchasing']};
  const result=JSON.parse(build(input));assert.deepEqual(result.comparison.constraints,input.constraints);
  assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  assert.throws(()=>build({...input,constraints:[{metric:'sewing_modeled_production_seconds',maximum:3600}]}),/assembly mass only/);
  assert.throws(()=>build({...input,constraints:[{metric:'selected_observed_order_cost',maximum:50,basis:'reported_bounds'}]}),/assembly mass only/);
  assert.throws(()=>build({...input,objectives:[{metric:'cutting_modeled_path_seconds',direction:'minimize'}]}),/assembly mass only/);
  assert.equal(JSON.parse(build({...state(),constraints:[{metric:'stock_area_m2',minimum:null,maximum:2}]})).comparison.constraints[0].minimum,null);
});

test('manufacturing guards refuse ambiguous bounds, duplicate policies and malformed retained files', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const constraint of [{metric:'stock_area_m2'},{metric:'stock_area_m2',minimum:null,maximum:null},
    {metric:'stock_area_m2',minimum:3,maximum:2},{metric:'stock_area_m2',maximum:NaN},
    {metric:'stock_area_m2',maximum:Infinity},{metric:'stock_area_m2',maximum:'2'},
    {metric:'stock_area_m2',maximum:undefined},{metric:'stock_area_m2',maximum:2,basis:null},
    {metric:'stock_area_m2',maximum:2,basis:undefined},{metric:'stock_area_m2',maximum:2,basis:'guess'},
    {metric:'stock_area_m2',maximum:2,typo:1},{metric:'wrong',maximum:2}]) {
    assert.throws(()=>build({...state(),constraints:[constraint]}));
  }
  const guard={metric:'stock_area_m2',maximum:2};
  assert.throws(()=>build({...state(),constraints:[guard,guard]}),/unique/);
  assert.throws(()=>build({...state(),constraints:Array(65).fill(guard)}),/64/);
  assert.throws(()=>build({...state(),requiredConsumers:['cutting','cutting']}),/unique/);
  assert.throws(()=>build({...state(),requiredConsumers:['not_a_consumer']}),/Unsupported/);
  assert.throws(()=>build({...state(),requiredConsumers:Array(17).fill('cutting')}),/16/);
  for(const key of ['constraints','requiredConsumers'])for(const value of [null,{},''])assert.throws(()=>build({...state(),[key]:value}));
  for(const key of ['sewing','sewingSelection'])for(const value of [{},'null','[]','{bad'])assert.throws(()=>build({...state(),[key]:value}));
  for(const purchasing of [{quotes:'{}'},{quotes:{},market:'{}'},{quotes:'{}',market:'null'},
    {quotes:'{}',market:'{}',unknown:true}])assert.throws(()=>build({...state(),purchasing}));
  for(const value of [null,'true',1,undefined])assert.throws(()=>build({...state(),cutAndSewPurchasing:value}),/boolean/);
  assert.throws(()=>build({...state(),cutting:{...cutting(),program:{}}}),/unsupported/,'source-bearing policies require original text');
});

test('the combined manufacturing sources share the existing UTF-8 request budget', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const raw=JSON.stringify({metadata:'é'.repeat(3*1024*1024)});
  assert.throws(()=>build({...state(),sewing:raw,purchasing:{quotes:raw,market:raw}}),/16 MiB/);
});

const finiteSource = () => ` {\r\n"schema":"ftx-finite-stock-request/1","stocks":[{"id":"hide-1","material":${selection()},
  "outline_mm":[[-0.0,0.0],[100.0,0.0],[100.0,100.0],[0.0,100.0]],"grain_mm":[[0.0,0.0],[0.0,1.0]],
  "geometry_source":{"reference":"test:stock","accessed_on":"2026-11-02","note":"Synthetic inventory"}}],
  "options":{"copies":2,"gap_mm":1.0,"boundary_margin_mm":1.0,"step_mm":1.0,"work_limit":10000,"program":{}}}\r\n`;
const jobSource = () => ` {\r\n"schema":"ftx-cutting-job/2","routes":[{"piece":"lining","stock":"roll"}],
  "material_routes":[{"piece":"front","choices":[{"material_sha256":"${'a'.repeat(64)}","stock":"finite_stock"}]}],
  "roll":${processSources().cutting},"finite_stock":${finiteSource()},
  "purchasing":{"quotes":${processSources().purchasing.quotes},"market":${processSources().purchasing.market},"finite_bindings":[]}}\r\n`;
function stockState(mode = 'finiteStock') {
  const input=state();delete input.cutting;
  input[mode]=mode==='finiteStock'?finiteSource():jobSource();
  input.objectives=[{metric:mode==='finiteStock'?'used_stock_count':'cutting_job_stock_area_m2',direction:'minimize'}];
  return input;
}

test('guided finite inventory keeps physical stock source bytes and admits its own objectives', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=stockState();
  input.objectives=['used_stock_count','used_stock_gross_area_m2','used_stock_mass_kg','finite_stock_modeled_path_seconds']
    .map(metric=>({metric,direction:'minimize'}));
  input.constraints=[{metric:'used_stock_count',maximum:2}];
  input.requiredConsumers=['finite_stock','finite_stock_program'];
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.ok(raw.includes(input.finiteStock));
  assert.equal(raw.split(input.finiteStock).length-1,1,'The shared inventory is supplied once, not consumed across alternatives');
  assert.match(raw,/18446744073709551615/);assert.match(raw,/\[-0\.0,0\.0\]/);assert.match(raw,/"gap_mm":1\.0/);
  assert.deepEqual(result.finite_stock,JSON.parse(input.finiteStock));
  for(const key of ['cutting','cutting_job','purchasing'])assert.equal(Object.hasOwn(result,key),false);
  assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);
  assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  assert.deepEqual(result.variants[0],{key:'baseline',changes:{schema:'ftx-material-bindings/1'}});
});

test('guided complete jobs retain routes and embedded market for combined fabric and thread orders', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...stockState('cuttingJob'),sewing:processSources().sewing,cutAndSewPurchasing:true,
    objectives:['cutting_job_stock_area_m2','cutting_job_stock_mass_kg','cutting_job_modeled_path_seconds',
      'cutting_job_selected_order_cost','cut_and_sew_selected_order_cost'].map(metric=>({metric,direction:'minimize'})),
    requiredConsumers:['cutting_job','cutting_job_program','cutting_job_purchasing','cut_and_sew_purchasing']};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.ok(raw.includes(input.cuttingJob));assert.ok(raw.includes(input.sewing));
  assert.equal(raw.split(input.cuttingJob).length-1,1);
  assert.deepEqual(result.cutting_job,JSON.parse(input.cuttingJob));
  assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  assert.equal(result.cut_and_sew_purchasing,true);
  for(const key of ['cutting','finite_stock','purchasing'])assert.equal(Object.hasOwn(result,key),false);
  const finiteOnly=JSON.parse(input.cuttingJob);
  finiteOnly.schema='ftx-cutting-job/1';delete finiteOnly.roll;delete finiteOnly.material_routes;
  finiteOnly.routes=input.pieceIds.map(piece=>({piece,stock:'finite_stock'}));
  const allFinite=JSON.stringify(finiteOnly);
  const changed=JSON.parse(build({...input,cuttingJob:allFinite}));
  assert.deepEqual(changed.cutting_job.routes,finiteOnly.routes);
  assert.equal(Object.hasOwn(changed.cutting_job,'roll'),false);
  assert.equal(changed.cut_and_sew_purchasing,true,'Whole finite-item pricing uses the ordinary complete job');
});

test('guided stock modes reject duplicate allocation and require the actual requested consumers', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const input of [{...state(),finiteStock:finiteSource()},{...state(),cuttingJob:jobSource()},
    {...stockState(),cuttingJob:jobSource()}])assert.throws(()=>build(input),/Choose one stock plan/);
  assert.throws(()=>build({...stockState('cuttingJob'),purchasing:processSources().purchasing}),/own purchasing/);
  assert.throws(()=>build({...stockState(),purchasing:processSources().purchasing}),/purchasing requires roll cutting/);
  assert.throws(()=>build({...stockState(),sewing:processSources().sewing,cutAndSewPurchasing:true}),/complete job/);
  for(const [mode,metric,consumer] of [
    ['finiteStock','finite_stock_modeled_path_seconds','finite_stock_program'],
    ['cuttingJob','cutting_job_modeled_path_seconds','cutting_job_program'],
    ['cuttingJob','cutting_job_selected_order_cost','cutting_job_purchasing'],
  ]) {
    const input=stockState(mode),source=JSON.parse(input[mode]);
    if(mode==='finiteStock')delete source.options.program;
    else {delete source.roll.program;delete source.finite_stock.options.program;delete source.purchasing;}
    input[mode]=JSON.stringify(source);
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]}),/requires requested/);
    assert.throws(()=>build({...input,requiredConsumers:[consumer]}),/requires requested/);
  }
  for(const [mode,metric,consumer] of [
    ['finiteStock','stock_area_m2','cutting'],['finiteStock','cutting_job_stock_area_m2','cutting_job'],
    ['cuttingJob','used_stock_mass_kg','finite_stock'],['cuttingJob','selected_observed_order_cost','purchasing'],
  ]) {
    assert.throws(()=>build({...stockState(mode),objectives:[{metric,direction:'minimize'}]}),/requires requested/);
    assert.throws(()=>build({...stockState(mode),requiredConsumers:[consumer]}),/requires requested/);
  }
  const withoutJobPrices=JSON.parse(jobSource());delete withoutJobPrices.purchasing;
  assert.throws(()=>build({...stockState('cuttingJob'),cuttingJob:JSON.stringify(withoutJobPrices),
    sewing:processSources().sewing,cutAndSewPurchasing:true}),/complete job/);
});

test('retained stock and job files need their exact schema and outer input shapes', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const mode of ['finiteStock','cuttingJob'])for(const raw of [null,{},'null','[]','{bad','{}']) {
    assert.throws(()=>build({...stockState(mode),[mode]:raw}));
  }
  const finite=JSON.parse(finiteSource());
  for(const change of [{schema:'ftx-finite-stock-request/2'},{stocks:{}},{options:null},{options:[]},{extra:true}]) {
    assert.throws(()=>build({...stockState(),finiteStock:JSON.stringify({...finite,...change})}));
  }
  const job=JSON.parse(jobSource());
  for(const change of [{schema:'ftx-cutting-job/3'},{roll:null,finite_stock:null},{roll:[]},
    {finite_stock:{...finite,options:null}},{purchasing:{}},{purchasing:{quotes:{},market:[]}},
    {purchasing:{quotes:{},market:{},unknown:true}},{unknown:true}]) {
    assert.throws(()=>build({...stockState('cuttingJob'),cuttingJob:JSON.stringify({...job,...change})}));
  }
  const nested={...job,roll:null,purchasing:null};
  const result=JSON.parse(build({...stockState('cuttingJob'),cuttingJob:JSON.stringify(nested)}));
  assert.equal(result.cutting_job.roll,null);assert.equal(result.cutting_job.purchasing,null);
});

test('complete assembly mass agrees with every selected finite and mixed-job allowance', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const finite=JSON.parse(finiteSource());finite.options.seam_allowance_mm=4;
  const input={...stockState(),finiteStock:JSON.stringify(finite),assemblyMass:{...mass(),seam_allowance_mm:4}};
  assert.equal(JSON.parse(build(input)).assembly_mass.seam_allowance_mm,4);
  assert.throws(()=>build({...input,assemblyMass:mass()}),/same seam allowance/);
  const job=JSON.parse(jobSource());job.finite_stock.options.seam_allowance_mm=4;
  const mixed={...stockState('cuttingJob'),cuttingJob:JSON.stringify(job),assemblyMass:input.assemblyMass};
  assert.throws(()=>build(mixed),/same seam allowance/,'The roll route cannot be ignored when finite stock agrees');
  job.roll.seam_allowance_mm=4;
  assert.equal(JSON.parse(build({...mixed,cuttingJob:JSON.stringify(job)})).assembly_mass.seam_allowance_mm,4);
  job.finite_stock.options.seam_allowance_mm=5;
  assert.throws(()=>build({...mixed,cuttingJob:JSON.stringify(job)}),/same seam allowance/,'The finite route cannot be ignored when roll stock agrees');
  for(const mode of ['finiteStock','cuttingJob'])for(const value of [-1,'4']) {
    const source=JSON.parse(stockState(mode)[mode]);
    if(mode==='finiteStock')source.options.seam_allowance_mm=value;else source.roll.seam_allowance_mm=value;
    assert.throws(()=>build({...stockState(mode),[mode]:JSON.stringify(source)}),/finite and nonnegative/);
  }
});

test('reported mass ranges compose with explicit nominal finite-stock and job limits only', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const [mode,metric] of [['finiteStock','used_stock_count'],['cuttingJob','cutting_job_selected_order_cost']]) {
    const input={...stockState(mode),assemblyMass:mass(),basis:'reported_bounds',
      objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],constraints:[{metric,maximum:20,basis:'nominal'}]};
    assert.deepEqual(JSON.parse(build(input)).comparison.constraints,input.constraints);
    assert.throws(()=>build({...input,constraints:[{metric,maximum:20}]}),/assembly mass only/);
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]}),/assembly mass only/);
  }
});

test('finite and mixed original sources share the same bounded UTF-8 output budget', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const text='é'.repeat(4*1024*1024),sewing=JSON.stringify({metadata:text});
  for(const mode of ['finiteStock','cuttingJob']) {
    const input=stockState(mode);input[mode]=input[mode].replace('é 🧵',text);
    assert.throws(()=>build({...input,sewing}),/16 MiB/);
  }
});

const scenarioState = () => ({allowNoFit:true,
  options:{spacing:12,sew_steps:12,drape_steps:36,step:1/24,max_settle_steps:0},
  maxMeshPoints:32768,maxVertexFrames:64000000});
const heatSource = () => ' {\r\n"schema":"ftx-steady-surface-conditions/1","boundaries":[{"inside_c":20.0,"outside_c":30.0,"inside_resistance_m2_k_w":-0.0,"outside_resistance_m2_k_w":1.0}]}\r\n';
const selfContact = () => ({dhat:0.001,thickness:0.002,kappa:100,mu:0.4,eps_v:0.001});

test('guided scenarios preserve exact fit and heat sources and only author declared numerical controls', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const fit=' {\r\n"schema":"fabric-fit/1","points":[],"source":{"seed":18446744073709551615,"x":-0.0,"value":1.0,"label":"é 🧵"}}\r\n';
  const input={...scenarioState(),allowNoFit:false,fitSpec:fit,surface:heatSource()};
  input.options.self_contact=selfContact();const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(fit));assert.ok(raw.includes(input.surface));
  assert.equal(result.schema,'ftx-drape-scenario/1');assert.deepEqual(result.options,input.options);
  assert.deepEqual(result.fit_spec,JSON.parse(fit));assert.deepEqual(result.surface,JSON.parse(input.surface));
  assert.equal(result.max_mesh_points,32768);assert.equal(result.max_vertex_frames,64000000);
  for(const key of ['motion','coverage','vapor_surface','allowNoFit','fitSpec'])assert.equal(Object.hasOwn(result,key),false);
  const ordinary=JSON.parse(build(scenarioState()));assert.deepEqual(ordinary.fit_spec,{});
  assert.equal(Object.hasOwn(ordinary,'surface'),false);assert.equal(Object.hasOwn(ordinary.options,'self_contact'),false);
  const off={...scenarioState(),options:{...scenarioState().options,self_contact:null}};
  assert.equal(JSON.parse(build(off)).options.self_contact,null);
});

test('guided scenarios require explicit no-fit consent and refuse wrong or malformed original sources', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  for(const allowNoFit of [false,undefined,null,'true',1])assert.throws(()=>build({...scenarioState(),allowNoFit}),/fit contract/);
  const absent=scenarioState();delete absent.allowNoFit;assert.throws(()=>build(absent),/missing/);
  for(const fitSpec of [{},'null','[]','{bad','\ufeff{}'])assert.throws(()=>build({...scenarioState(),fitSpec}));
  for(const surface of [{},'null','[]','{bad','{}','{"schema":"wrong","boundaries":[{}]}',
    '{"schema":"ftx-steady-surface-conditions/1","boundaries":[]}',
    '{"schema":"ftx-steady-surface-conditions/1","boundaries":{}}','\ufeff'+heatSource()]) {
    assert.throws(()=>build({...scenarioState(),surface}));
  }
  assert.throws(()=>build({...scenarioState(),unknown:true}),/unsupported/);
  assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,guess:true}}),/unsupported/);
});

test('guided numerical and self-contact policies stay within the native scenario allocation limits', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  for(const key of ['spacing','step'])for(const value of [0,-1,NaN,Infinity,'1']) {
    assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,[key]:value}}),/finite and positive/);
  }
  for(const key of ['sew_steps','drape_steps','max_settle_steps'])for(const value of [-1,257,1.5,NaN,Infinity,'1']) {
    assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,[key]:value}}),/integer/);
  }
  assert.throws(()=>build({...scenarioState(),options:{spacing:10,sew_steps:5,drape_steps:0,step:0.001,max_settle_steps:200}}),/at least six/);
  assert.throws(()=>build({...scenarioState(),options:{spacing:10,sew_steps:120,drape_steps:100,step:0.001,max_settle_steps:37}}),/exceed 256/);
  assert.equal(JSON.parse(build({...scenarioState(),options:{spacing:10,sew_steps:120,drape_steps:100,step:0.001,max_settle_steps:36}})).options.max_settle_steps,36);
  assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,step:Number.MAX_VALUE,max_settle_steps:1}}),/duration.*finite/);
  for(const [key,values] of [['maxMeshPoints',[2,32769,3.5,'3',NaN]],['maxVertexFrames',[0,64000001,1.5,'1',Infinity]]]) {
    for(const value of values)assert.throws(()=>build({...scenarioState(),[key]:value}),/budget.*integer/);
  }
  assert.equal(JSON.parse(build({...scenarioState(),maxMeshPoints:3,maxVertexFrames:1})).max_mesh_points,3);
  for(const key of ['dhat','thickness','kappa','mu','eps_v'])for(const value of [-1,NaN,Infinity,'1']) {
    assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,self_contact:{...selfContact(),[key]:value}}}),/finite/);
  }
  for(const key of ['dhat','kappa','eps_v'])assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,self_contact:{...selfContact(),[key]:0}}}),/positive/);
  for(const self_contact of [{},false,undefined,{...selfContact(),unknown:1}]) {
    assert.throws(()=>build({...scenarioState(),options:{...scenarioState().options,self_contact}}));
  }
  const noFriction={...scenarioState(),options:{...scenarioState().options,self_contact:{...selfContact(),thickness:0,mu:0}}};
  assert.equal(JSON.parse(build(noFriction)).options.self_contact.mu,0);
});

test('original scenario sources share a UTF-8 request budget before combined serialization', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const fitSpec=JSON.stringify({schema:'fabric-fit/1',source:'é'.repeat(4*1024*1024)});
  const surface=heatSource().replace('"boundaries":[','"boundaries":['+' '.repeat(9*1024*1024));
  assert.throws(()=>build({...scenarioState(),fitSpec,surface}),/16 MiB/);
});

test('scenario-only comparisons retain signed heat direction and cannot leak into manufacturing mode', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();delete input.cutting;
  input.objectives=[{metric:'independent_surface_power_w',direction:'maximize'},
    {metric:'simulated_cloth_mass_kg',direction:'minimize'},{metric:'max_absolute_edge_strain_pct',direction:'minimize'}];
  input.constraints=[{metric:'independent_surface_power_w',minimum:-50,maximum:-1},
    {metric:'max_absolute_edge_strain_pct',maximum:8}];input.requiredConsumers=['drape','surface'];
  const before=JSON.stringify(input),raw=build(input,{drape:true,surface:true}),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  for(const key of ['cutting','assembly_mass','sewing','scenario','surface','drape'])assert.equal(Object.hasOwn(result,key),false);
  assert.throws(()=>build(input),/Choose cutting/);
  assert.throws(()=>build({...input,cutting:cutting()}),/requested surface/);
  assert.throws(()=>build(input,{drape:true,surface:false}),/requested surface/);
  for(const context of [null,{},true,{drape:true},{drape:false,surface:false},{drape:false,surface:true},
    {drape:'true',surface:false},{drape:true,surface:'false'},{drape:true,surface:false,unknown:true}]) {
    assert.throws(()=>build(state(),context));
  }
  const drapeOnly={...input,objectives:[{metric:'max_absolute_edge_strain_pct',direction:'minimize'}],constraints:[],requiredConsumers:['drape']};
  assert.deepEqual(JSON.parse(build(drapeOnly,{drape:true,surface:false})).comparison.required_consumers,['drape']);
});

test('bounded complete mass can use explicit nominal scenario guards without inventing scenario ranges', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),assemblyMass:mass(),basis:'reported_bounds',
    objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:[{metric:'independent_surface_power_w',minimum:-10,maximum:10,basis:'nominal'},
      {metric:'max_absolute_edge_strain_pct',maximum:8,basis:'nominal'}],requiredConsumers:['drape','surface']};
  const context={drape:true,surface:true},result=JSON.parse(build(input,context));
  assert.deepEqual(result.comparison.constraints,input.constraints);
  assert.throws(()=>build({...input,constraints:[{metric:'independent_surface_power_w',maximum:10}]},context),/assembly mass only/);
  for(const metric of ['simulated_cloth_mass_kg','max_absolute_edge_strain_pct','independent_surface_power_w']) {
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]},context),/assembly mass only/);
  }
});

test('finite and mixed objectives compose with separately requested scenario consumers', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const [mode,metric,consumer] of [['finiteStock','used_stock_count','finite_stock'],
    ['cuttingJob','cutting_job_stock_area_m2','cutting_job']]) {
    const input=stockState(mode);
    input.objectives=[{metric,direction:'minimize'},
      {metric:'simulated_cloth_mass_kg',direction:'minimize'},{metric:'independent_surface_power_w',direction:'maximize'}];
    input.constraints=[{metric:'max_absolute_edge_strain_pct',maximum:8,basis:'nominal'}];
    input.requiredConsumers=[consumer,'drape','surface'];
    const before=JSON.stringify(input),raw=build(input,{drape:true,surface:true}),result=JSON.parse(raw);
    assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(input[mode]));
    assert.deepEqual(result.comparison.objectives,input.objectives);
    assert.deepEqual(result.comparison.constraints,input.constraints);
    assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
    for(const key of ['drape','surface','scenario'])assert.equal(Object.hasOwn(result,key),false);
    assert.throws(()=>build(input),/requested drape/);
    assert.throws(()=>build(input,{drape:true,surface:false}),/requested surface/);
  }
});

const linkedFitSource = () => ' {\r\n"schema":"fabric-fit/1","points":[{"id":"length","kind":"vertical","instance":"front","from":"a","to":"d","body":"stature","body_scale":0.1,"target_mm":[-0.1,0.0,0.1]}],"source":{"seed":18446744073709551615,"x":-0.0,"value":1.0,"label":"é 🧵"}}\r\n';
const linkedDesignInput = (changes = {}) => ({pieceIds:['front','lining'],axis:'y',atMm:50,multiplier:1,
  values:[0,5,10,15,20,25,30,35,40],maxDesigns:1,fitSpec:linkedFitSource(),...changes});

test('linked design authoring retains exact fit and material sources with independent alteration targets', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),target:{scope:'pieces',ids:['lining']},
    cutting:' {"copies":1,"gap_mm":2.0,"edge_margin_mm":-0.0,"end_margin_mm":0.0}\r\n',
    design:linkedDesignInput({pieceIds:['lining','front'],values:[-4,0,7.5],atMm:-10,multiplier:0.5,maxDesigns:2})};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.equal(result.schema,'ftx-design-study/1');
  for(const source of [input.design.fitSpec,input.cutting,input.candidates[0].selection])assert.ok(raw.includes(source));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"x":-0\.0/);assert.match(raw,/"value":1\.0/);
  assert.deepEqual(result.design_grid,{variables:[{key:'linked_adjustment',values_mm:[-4,0,7.5],sites:[
    {piece:'lining',axis:'y',at_mm:-10,multiplier:0.5},{piece:'front',axis:'y',at_mm:-10,multiplier:0.5}]}],
    max_designs:2,fit_screening:{max_candidates:3}});
  assert.deepEqual(result.design_fit,{fit_spec:JSON.parse(input.design.fitSpec)});
  assert.deepEqual(result.variants[0],{key:'baseline',changes:{schema:'ftx-material-bindings/1'}});
  assert.deepEqual(Object.keys(result.variants[1].changes.pieces),['lining']);
  for(const key of ['design','designs','design_adaptive','scenario'])assert.equal(Object.hasOwn(result,key),false);
});

test('linked design rejects invalid or nonfinite sites and domains without sorting or dropping values', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const check=design=>build({...state(),design});
  for(const design of [null,[],{}, {...linkedDesignInput(),unknown:true}])assert.throws(()=>check(design));
  for(const key of Object.keys(linkedDesignInput())) {
    const missing=linkedDesignInput();delete missing[key];assert.throws(()=>check(missing),/missing/);
  }
  for(const pieceIds of [[],['front','front'],['missing'],[''],[null],Array.from({length:17},(_,i)=>`p${i}`)]) {
    assert.throws(()=>check(linkedDesignInput({pieceIds})));
  }
  for(const axis of ['z','X',null,1])assert.throws(()=>check(linkedDesignInput({axis})),/axis/);
  for(const atMm of [NaN,Infinity,-Infinity,'50',null])assert.throws(()=>check(linkedDesignInput({atMm})),/finite/);
  for(const multiplier of [0,-0,NaN,Infinity,-Infinity,'1',null])assert.throws(()=>check(linkedDesignInput({multiplier})),/nonzero/);
  for(const values of [[],[0,0],[1,0],['1'],[NaN],[Infinity],[0,,1],Array.from({length:65},(_,i)=>i)]) {
    assert.throws(()=>check(linkedDesignInput({values})),/strictly increasing/);
  }
  for(const maxDesigns of [0,32,1.5,NaN,Infinity,'1',null])assert.throws(()=>check(linkedDesignInput({maxDesigns})),/integer/);
  assert.throws(()=>check(linkedDesignInput({values:[Number.MAX_VALUE],multiplier:2})),/overflows/);
  assert.throws(()=>check(linkedDesignInput({values:[Number.MIN_VALUE],multiplier:0.5})),/underflows/);
  const pieceIds=['front',...Array.from({length:15},(_,i)=>`piece-${i}`)];
  const result=JSON.parse(build({...state(),pieceIds,design:linkedDesignInput({pieceIds,
    values:Array.from({length:64},(_,i)=>i-32),multiplier:-0.5})}));
  assert.equal(result.design_grid.variables[0].sites.length,16);assert.equal(result.design_grid.fit_screening.max_candidates,64);
  assert.equal(result.design_grid.variables[0].sites[0].multiplier,-0.5);
  const long='p'.repeat(256*1024);
  assert.throws(()=>build({...state(),pieceIds:[long],design:linkedDesignInput({pieceIds:[long]})}),/grid exceeds 256 KiB/);
});

test('linked expansion counts both baselines and refuses excess work before reading frozen materials', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const candidates=Array.from({length:7},(_,i)=>({key:`choice-${i}`,selection:selection(`cloth-${i}`,'abcdef0'[i])}));
  const input={...state(),candidates,design:linkedDesignInput({values:[0,1,2,3],maxDesigns:3})};
  const result=JSON.parse(build(input));assert.equal(result.variants.length,8);assert.equal(result.design_grid.max_designs,3);
  assert.equal(result.design_grid.fit_screening.max_candidates,4,'All declared values are screened even when fewer passing designs reach consumers');
  let snapshotsRead=0;
  const unread=candidates.map(candidate=>({key:candidate.key,get selection(){snapshotsRead++;throw Error('Source must remain unread');}}));
  assert.throws(()=>build({...input,candidates:unread,design:{...input.design,maxDesigns:4}}),/32 expanded/);
  assert.equal(snapshotsRead,0);
  const bounded=JSON.parse(build({...input,design:linkedDesignInput({values:[0],maxDesigns:31})}));
  assert.equal(bounded.design_grid.max_designs,31,'Reserve the smaller domain without silently rewriting the declared allowance');
  const exact=JSON.parse(build({...state(),design:linkedDesignInput({values:Array.from({length:15},(_,i)=>i),maxDesigns:15})}));
  assert.equal(exact.design_grid.max_designs,15);
  assert.throws(()=>build({...state(),design:linkedDesignInput({values:Array.from({length:16},(_,i)=>i),maxDesigns:16})}),/32 expanded/);
});

test('linked fit contracts retain original JSON and enforce explicit point and UTF-8 source bounds', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const check=fitSpec=>build({...state(),design:linkedDesignInput({fitSpec})});
  for(const fitSpec of [{},null,'null','[]','{bad','{}','\ufeff'+linkedFitSource(),
    '{"schema":"other","points":[{}]}','{"schema":null,"points":[{}]}','{"points":[]}'])assert.throws(()=>check(fitSpec));
  const point=JSON.parse(linkedFitSource()).points[0];
  const points=Array.from({length:32},(_,i)=>({...point,id:`point-${i}`}));
  assert.equal(JSON.parse(check(JSON.stringify({points}))).design_fit.fit_spec.points.length,32);
  assert.throws(()=>check(JSON.stringify({points:[...points,{...point,id:'last'}]})),/1–32/);
  const prefix='{"schema":"fabric-fit/1","points":'+JSON.stringify([point])+',"source":"',suffix='"}';
  const room=256*1024-Buffer.byteLength(prefix+suffix),fit=prefix+'é'.repeat(Math.floor(room/2))+'x'.repeat(room%2)+suffix;
  assert.equal(Buffer.byteLength(fit),256*1024);assert.ok(check(fit).includes(fit));
  assert.throws(()=>check(' '+fit),/256 KiB.*UTF-8/);
});

test('linked grids compose with stock and scenario consumers while fixed-study requests remain unchanged', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const mode of ['cutting','finiteStock','cuttingJob']) {
    const input=mode==='cutting'?state():stockState(mode),context={drape:true,surface:true};
    input.objectives.push({metric:'independent_surface_power_w',direction:'maximize'});
    input.constraints=[{metric:'max_absolute_edge_strain_pct',maximum:8,basis:'nominal'}];
    input.requiredConsumers=['drape','surface'];const fixed=build(input,context),before=JSON.stringify(input);
    const design=linkedDesignInput(),raw=build({...input,design},context),result=JSON.parse(raw);
    assert.equal(JSON.stringify(input),before);assert.equal(result.schema,'ftx-design-study/1');
    assert.deepEqual(result.comparison,JSON.parse(fixed).comparison);
    if(typeof input[mode]==='string')assert.ok(raw.includes(input[mode]));
    delete result.design_grid;delete result.design_fit;result.schema='ftx-material-study/1';
    assert.deepEqual(result,JSON.parse(fixed));assert.equal(build(input,context),fixed);
    assert.throws(()=>build({...input,design}),/requested surface/);
    for(const key of ['design_grid','design_fit'])assert.equal(Object.hasOwn(JSON.parse(fixed),key),false);
  }
});

const multiDesignInput = (changes = {}) => ({variables:[
  {key:'ease',values:[-4,0,6],sites:[{piece:'front',axis:'x',at_mm:50,multiplier:0.5},
    {piece:'lining',axis:'x',at_mm:50,multiplier:-0.5}]},
  {key:'length',values:[-5,5],sites:[{piece:'front',axis:'y',at_mm:30,multiplier:1}]},
  {key:'local shape',values:[0,2],sites:[{op:'move',piece:'front',vertex:'d',dx_multiplier:0,dy_multiplier:1},
    {op:'move',piece:'lining',vertex:'d',dx_multiplier:1,dy_multiplier:-0.5}]}],
  maxDesigns:3,fitSpec:linkedFitSource(),...changes});

test('multiple linked variables preserve authored order, mixed site semantics and exact independent sources', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),target:{scope:'pieces',ids:['lining']},design:multiDesignInput()};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.equal(result.schema,'ftx-design-study/1');
  assert.deepEqual(result.design_grid,{variables:input.design.variables.map(v=>({key:v.key,values_mm:v.values,sites:v.sites})),
    max_designs:3,fit_screening:{max_candidates:12}});
  for(const source of [input.design.fitSpec,input.candidates[0].selection])assert.ok(raw.includes(source));
  assert.deepEqual(Object.keys(result.variants[1].changes.pieces),['lining']);
  assert.equal(Object.hasOwn(result,'design_adaptive'),false);
  const prefix=JSON.parse(build({...input,design:{...input.design,maxCandidates:4}}));
  assert.equal(prefix.design_grid.fit_screening.max_candidates,4);
  assert.deepEqual(prefix.design_grid.variables,result.design_grid.variables);
});

test('multi-variable screening uses the whole Cartesian domain or an explicitly retained bounded prefix', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const variable=(key,count)=>({key,values:Array.from({length:count},(_,i)=>i),
    sites:[{piece:'front',axis:'x',at_mm:50,multiplier:1}]});
  const design=multiDesignInput({variables:[variable('width',16),variable('length',16)]});
  assert.equal(JSON.parse(build({...state(),design})).design_grid.fit_screening.max_candidates,256);
  const large={...design,variables:[variable('width',64),variable('length',64),variable('shape',64)]};
  assert.throws(()=>build({...state(),design:large}),/declare maxCandidates/);
  for(const maxCandidates of [1,64,256]) {
    const result=JSON.parse(build({...state(),design:{...large,maxCandidates}}));
    assert.equal(result.design_grid.fit_screening.max_candidates,maxCandidates);
    assert.equal(result.design_grid.variables.length,3);assert.equal(result.design_grid.variables[2].values_mm.length,64);
  }
  for(const maxCandidates of [0,-1,257,1.5,NaN,Infinity,'2',null,undefined]) {
    assert.throws(()=>build({...state(),design:{...design,maxCandidates}}),/integer from 1 to 256/);
  }
  const small={...design,variables:[variable('width',1)],maxCandidates:256,maxDesigns:31};
  const result=JSON.parse(build({...state(),design:small}));
  assert.equal(result.design_grid.fit_screening.max_candidates,256,'An explicit allowance is not silently reduced to domain size');
  assert.equal(result.design_grid.max_designs,31);
});

test('multi-variable admission refuses ambiguous shapes, duplicate keys and invalid source sites', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const good=multiDesignInput(),variable=good.variables[0],check=design=>build({...state(),design});
  for(const variables of [[],null,[null],[,variable],Array(4).fill(variable),[variable,{...variable}]]) {
    assert.throws(()=>check({...good,variables}));
  }
  for(const key of ['',null,1])assert.throws(()=>check({...good,variables:[{...variable,key}]}),/nonempty/);
  for(const key of ['key','values','sites']) {
    const v={...variable};delete v[key];assert.throws(()=>check({...good,variables:[v]}),/missing/);
  }
  for(const values of [[],[0,0],[1,0],[0,,2],['1'],[NaN],[Infinity],Array.from({length:65},(_,i)=>i)]) {
    assert.throws(()=>check({...good,variables:[{...variable,values}]}),/strictly increasing/);
  }
  assert.throws(()=>check({...good,pieceIds:['front']}),/unsupported/);
  const move=good.variables[2].sites[0];
  for(const sites of [[],null,[null],[,move],[{...move,piece:'missing'}],[{...move,vertex:''}],
    [{...move,op:'slash_spread'}],[{...move,dx_multiplier:0,dy_multiplier:-0}],
    [{...move,dx_multiplier:NaN}],[{...move,dy_multiplier:'1'}],
    [{...move,multiplier:1}],[{...variable.sites[0],vertex:'d'}]]) {
    assert.throws(()=>check({...good,variables:[{...variable,sites}]}));
  }
  const sixteen=Array.from({length:16},(_,i)=>({...move,vertex:'v'+i}));
  assert.equal(JSON.parse(check({...good,variables:[{...variable,sites:sixteen}]})).design_grid.variables[0].sites.length,16);
  assert.throws(()=>check({...good,variables:[{...variable,sites:sixteen},good.variables[1]]}),/16 ordered/);
});

test('all linked site displacements are admitted even outside the chosen screening prefix', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const check=(values,site)=>build({...state(),design:multiDesignInput({variables:[{key:'shape',values,sites:[site]}],
    maxCandidates:1,maxDesigns:1})});
  const slash={piece:'front',axis:'x',at_mm:50,multiplier:2};
  const move={op:'move',piece:'front',vertex:'d',dx_multiplier:0,dy_multiplier:2};
  for(const site of [slash,move])assert.throws(()=>check([0,Number.MAX_VALUE],site),/overflows/);
  assert.throws(()=>check([0,Number.MIN_VALUE],{...move,dy_multiplier:0.5}),/underflows/);
  const result=JSON.parse(check([0,Number.MIN_VALUE],{...move,dy_multiplier:1}));
  assert.equal(result.design_grid.variables[0].sites[0].dx_multiplier,0,'An explicitly zero vector component remains valid');
  assert.equal(result.design_grid.variables[0].values_mm[1],Number.MIN_VALUE);
});

test('multi-variable row reservations include both baselines before material snapshots are inspected', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  let reads=0;
  const candidates=Array.from({length:7},(_,i)=>({key:'choice-'+i,get selection(){reads++;throw Error('Unread snapshot');}}));
  const design=multiDesignInput({maxDesigns:4,maxCandidates:1});
  assert.throws(()=>build({...state(),candidates,design}),/32 expanded/);assert.equal(reads,0);
  const real=candidates.map((c,i)=>({key:c.key,selection:selection('cloth-'+i,'abcdef0'[i])}));
  const raw=build({...state(),candidates:real,design:{...design,maxDesigns:3}});
  assert.equal(JSON.parse(raw).variants.length,8);
  const long='v'.repeat(256*1024),oversize=multiDesignInput({variables:[
    {key:'shape',values:[0],sites:[{op:'move',piece:'front',vertex:long,dx_multiplier:1,dy_multiplier:0}]}]});
  assert.throws(()=>build({...state(),design:oversize}),/grid exceeds 256 KiB/);
});

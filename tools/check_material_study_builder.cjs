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

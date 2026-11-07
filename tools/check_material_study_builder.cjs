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


test('saved configurations retain heterogeneous assignments and exact interface evidence beside a single selection', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const shell=selection('shell','b'),lining=selection('lining','c',true);
  const pairs=' [{"materials":["'+ 'b'.repeat(64)+'","'+'c'.repeat(64)+'"],"source":{"seed":18446744073709551615,"x":-0.0,"value":1.0,"label":"é 🧵"}}]\r\n';
  const changes=' {"schema":"ftx-material-bindings/1","default":'+shell+',"pieces":{"lining":'+lining+'},"interfaces":'+pairs+'}\r\n';
  const input={...state(),target:{scope:'pieces',ids:['front']},
    candidates:[...state().candidates,{key:'shell-and-lining',changes}]};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.deepEqual(result.variants[0],{key:'baseline',changes:{schema:'ftx-material-bindings/1'}});
  assert.deepEqual(Object.keys(result.variants[1].changes.pieces),['front']);
  assert.deepEqual(result.variants[2],{key:'shell-and-lining',changes:JSON.parse(changes)});
  for(const source of [changes,shell,lining,pairs,input.candidates[0].selection])assert.ok(raw.includes(source));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"x":-0\.0/);assert.match(raw,/"value":1\.0/);
});

test('saved configurations own their targets and preserve omitted, null and empty interface replacements', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const base='{"schema":"ftx-material-bindings/1","default":null,"pieces":{"__proto__":'+selection()+'}';
  const candidates=[
    {key:'preserve',changes:base+'}'},
    {key:'explicit-null',changes:base+',"interfaces":null}'},
    {key:'clear',changes:base+',"interfaces":[]}'},
    {key:'interfaces-only',changes:'{"schema":"ftx-material-bindings/1","interfaces":[]}'},
  ];
  const input={...state(),pieceIds:['__proto__','lining'],candidates};delete input.target;
  const raw=build(input),variants=JSON.parse(raw).variants;
  assert.equal(Object.hasOwn(variants[1].changes,'interfaces'),false);
  assert.equal(variants[2].changes.interfaces,null);assert.deepEqual(variants[3].changes.interfaces,[]);
  assert.deepEqual(variants[4].changes,{schema:'ftx-material-bindings/1',interfaces:[]});
  for(const candidate of candidates)assert.ok(raw.includes(candidate.changes));
  assert.equal(build({...input,target:{scope:'pieces',ids:[]}}),raw,'An unused shared target cannot block saved configurations');
  assert.throws(()=>build({...input,candidates:[...candidates,...state().candidates]}),/Material target/);
  assert.throws(()=>build({...input,target:{scope:'pieces',ids:[]},candidates:[...candidates,...state().candidates]}),/Selected source pieces/);
});

test('saved configurations refuse ambiguous entries, stale piece names, unfrozen sources and incomplete replacements', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const valid='{"schema":"ftx-material-bindings/1","default":'+selection()+'}';
  const check=changes=>build({...state(),candidates:[{key:'configuration',changes}]});
  for(const changes of [null,{},'null','[]','{bad','{}','{"schema":"other","interfaces":[]}',
    '{"schema":"ftx-material-bindings/1"}','{"schema":"ftx-material-bindings/1","default":null,"pieces":{},"interfaces":null}',
    '{"schema":"ftx-material-bindings/1","default":{"id":"cloth"}}',
    valid.replace('a'.repeat(64),'stale'),valid.replace('"allow_estimates":false','"allow_estimates":"false"'),
    '{"schema":"ftx-material-bindings/1","pieces":null}',
    '{"schema":"ftx-material-bindings/1","pieces":[]}',
    '{"schema":"ftx-material-bindings/1","pieces":{"front":null}}',
    '{"schema":"ftx-material-bindings/1","pieces":{"missing":'+selection()+'}}',
    '{"schema":"ftx-material-bindings/1","interfaces":{}}',
    JSON.stringify({schema:'ftx-material-bindings/1',interfaces:Array(2017).fill({})}),
    '{"schema":"ftx-material-bindings/1","interfaces":[],"elastic_casings":{}}'])assert.throws(()=>check(changes));
  for(const candidate of [{key:'neither'},{key:'both',selection:selection(),changes:valid}]) {
    assert.throws(()=>build({...state(),candidates:[candidate]}),/one frozen selection or one saved configuration/);
  }
  assert.throws(()=>build({...state(),candidates:[{key:'one',changes:valid},{key:'two',changes:valid}]}),/already included/);
  assert.throws(()=>build({...state(),candidates:[{key:'same',selection:selection()},{key:'same',changes:valid}]}),/names.*unique/);
  const candidates=Array.from({length:7},(_,i)=>({key:'configuration-'+i,
    changes:'{"schema":"ftx-material-bindings/1","default":'+selection('cloth-'+i,String(i))+'}'}));
  assert.equal(JSON.parse(build({...state(),candidates})).variants.length,8);
  assert.throws(()=>build({...state(),candidates:[...candidates,{key:'eighth',changes:valid}]}),/1–7/);
});

test('saved configurations remain whole variants through linked grids and named-case adaptive nomination', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const changes=' {"schema":"ftx-material-bindings/1","pieces":{"front":'+selection('shell','b')+
    ',"lining":'+selection('lining','c')+'},"interfaces":[]}\r\n';
  for(const design of [linkedDesignInput({values:[0,10]}),namedDesign({materialKey:'shell-and-lining'})]) {
    const input={...state(),candidates:[{key:'shell-and-lining',changes}],design};delete input.target;
    const context=namedContext(undefined,12),before=JSON.stringify(input),raw=build(input,context),result=JSON.parse(raw);
    assert.equal(result.schema,'ftx-design-study/1');assert.ok(raw.includes(changes));assert.ok(raw.includes(design.fitSpec));
    assert.deepEqual(result.variants[1],{key:'shell-and-lining',changes:JSON.parse(changes)});
    assert.equal(JSON.stringify(input),before);
    if(design.mode==='adaptive') {
      assert.equal(result.design_adaptive.material_key,'shell-and-lining');
      assert.equal(result.design_adaptive.case_key,'cold');
      assert.throws(()=>build(input,namedContext(undefined,11)),/12 reserved case evaluations/);
    }
  }
});

test('reported bounds require the explicit complete assembly inventory and preserve raw extra evidence', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input=state();delete input.cutting;
  input.assemblyMass=mass();input.basis='reported_bounds';input.objectives=[{metric:'assembly_mass_kg',direction:'minimize'}];
  const result=JSON.parse(build(input));assert.equal(Object.hasOwn(result,'cutting'),false);assert.deepEqual(result.assembly_mass,mass());
  assert.throws(()=>build({...input,assemblyMass:mass(false)}),/complete inventory/);
  const undeclared=mass();delete undeclared.inventory_complete;
  assert.throws(()=>build({...input,assemblyMass:undeclared}),/missing/);
  assert.throws(()=>build({...state(),basis:'reported_bounds'}),/mass, steady heat, vapor potential and final body-water totals only/);
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
  assert.throws(()=>build({...input,constraints:[{metric:'sewing_modeled_production_seconds',maximum:3600}]}),/mass, steady heat, vapor potential and final body-water totals only/);
  assert.throws(()=>build({...input,constraints:[{metric:'selected_observed_order_cost',maximum:50,basis:'reported_bounds'}]}),/mass, steady heat, vapor potential and final body-water totals only/);
  assert.throws(()=>build({...input,objectives:[{metric:'cutting_modeled_path_seconds',direction:'minimize'}]}),/mass, steady heat, vapor potential and final body-water totals only/);
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
    assert.throws(()=>build({...input,constraints:[{metric,maximum:20}]}),/mass, steady heat, vapor potential and final body-water totals only/);
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]}),/mass, steady heat, vapor potential and final body-water totals only/);
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
const motionPolicy = (changes={}) => ({schema:'ftx-drape-motion/1',keyframes:[
  {time_s:0.1,arm_abduction_deg:50,leg_pose:{posture:'seated',fraction:0.5}},
  {time_s:0.2,arm_abduction_deg:35}],max_step_s:0.01,max_trials:32,max_pairs:1000,...changes});
const strainPolicy = (changes={}) => ({schema:'ftx-material-strain/1',max_triangle_samples:10000,...changes});
const contactPolicy = (changes={}) => ({schema:'ftx-contact-loads/1',max_vertex_samples:100000,max_pair_evaluations:1000000,...changes});
const coveragePolicy = (changes={}) => ({body_triangle_ids:[2],instance_layers:{band:'shell'},
  settings:{max_distance_m:0.2,min_normal_cosine:0.1,separation_tolerance_m:0.00001,cells_per_layer:1,interfaces:[]},
  conditions:{boundaries:[{inside_c:30,outside_c:10,inside_resistance_m2_k_w:0.1,outside_resistance_m2_k_w:0.2}]},...changes});
const vaporBoundary = () => ({inside_vapor_pressure_pa:2000,outside_vapor_pressure_pa:1000,
  inside_film_resistance_m2_pa_w:1,outside_film_resistance_m2_pa_w:2,latent_heat_j_kg:2500000});
const vaporSurfacePolicy = (changes={}) => ({schema:'ftx-vapor-surface-conditions/1',boundaries:[vaporBoundary()],...changes});
const vaporCoveragePolicy = (changes={}) => {
  const value=coveragePolicy();delete value.settings.cells_per_layer;
  return {...value,conditions:{boundaries:[vaporBoundary()]},...changes};
};
const vaporHistoryPolicy = () => ({schema:'ftx-motion-vapor-coverage/1',integration:'composite_trapezoid',max_geometry_operations:1000000});
const motionState = () => ({keyframes:[{time_s:0.25,arm_abduction_deg:30}],max_step_s:0.125,
  max_trials:128,max_pairs:100000,max_sample_vertices:500000});
const dryMotionHeat = () => ({schema:'ftx-motion-surface-heat/1',initial_temperature_c:18,cells_per_layer:4,
  boundaries:[{inside_c:35,outside_c:-5,inside_resistance_m2_k_w:0.1,outside_resistance_m2_k_w:0}],
  max_step_s:0.01,max_cell_steps:100000});
const exteriorPolicy = (changes={}) => ({air_c:-5,convection_w_m2_k:8,surface_temperature_range_c:[-40,80],
  convection_source:{reference:'test:guided-exterior',accessed_on:'2026-11-05',note:'Synthetic prescribed boundary; no measured garment claim.'},
  convection_evidence:'estimate',convection_conditions:'Declared test geometry, air state and coefficient; no wind correlation.',
  allow_estimates:true,...changes});

test('qualified exterior authoring copies source evidence and optical choices without inventing defaults', async () => {
  const {buildMaterialExteriorExchange:build}=await modulePromise;
  for(const radiation of [undefined,null,{ambient_c:-15,emissivity_property:'infrared_emissivity',exposed_face:'public',allow_opaque_gray_approximation:true},
    {ambient_c:25,automatic_material_face:true,allow_opaque_gray_approximation:true},
    {ambient_c:25,automatic_material_face:true,emissivity_property:'',exposed_face:'',allow_opaque_gray_approximation:true}]) {
    const input=exteriorPolicy();if(radiation!==undefined)input.radiation=radiation;
    const before=JSON.stringify(input),result=build(input);
    assert.deepEqual(result,input);assert.equal(JSON.stringify(input),before);
    assert.notEqual(result,input);assert.notEqual(result.convection_source,input.convection_source);
    assert.notEqual(result.surface_temperature_range_c,input.surface_temperature_range_c);
    if(radiation)assert.notEqual(result.radiation,radiation);
    result.convection_source.note='Changed draft';result.surface_temperature_range_c[0]=-25;
    if(radiation)result.radiation.ambient_c=30;
    assert.equal(JSON.stringify(input),before,'Saved source, domain and radiation choices must not share mutable draft objects');
  }
  for(const convection_evidence of ['measured','published']) {
    const input=exteriorPolicy({convection_evidence});delete input.allow_estimates;
    assert.deepEqual(build(input),input);assert.equal(Object.hasOwn(build(input),'allow_estimates'),false);
  }
  for(const accessed_on of ['0001-01-01','2024-02-29','2000-02-29','9999-12-31']) {
    const input=exteriorPolicy();input.convection_source.accessed_on=accessed_on;
    assert.equal(build(input).convection_source.accessed_on,accessed_on);
  }
  const narrow=exteriorPolicy({air_c:100,convection_w_m2_k:0,surface_temperature_range_c:[12,12]});
  assert.deepEqual(build(narrow),narrow,'Zero convection is a nominal input; actual surface-domain and source-bound admission remain native');
  assert.equal(build(exteriorPolicy({air_c:-273.15,surface_temperature_range_c:[-273.15,0]})).air_c,-273.15);
});

test('qualified convection refuses incomplete sources, estimates without consent and invalid physical or calendar values', async () => {
  const {buildMaterialExteriorExchange:build}=await modulePromise,valid=exteriorPolicy();
  for(const value of [null,undefined,[],{},'{}'])assert.throws(()=>build(value));
  for(const key of Object.keys(valid).filter(key=>key!=='allow_estimates')) {
    const missing={...valid};delete missing[key];assert.throws(()=>build(missing),key);
  }
  for(const key of ['air_c','convection_w_m2_k'])for(const value of [null,undefined,'1',NaN,Infinity,-Infinity]) {
    assert.throws(()=>build({...valid,[key]:value}),key);
  }
  for(const extra of [{air_c:-273.151},{convection_w_m2_k:-0.1},{convection_evidence:'inferred'},
    {allow_estimates:false},{allow_estimates:'true'},{allow_estimates:null},{allow_estimates:undefined},
    {schema:'ftx-exterior/1'},{emissivity:0.9},{convection_conditions:' '},{convection_conditions:'é'.repeat(2049)}]) {
    assert.throws(()=>build({...valid,...extra}));
  }
  const missingConsent={...valid};delete missingConsent.allow_estimates;
  assert.throws(()=>build(missingConsent),/estimate consent/);
  assert.throws(()=>build(Object.assign(Object.create({allow_estimates:true}),missingConsent)),/estimate consent/);
  for(const range of [null,[],[0],[0,1,2],[20,10],[-274,20],['0',20],[0,NaN],[0,Infinity]]) {
    assert.throws(()=>build({...valid,surface_temperature_range_c:range}));
  }
  for(const key of ['reference','accessed_on','note']) {
    const source={...valid.convection_source};delete source[key];assert.throws(()=>build({...valid,convection_source:source}));
    for(const value of ['',null,1,'é'.repeat(2049)])assert.throws(()=>build({...valid,convection_source:{...valid.convection_source,[key]:value}}));
  }
  for(const accessed_on of ['2026-2-03','2026-02-30','2026-02-29','1900-02-29','0000-01-01','2026-13-01','2026-01-00','2026-04-31','abcdefghij']) {
    assert.throws(()=>build({...valid,convection_source:{...valid.convection_source,accessed_on}}),/calendar date/);
  }
  assert.throws(()=>build({...valid,convection_source:{...valid.convection_source,doi:'extra'}}),/unsupported/);
});

test('radiation requires qualified optical names or automatic material faces and explicit enclosure consent', async () => {
  const {buildMaterialExteriorExchange:build}=await modulePromise;
  const radiation={ambient_c:10,emissivity_property:'emissivity',exposed_face:'public',allow_opaque_gray_approximation:true};
  for(const value of [undefined,[],{},'{}'])assert.throws(()=>build(exteriorPolicy({radiation:value})));
  for(const key of Object.keys(radiation)) {
    const missing={...radiation};delete missing[key];assert.throws(()=>build(exteriorPolicy({radiation:missing})),key);
  }
  for(const extra of [{ambient_c:-274},{ambient_c:NaN},{ambient_c:'10'},{emissivity_property:''},{exposed_face:' '},
    {emissivity_property:0.9},{exposed_face:'é'.repeat(2049)},{allow_opaque_gray_approximation:false},
    {allow_opaque_gray_approximation:'true'},{automatic_material_face:true},{automatic_material_face:'false'},
    {emissivity:0.9},{view_factor:1}])assert.throws(()=>build(exteriorPolicy({radiation:{...radiation,...extra}})));
  const explicit=exteriorPolicy({radiation:{...radiation,automatic_material_face:false}});
  assert.deepEqual(build(explicit),explicit);
  for(const key of ['emissivity_property','exposed_face'])for(const value of [null,' ',1]) {
    assert.throws(()=>build(exteriorPolicy({radiation:{ambient_c:10,automatic_material_face:true,allow_opaque_gray_approximation:true,[key]:value}})));
  }
});

test('uniform sheet heat and vapor builders author independent explicit boundaries without material or model defaults', async () => {
  const {buildMaterialSurfaceConditions:heat,buildMaterialVaporSurfaceConditions:vapor,buildMaterialScenarioRequest:scenario}=await modulePromise;
  const boundary={inside_c:-5,outside_c:30,inside_resistance_m2_k_w:0,outside_resistance_m2_k_w:0.2};
  const vaporInput=vaporBoundary(),before=JSON.stringify({boundary,vaporInput});
  const surface=heat(boundary),vaporSurface=vapor(vaporInput);
  assert.deepEqual(JSON.parse(surface),{schema:'ftx-steady-surface-conditions/1',boundaries:[boundary]});
  assert.deepEqual(JSON.parse(vaporSurface),{schema:'ftx-vapor-surface-conditions/1',boundaries:[vaporInput]});
  assert.equal(JSON.stringify({boundary,vaporInput}),before);
  for(const raw of [surface,vaporSurface]) for(const key of ['model_sha256','allow_mixed_protocols','required_method','required_conditions','exterior_exchange']) {
    assert.equal(Object.hasOwn(JSON.parse(raw),key),false,key+' stays native-defaulted or on the original-file route');
  }
  const raw=scenario({...scenarioState(),surface,vaporSurface}),both=JSON.parse(raw);
  assert.ok(raw.includes(surface));assert.ok(raw.includes(vaporSurface));assert.deepEqual(both.surface.boundaries,[boundary]);
  assert.deepEqual(both.vapor_surface.boundaries,[vaporInput]);assert.equal(Object.hasOwn(both,'motion'),false);
  assert.equal(Object.hasOwn(JSON.parse(scenario({...scenarioState(),vaporSurface})),'surface'),false,'Vapor needs no dry heat request');
  for(const inside_c of [-273.15,30,35]) assert.equal(JSON.parse(heat({...boundary,inside_c})).boundaries[0].inside_c,inside_c);
  for(const pressure of [0,1000]) {
    assert.equal(JSON.parse(vapor({...vaporInput,inside_vapor_pressure_pa:pressure,outside_vapor_pressure_pa:pressure}))
      .boundaries[0].inside_vapor_pressure_pa,pressure,'Equal reservoirs are an explicit valid zero-potential case');
  }
});

test('uniform sheet boundary builders refuse missing values, coercion and unsupported or nonphysical conditions', async () => {
  const {buildMaterialSurfaceConditions:heat,buildMaterialVaporSurfaceConditions:vapor}=await modulePromise;
  const heatInput={inside_c:30,outside_c:10,inside_resistance_m2_k_w:0.1,outside_resistance_m2_k_w:0.2},vaporInput=vaporBoundary();
  for(const [build,input] of [[heat,heatInput],[vapor,vaporInput]]) {
    for(const value of [null,[],{},true,'{}'])assert.throws(()=>build(value));
    for(const key of Object.keys(input)) {
      const missing={...input};delete missing[key];assert.throws(()=>build(missing),key);
      for(const value of [NaN,Infinity,-Infinity,null,undefined,'1',true])assert.throws(()=>build({...input,[key]:value}),key);
    }
    for(const extra of [{model_sha256:'a'.repeat(64)},{allow_mixed_protocols:true},{required_method:'test'},{exterior_exchange:[]}]) {
      assert.throws(()=>build({...input,...extra}),/unsupported/,'Advanced policies retain their original source file');
    }
  }
  for(const key of ['inside_c','outside_c'])assert.throws(()=>heat({...heatInput,[key]:-273.15001}));
  for(const key of ['inside_resistance_m2_k_w','outside_resistance_m2_k_w'])assert.throws(()=>heat({...heatInput,[key]:-0.001}));
  for(const key of ['inside_vapor_pressure_pa','outside_vapor_pressure_pa'])assert.throws(()=>vapor({...vaporInput,[key]:-1}));
  assert.throws(()=>vapor({...vaporInput,inside_vapor_pressure_pa:999}),/outside pressure/);
  for(const key of ['inside_film_resistance_m2_pa_w','outside_film_resistance_m2_pa_w','latent_heat_j_kg']) {
    for(const value of [0,-1])assert.throws(()=>vapor({...vaporInput,[key]:value}),/positive/);
  }
});

test('guided sheet exteriors replace only the declared outside film and preserve source-qualified scenario inputs', async () => {
  const {buildMaterialSurfaceConditions:build,buildMaterialScenarioRequest:scenario}=await modulePromise;
  const boundary=dryMotionHeat().boundaries[0],exterior=exteriorPolicy(),before=JSON.stringify({boundary,exterior});
  const raw=build(boundary,exterior),result=JSON.parse(raw);
  assert.deepEqual(result,{schema:'ftx-steady-surface-conditions/1',boundaries:[boundary],exterior_exchange:[exterior]});
  assert.equal(JSON.stringify({boundary,exterior}),before);assert.ok(scenario({...scenarioState(),surface:raw}).includes(raw));
  assert.equal(build(boundary),JSON.stringify({schema:'ftx-steady-surface-conditions/1',boundaries:[boundary]}));
  assert.equal(build(boundary,null),build(boundary));
  for(const changes of [{outside_c:exterior.air_c+1},{outside_resistance_m2_k_w:0.2}]) {
    assert.throws(()=>build({...boundary,...changes},exterior),/zero outside film resistance and outside_c equal to air_c/);
  }
  for(const convection_w_m2_k of [0,8])assert.equal(JSON.parse(build(boundary,{...exterior,convection_w_m2_k})).exterior_exchange[0].convection_w_m2_k,convection_w_m2_k);
  const radiation={ambient_c:-10,automatic_material_face:true,allow_opaque_gray_approximation:true};
  assert.deepEqual(JSON.parse(build(boundary,{...exterior,radiation})).exterior_exchange[0].radiation,radiation);
});

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

test('guided motion preserves ordered absolute endpoints and original scenario sources', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:build}=await modulePromise;
  const request={...motionState(),keyframes:[{time_s:0.25,arm_abduction_deg:25,leg_pose:{posture:'seated',fraction:0.5}},
    {time_s:0.5,arm_abduction_deg:35,leg_pose:{posture:'squat',fraction:0.25}},
    {time_s:0.75,arm_abduction_deg:0}]};
  const before=JSON.stringify(request),rawMotion=motion(request);
  assert.deepEqual(JSON.parse(rawMotion),{schema:'ftx-drape-motion/1',...request});
  assert.equal(Object.hasOwn(JSON.parse(rawMotion).keyframes[2],'leg_pose'),false,'Returning to standing omits leg pose, rather than carrying the prior endpoint forward');
  const fit=' {"schema":"fabric-fit/1","source":{"seed":18446744073709551615,"x":-0.0}}\r\n';
  const raw=build({...scenarioState(),motion:request,maxMotionTrials:4096,fitSpec:fit,surface:heatSource()}),result=JSON.parse(raw);
  assert.equal(JSON.stringify(request),before);assert.ok(raw.includes(rawMotion));assert.ok(raw.includes(fit));assert.ok(raw.includes(heatSource()));
  assert.deepEqual(result.motion,JSON.parse(rawMotion));assert.equal(result.max_motion_trials,4096);
  assert.equal(Object.hasOwn(result,'maxMotionTrials'),false);
  const omitted=motionState();delete omitted.max_sample_vertices;
  assert.equal(Object.hasOwn(JSON.parse(motion(omitted)),'max_sample_vertices'),false,'Omission retains the native 500000 default');
});

test('guided motion rejects ambiguous endpoints, invalid poses and unordered physical times', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise;
  const frame=motionState().keyframes[0],withFrames=keyframes=>({...motionState(),keyframes});
  for(const keyframes of [null,{},[],[null],[{}],Array(17).fill(frame)]) assert.throws(()=>build(withFrames(keyframes)));
  for(const time_s of [0,-1,5e-10,60.0001,NaN,Infinity,'0.25']) {
    assert.throws(()=>build(withFrames([{...frame,time_s}])));
  }
  for(const time_s of [0.25,0.1,0.25+5e-10]) assert.throws(()=>build(withFrames([frame,{...frame,time_s}])));
  for(const arm_abduction_deg of [-0.001,180.001,NaN,Infinity,'30']) {
    assert.throws(()=>build(withFrames([{...frame,arm_abduction_deg}])));
  }
  for(const leg_pose of [{},{posture:'standing',fraction:0},{posture:'seated'},
    ...[-0.01,1.01,NaN,Infinity,'0.5'].map(fraction=>({posture:'squat',fraction})),
    {posture:'seated',fraction:0.5,extra:true}]) assert.throws(()=>build(withFrames([{...frame,leg_pose}])));
  assert.throws(()=>build(withFrames([{...frame,extra:true}])));
  for(const extra of [{schema:'ftx-drape-motion/1'},{transient_surface:{}},{unknown:true}]) {
    assert.throws(()=>build({...motionState(),...extra}),/unsupported/,'Advanced source policies use their original JSON route');
  }
  const endpoints=Array.from({length:16},(_,i)=>({time_s:(i+1)/16,arm_abduction_deg:i*12}));
  assert.equal(JSON.parse(build({...motionState(),keyframes:endpoints})).keyframes.length,16);
});

test('guided dry heat authors one explicit film per endpoint and enables same-case transient decisions', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix,
    buildMaterialStudyRequest:study}=await modulePromise;
  const heat=dryMotionHeat();heat.boundaries.push({...heat.boundaries[0],inside_c:-5,outside_c:35});
  const input={...motionState(),keyframes:[{time_s:0.25,arm_abduction_deg:30},{time_s:0.5,arm_abduction_deg:30}],transient_surface:heat};
  const before=JSON.stringify(input),raw=motion(input),request=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(request.transient_surface,heat);
  assert.equal(request.transient_surface.boundaries[0].outside_resistance_m2_k_w,0,'A prescribed face temperature remains explicit zero');
  assert.equal(Object.hasOwn(JSON.parse(motion(motionState())),'transient_surface'),false,'Ordinary dry motion stays unchanged');
  const scenario=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  assert.ok(scenario.includes(raw));assert.equal(Object.hasOwn(JSON.parse(scenario),'surface'),false,'No steady heat is invented');
  const comparison={basis:'nominal',objectives:[{metric:'transient_surface_net_boundary_energy_j',direction:'minimize'}],
    constraints:[{metric:'transient_surface_final_min_temperature_c',minimum:-15}],required_consumers:['transient_surface']};
  const named=matrix({maxEvaluations:2,cases:[{key:'warm-cold',scenario,comparison:JSON.stringify(comparison)}]});
  assert.deepEqual(JSON.parse(named).cases[0].scenario.motion.transient_surface,heat);
  const result=JSON.parse(study({...state(),objectives:comparison.objectives,constraints:comparison.constraints,
    requiredConsumers:comparison.required_consumers},{drape:true,surface:false,motion:true,transientSurface:true}));
  assert.deepEqual(result.comparison.objectives,comparison.objectives);
  assert.throws(()=>study({...state(),objectives:comparison.objectives},{drape:true,surface:false,motion:true}),/transient/i);
  heat.max_cell_steps=10000000;
  const expensive=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  assert.equal(JSON.parse(matrix({maxEvaluations:3,cases:['one','two','three'].map(key=>({key,scenario:expensive}))})).cases.length,3,
    'Native optional consumer reservations may refuse individual histories without blocking independent study outcomes');
});

test('guided wet-source composition retains original direct and selected policies beside authored heat', async () => {
  const {buildMaterialMotionRequest:build,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const water=' {\r\n"schema":"ftx-textile-surface-water/5","bindings":{"front.R":{"material_sha256":"'+'a'.repeat(64)+
    '","initial_liquid_kg_m2":1.25000e-2}},"enthalpy_reference_c":-0.0,"latent_heat_j_kg":18446744073709551615,'+
    '"source":{"reference":"test:café 🧵"},"vapor_reservoir":{"initial_vapor_mass_kg":1.0}}\r\n';
  const mechanics=' {"schema":"ftx-coupled-liquid-mechanics/6","material_report_sha256":"'+'b'.repeat(64)+'"}\r\n';
  const alternatives=' [\r\n'+water+','+water.replace('a'.repeat(64),'c'.repeat(64))+']\r\n';
  const paired=' [ {"surface_water":'+water+',"mechanical_coupling":'+mechanics+'} ]\r\n';
  const heat={...dryMotionHeat(),initial_instance_temperatures_c:{'front.R':5},
    exterior_exchange:[exteriorPolicy()],plastic_dissipation_heat:'uniform_reference_thickness'};
  const input={...motionState(),transient_surface:heat},before=JSON.stringify(input);
  const choices=[...[1,2,3,4,5].map(version=>({surface_water:water.replace('/5','/'+version)})),
    {surface_water:water,mechanical_coupling:mechanics},{surface_water_alternatives:alternatives},{coupled_water_alternatives:paired}];
  for(const liquid of choices) {
    const sourceBefore=JSON.stringify(liquid),raw=build(input,liquid),value=JSON.parse(raw);
    for(const [key,original] of Object.entries(liquid)) {
      assert.ok(raw.includes(original));assert.deepEqual(value.transient_surface[key],JSON.parse(original));
    }
    for(const [key,value] of Object.entries(heat))assert.deepEqual(JSON.parse(raw).transient_surface[key],value);
    assert.equal(Object.hasOwn(value,'motionLiquid'),false);assert.equal(JSON.stringify(input),before);assert.equal(JSON.stringify(liquid),sourceBefore);
    const saved=single({...scenarioState(),motion:input,motionLiquid:liquid,maxMotionTrials:1024,maxVertexFrames:16000000});
    assert.ok(saved.includes(raw));assert.match(saved,/18446744073709551615/);assert.match(saved,/1\.25000e-2/);assert.match(saved,/"enthalpy_reference_c":-0\.0/);
    const named=matrix({maxEvaluations:2,cases:[{key:'wet',scenario:saved}]});assert.ok(named.includes(saved));
    heat.initial_temperature_c=22;
    assert.equal(JSON.parse(saved).motion.transient_surface.initial_temperature_c,18);
    heat.initial_temperature_c=18;
  }
});

test('guided wet-source composition requires one complete source mode and never overwrites original motion', async () => {
  const {buildMaterialMotionRequest:build,buildMaterialScenarioRequest:single}=await modulePromise;
  const motion={...motionState(),transient_surface:dryMotionHeat()},direct={surface_water:'{}'};
  for(const liquid of [null,{},[],{mechanical_coupling:'{}'},{surface_water:{schema:'water'}},
    {surface_water:'null'},{surface_water:'[]'},{surface_water:'{bad'},
    {surface_water:'{}',surface_water_alternatives:'[{}]'},{surface_water_alternatives:'[{}]',mechanical_coupling:'{}'},
    {coupled_water_alternatives:'[{"surface_water":{},"mechanical_coupling":{}}]',mechanical_coupling:'{}'},
    {surface_water:'{}',unexpected:'{}'}])assert.throws(()=>build(motion,liquid));
  for(const field of ['surface_water_alternatives','coupled_water_alternatives']) {
    for(const raw of ['{}','[]','[null]','[[]]','[1]','['+Array(65).fill('{}').join(',')+']'])assert.throws(()=>build(motion,{[field]:raw}));
  }
  for(const raw of ['[{}]','[{"surface_water":{}}]','[{"surface_water":{},"mechanical_coupling":null}]']) {
    assert.throws(()=>build(motion,{coupled_water_alternatives:raw}));
  }
  assert.throws(()=>build(motionState(),direct),/independent-sheet heat/);
  assert.throws(()=>single({...scenarioState(),motionLiquid:direct}),/authored motion/);
  const original=' \r\n'+build(motion)+'\r\n';
  assert.throws(()=>single({...scenarioState(),motion:original,motionLiquid:direct,maxMotionTrials:1}),/Original motion/);
  assert.ok(single({...scenarioState(),motion:original,maxMotionTrials:1}).includes(original));
  const full='{}'+' '.repeat(16*1024*1024-2);
  assert.throws(()=>build(motion,{surface_water:full}),/16 MiB/,'Source bytes and authored framing share the complete request limit');
});

test('guided dry heat rejects missing conditions and invalid physical or work limits without admitting advanced policies', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise;
  const run=transient_surface=>build({...motionState(),transient_surface});
  for(const changes of [{schema:'wrong'},{boundaries:[]},{boundaries:[dryMotionHeat().boundaries[0],dryMotionHeat().boundaries[0]]},
    {boundaries:[null]},{surface_water:{}},{mechanical_coupling:{}},{exterior_exchange:[]}]) {
    assert.throws(()=>run({...dryMotionHeat(),...changes}));
  }
  for(const key of Object.keys(dryMotionHeat())) {const heat=dryMotionHeat();delete heat[key];assert.throws(()=>run(heat),key);}
  for(const value of [-273.150001,NaN,Infinity,'18',null]) {
    assert.throws(()=>run({...dryMotionHeat(),initial_temperature_c:value}));
    for(const key of ['inside_c','outside_c']) {const heat=dryMotionHeat();heat.boundaries[0][key]=value;assert.throws(()=>run(heat),key);}
  }
  for(const key of ['inside_resistance_m2_k_w','outside_resistance_m2_k_w']) {
    for(const value of [-0.001,NaN,Infinity,'0',null]) {const heat=dryMotionHeat();heat.boundaries[0][key]=value;assert.throws(()=>run(heat),key);}
    const missing=dryMotionHeat();delete missing.boundaries[0][key];assert.throws(()=>run(missing),key);
  }
  for(const [key,maximum] of [['cells_per_layer',64],['max_cell_steps',10000000]]) {
    for(const value of [0,-1,1.5,maximum+1,NaN,Infinity,'1'])assert.throws(()=>run({...dryMotionHeat(),[key]:value}),key);
  }
  for(const max_step_s of [0,5e-10,1.001,NaN,Infinity,'0.01'])assert.throws(()=>run({...dryMotionHeat(),max_step_s}));
  assert.equal(JSON.parse(run({...dryMotionHeat(),initial_temperature_c:-273.15,cells_per_layer:64,max_step_s:1,max_cell_steps:10000000}))
    .transient_surface.initial_temperature_c,-273.15);
});

test('guided moving sheet exterior schedules retain each qualified interval through single and named cases', async () => {
  const {buildMaterialMotionRequest:build,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const heat=dryMotionHeat(),first=exteriorPolicy(),second=exteriorPolicy({air_c:15,
    radiation:{ambient_c:10,automatic_material_face:true,allow_opaque_gray_approximation:true}});
  heat.boundaries.push({...heat.boundaries[0],outside_c:15});heat.exterior_exchange=[first,second];
  heat.initial_instance_temperatures_c={'front.R':10,'lining.R':30};
  const input={...motionState(),keyframes:[{time_s:0.25,arm_abduction_deg:30},{time_s:0.5,arm_abduction_deg:40}],transient_surface:heat};
  const before=JSON.stringify(input),raw=build(input);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(JSON.parse(raw).transient_surface,heat);
  const scenario=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});assert.ok(scenario.includes(raw));
  const saved=JSON.parse(matrix({maxEvaluations:2,cases:[{key:'changing environment',scenario}]})).cases[0].scenario;
  assert.deepEqual(saved.motion.transient_surface.exterior_exchange,[first,second]);assert.equal(Object.hasOwn(saved,'surface'),false);
  const legacy={...motionState(),transient_surface:dryMotionHeat()};
  assert.equal(build(legacy),JSON.stringify({schema:'ftx-drape-motion/1',...legacy}));
  legacy.transient_surface.exterior_exchange=null;
  assert.equal(JSON.parse(build(legacy)).transient_surface.exterior_exchange,null,'Native optional null remains distinct from an authored schedule');
});

test('moving sheet exterior schedules require every endpoint and refuse null entries or mismatched films', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise;
  const run=changes=>build({...motionState(),transient_surface:{...dryMotionHeat(),...changes}}),exterior=exteriorPolicy();
  for(const schedule of [undefined,{},[],[null],[undefined],[exterior,exterior]])assert.throws(()=>run({exterior_exchange:schedule}));
  for(const changes of [{outside_c:0},{outside_resistance_m2_k_w:0.1}]) {
    assert.throws(()=>run({boundaries:[{...dryMotionHeat().boundaries[0],...changes}],exterior_exchange:[exterior]}),/zero outside film/);
  }
  for(const extra of [{bare_boundaries:[null]},{exposed_exterior_exchange:[null]}])assert.throws(()=>run(extra),/unsupported/);
  assert.throws(()=>run({exterior_exchange:[{...exterior,allow_estimates:false}]}),/estimate consent/);
});

test('guided dry instance temperatures retain exact physical names, explicit fallback and immutable single and named requests', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const temperatures=JSON.parse('{"front.R":-15,"lining.R":30,"__proto__":12," side β ":0}');
  const heat={...dryMotionHeat(),initial_instance_temperatures_c:temperatures},input={...motionState(),transient_surface:heat};
  const before=JSON.stringify(input),raw=motion(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.transient_surface,heat);
  assert.deepEqual(Object.keys(result.transient_surface.initial_instance_temperatures_c),Object.keys(temperatures));
  assert.equal(result.transient_surface.initial_temperature_c,18,'Unlisted instances retain the authored scalar fallback');
  const legacy={...motionState(),transient_surface:dryMotionHeat()};
  assert.equal(motion(legacy),JSON.stringify({schema:'ftx-drape-motion/1',...legacy}),'Omission leaves existing uniform requests unchanged');
  const scenario=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  assert.ok(scenario.includes(raw));assert.equal(Object.hasOwn(JSON.parse(scenario),'surface'),false);
  const named=matrix({maxEvaluations:2,cases:[{key:'warm lining',scenario}]});
  assert.deepEqual(JSON.parse(named).cases[0].scenario.motion.transient_surface,heat);
  const original=' '+raw.replace('"front.R":-15','"front.R":-15.0')+'\r\n';
  assert.ok(single({...scenarioState(),motion:original,maxMotionTrials:1024,maxVertexFrames:16000000}).includes(original),
    'The original-motion route retains its exact native source without rewriting the override map');
});

test('guided dry instance temperatures reject ambiguous maps and nonphysical values while requiring the explicit fallback', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise;
  const run=initial_instance_temperatures_c=>build({...motionState(),transient_surface:{...dryMotionHeat(),initial_instance_temperatures_c}});
  for(const map of [null,undefined,[],{},'{}',new Map([['front.R',20]]),Object.create({'front.R':20}),
    {'':20},{' \t':20},...[-273.150001,NaN,Infinity,-Infinity,'20',null,undefined].map(value=>({'front.R':value}))]) {
    assert.throws(()=>run(map));
  }
  const heat={...dryMotionHeat(),initial_instance_temperatures_c:{'front.R':20}};delete heat.initial_temperature_c;
  assert.throws(()=>build({...motionState(),transient_surface:heat}),/missing/);
  assert.equal(JSON.parse(run({'front.R':-273.15})).transient_surface.initial_instance_temperatures_c['front.R'],-273.15);
  const long='unaliased physical instance '+ 'β'.repeat(2000);
  assert.equal(JSON.parse(run({[long]:0})).transient_surface.initial_instance_temperatures_c[long],0,
    'The builder neither invents a key-length restriction nor substitutes a source-piece or material alias');
  for(const extra of [{surface_water:{}},{mechanical_coupling:{}}]) {
    assert.throws(()=>build({...motionState(),transient_surface:{...dryMotionHeat(),initial_instance_temperatures_c:{'front.R':20},...extra}}),/unsupported/);
  }
});

test('guided dry instance temperatures obey the native entry count and complete UTF-8 request allowance', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise;
  const run=initial_instance_temperatures_c=>build({...motionState(),transient_surface:{...dryMotionHeat(),initial_instance_temperatures_c}});
  const largest=Object.fromEntries(Array.from({length:65536},(_,i)=>['instance-'+i,i%2?20:-10]));
  assert.equal(Object.keys(JSON.parse(run(largest)).transient_surface.initial_instance_temperatures_c).length,65536);
  assert.throws(()=>run({...largest,extra:0}),/1–65536/);
  const oversized=Object.fromEntries(Array.from({length:1024},(_,i)=>['é'.repeat(8192)+i,0]));
  assert.throws(()=>run(oversized),/16 MiB/);
});

test('motion work limits and nominal substeps match canonical bounds without inventing convergence', async () => {
  const {buildMaterialMotionRequest:build,buildMaterialScenarioRequest:scenario}=await modulePromise;
  for(const max_step_s of [0,5e-10,1.001,NaN,Infinity,'0.1']) assert.throws(()=>build({...motionState(),max_step_s}));
  for(const [key,maximum] of [['max_trials',512],['max_pairs',1000000],['max_sample_vertices',2000000]]) {
    for(const value of [0,-1,1.5,maximum+1,NaN,Infinity,'1',undefined]) assert.throws(()=>build({...motionState(),[key]:value}));
  }
  const rounded={...motionState(),keyframes:[{time_s:0.1,arm_abduction_deg:0},{time_s:0.30000000000000004,arm_abduction_deg:180}],
    max_step_s:0.1,max_trials:3};
  assert.equal(JSON.parse(build(rounded)).max_trials,3);
  assert.throws(()=>build({...rounded,max_trials:2}),/nominal substeps/);
  const minimum={keyframes:[{time_s:1e-9,arm_abduction_deg:0,leg_pose:{posture:'seated',fraction:0}}],
    max_step_s:1e-9,max_trials:1,max_pairs:1,max_sample_vertices:1};
  assert.equal(JSON.parse(build(minimum)).max_sample_vertices,1);
  const maximum={keyframes:[{time_s:60,arm_abduction_deg:180,leg_pose:{posture:'squat',fraction:1}}],
    max_step_s:1,max_trials:512,max_pairs:1000000,max_sample_vertices:2000000};
  assert.equal(JSON.parse(build(maximum)).keyframes[0].time_s,60);
  assert.equal(JSON.parse(scenario({...scenarioState(),motion:maximum,maxMotionTrials:1})).max_motion_trials,1,
    'The native session may preserve static results while refusing motion when its allowance cannot cover the requested trials');
  for(const maxMotionTrials of [0,-1,32769,1.5,NaN,Infinity,'4096',undefined]) {
    assert.throws(()=>scenario({...scenarioState(),motion:motionState(),maxMotionTrials}),/motion-trial budget/);
  }
  assert.throws(()=>scenario({...scenarioState(),motion:motionState()}),/supplied together/);
  assert.throws(()=>scenario({...scenarioState(),maxMotionTrials:4096}),/supplied together/);
  assert.equal(JSON.parse(scenario({...scenarioState(),motion:motionState(),maxMotionTrials:32768})).max_motion_trials,32768);
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

test('guided motion and material strain preserve exact source fragments and omitted native defaults', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const motion=' {\r\n"schema":"ftx-drape-motion/1","keyframes":[{"time_s":0.1,"arm_abduction_deg":-0.0,'+
    '"leg_pose":{"posture":"squat","fraction":1.0}},{"time_s":0.2,"arm_abduction_deg":35.0,"leg_pose":null}],'+
    '"max_step_s":0.01,"max_trials":32,"max_pairs":1000,"transient_surface":{"source":{"n":18446744073709551615,'+
    '"x":-0.0,"label":"é 🧵"}},"transient_coverage":null,"vapor_coverage_history":{}}\r\n';
  const materialStrain=' {\r\n"schema":"ftx-material-strain/1","instances":["front:left","lining"],"max_triangle_samples":10000}\r\n';
  const input={...scenarioState(),motion,maxMotionTrials:4096,materialStrain},before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(motion));assert.ok(raw.includes(materialStrain));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"x":-0\.0/);assert.match(raw,/"fraction":1\.0/);
  assert.deepEqual(result.motion,JSON.parse(motion));assert.deepEqual(result.material_strain,JSON.parse(materialStrain));
  assert.equal(result.max_motion_trials,4096);assert.equal(Object.hasOwn(result.motion,'max_sample_vertices'),false);
  for(const key of ['motion','material_strain','max_motion_trials'])assert.equal(Object.hasOwn(JSON.parse(build(scenarioState())),key),false);
  for(const instances of [undefined,null]) {
    const all=JSON.stringify(strainPolicy(instances===undefined?{}:{instances}));
    const independent=JSON.parse(build({...scenarioState(),materialStrain:all}));
    assert.deepEqual(independent.material_strain,JSON.parse(all));assert.equal(Object.hasOwn(independent,'motion'),false);
  }
});

test('authored motion and exact original motion compose with material strain without changing their contracts', async () => {
  const {buildMaterialMotionRequest:author,buildMaterialScenarioRequest:build}=await modulePromise;
  const motion=motionState(),original=' \r\n'+author(motion)+'\r\n',materialStrain=JSON.stringify(strainPolicy());
  const before=JSON.stringify(motion),input={...scenarioState(),maxMotionTrials:1,materialStrain};
  const authored=build({...input,motion}),retained=build({...input,motion:original});
  assert.deepEqual(JSON.parse(authored),JSON.parse(retained));assert.ok(retained.includes(original));
  assert.equal(JSON.stringify(motion),before);
  assert.equal(JSON.parse(authored).max_motion_trials,1,'A smaller aggregate allowance remains a native admission decision for both routes');
  assert.ok(authored.includes(materialStrain));assert.ok(retained.includes(materialStrain));
  assert.throws(()=>build({...input,motion:JSON.parse(original)}),/unsupported/,'Schema-bearing source objects must use original JSON text');
  for(const buildMotion of [author,value=>build({...input,motion:value})]) {
    assert.throws(()=>buildMotion({...motion,keyframes:[{...motion.keyframes[0],leg_pose:undefined}]}),/object/);
    assert.doesNotThrow(()=>buildMotion({...motion,keyframes:[{...motion.keyframes[0],leg_pose:null}]}));
  }
});

test('guided motion refuses invalid ordered poses and reserves the actual nominal substeps', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const check=policy=>build({...scenarioState(),motion:JSON.stringify(policy),maxMotionTrials:4096});
  for(const policy of [null,[],{},motionPolicy({schema:'wrong'}),motionPolicy({keyframes:[]}),
    motionPolicy({keyframes:Array(17).fill(motionPolicy().keyframes[0])}),motionPolicy({extra:1})])assert.throws(()=>check(policy));
  for(const key of ['schema','keyframes','max_step_s','max_trials','max_pairs']) {
    const missing=motionPolicy();delete missing[key];assert.throws(()=>check(missing),/missing/);
  }
  for(const time_s of [0,-1,5e-10,60.1,Infinity,NaN,'0.1']) {
    assert.throws(()=>check(motionPolicy({keyframes:[{time_s,arm_abduction_deg:35}]})),/separated/);
  }
  for(const time_s of [0.1,0.05,0.1+5e-10]) {
    assert.throws(()=>check(motionPolicy({keyframes:[{time_s:0.1,arm_abduction_deg:35},{time_s,arm_abduction_deg:35}]})),/separated/);
  }
  for(const arm_abduction_deg of [-1,180.1,Infinity,NaN,'35']) {
    assert.throws(()=>check(motionPolicy({keyframes:[{time_s:0.1,arm_abduction_deg}]})),/arm angles/);
  }
  for(const leg_pose of [{},{posture:'standing',fraction:0},{posture:'seated',fraction:-1},
    {posture:'squat',fraction:1.1},{posture:'squat',fraction:'0.5'},{posture:'seated',fraction:0.5,extra:1}]) {
    assert.throws(()=>check(motionPolicy({keyframes:[{time_s:0.1,arm_abduction_deg:35,leg_pose}]})));
  }
  assert.throws(()=>check(motionPolicy({keyframes:[{time_s:0.1,arm_abduction_deg:35,extra:1}]})),/unsupported/);
  for(const max_step_s of [0,1e-10,1.01,Infinity,'0.01'])assert.throws(()=>check(motionPolicy({max_step_s})),/maximum substep/);
  for(const [key,maximum] of [['max_trials',512],['max_pairs',1000000],['max_sample_vertices',2000000]]) {
    for(const value of [0,-1,maximum+1,1.5,'1',null])assert.throws(()=>check(motionPolicy({[key]:value})),/must be an integer/);
    assert.equal(JSON.parse(check(motionPolicy({[key]:maximum}))).motion[key],maximum);
  }
  assert.throws(()=>check(motionPolicy({max_trials:19})),/more nominal substeps/);
  assert.equal(JSON.parse(check(motionPolicy({max_trials:20}))).motion.max_trials,20);
  assert.equal(JSON.parse(check(motionPolicy({keyframes:[{time_s:1e-9,arm_abduction_deg:0}],max_step_s:1e-9,max_trials:1}))).motion.max_trials,1);
  assert.equal(JSON.parse(check(motionPolicy({keyframes:[{time_s:60,arm_abduction_deg:180}],max_step_s:1,max_trials:60}))).motion.keyframes[0].time_s,60);
  for(const key of ['transient_surface','transient_coverage','vapor_coverage_history'])for(const value of [[],false,'{}',1]) {
    assert.throws(()=>check(motionPolicy({[key]:value})),/original object or null/);
  }
});

test('guided motion requires its explicit aggregate budget and bounded original JSON text', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const input={...scenarioState(),motion:JSON.stringify(motionPolicy())};
  assert.throws(()=>build(input),/explicit integer scenario motion-trial budget/);
  for(const maxMotionTrials of [0,-1,32769,1.5,'1',null,Infinity])assert.throws(()=>build({...input,maxMotionTrials}),/motion-trial budget/);
  const inherited=Object.assign(Object.create({maxMotionTrials:4096}),input);
  assert.throws(()=>build(inherited),/explicit integer/);
  for(const maxMotionTrials of [1,32768])assert.equal(JSON.parse(build({...input,maxMotionTrials})).max_motion_trials,maxMotionTrials);
  assert.throws(()=>build({...scenarioState(),maxMotionTrials:4096}),/requires a prescribed motion/);
  for(const motion of [motionPolicy(),null,'null','[]','{bad','\ufeff'+input.motion])assert.throws(()=>build({...input,motion,maxMotionTrials:4096}));
  const fitSpec=JSON.stringify({source:'é'.repeat(4*1024*1024)}),
    motion=JSON.stringify(motionPolicy({transient_surface:{source:'x'.repeat(9*1024*1024)}}));
  assert.throws(()=>build({...input,fitSpec,motion,maxMotionTrials:4096}),/16 MiB/);
});

test('guided material strain validates exact distinct instance scope and explicit triangle-sample limits', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const check=policy=>build({...scenarioState(),materialStrain:JSON.stringify(policy)});
  for(const policy of [null,[],{},strainPolicy({schema:'wrong'}),strainPolicy({unknown:true})])assert.throws(()=>check(policy));
  for(const max_triangle_samples of [undefined,0,-1,64000001,1.5,null,'1',Infinity])assert.throws(()=>check(strainPolicy({max_triangle_samples})));
  for(const max_triangle_samples of [1,64000000])assert.equal(JSON.parse(check(strainPolicy({max_triangle_samples}))).material_strain.max_triangle_samples,max_triangle_samples);
  for(const instances of [[],[''],[' '],['front','front'],[1],{},Array.from({length:4097},(_,i)=>String(i)),['é'.repeat(2049)]]) {
    assert.throws(()=>check(strainPolicy({instances})));
  }
  const instances=['é'.repeat(2048),'__proto__','front:right'];
  assert.deepEqual(JSON.parse(check(strainPolicy({instances}))).material_strain.instances,instances);
  for(const materialStrain of [strainPolicy(),'null','[]','{bad','\ufeff'+JSON.stringify(strainPolicy())]) {
    assert.throws(()=>build({...scenarioState(),materialStrain}));
  }
});

test('guided contact loads retain original scope and budget text alongside motion, strain and fit sources', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const contactLoads=' {\r\n"max_pair_evaluations":1000000,"schema":"ftx-contact-loads/1",'+
    '"instances":["\\u0066ront:right"," lining 🧵 "],"max_vertex_samples":100000}\r\n';
  const fitSpec=' {"source":{"seed":18446744073709551615,"zero":-0.0}}\r\n';
  const materialStrain=JSON.stringify(strainPolicy()),motion=' \r\n'+JSON.stringify(motionPolicy())+'\r\n';
  const input={...scenarioState(),contactLoads,fitSpec,materialStrain,motion,maxMotionTrials:4096},before=JSON.stringify(input);
  const raw=build(input),result=JSON.parse(raw);
  for(const original of [contactLoads,fitSpec,materialStrain,motion])assert.ok(raw.includes(original));
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.contact_loads,JSON.parse(contactLoads));
  assert.equal(Object.hasOwn(result,'contactLoads'),false);
  assert.equal(Object.hasOwn(JSON.parse(build(scenarioState())),'contact_loads'),false);
  for(const instances of [undefined,null]) {
    const original=JSON.stringify(contactPolicy(instances===undefined?{}:{instances}));
    const independent=JSON.parse(build({...scenarioState(),contactLoads:original}));
    assert.deepEqual(independent.contact_loads,JSON.parse(original));
    for(const key of ['motion','material_strain','max_motion_trials'])assert.equal(Object.hasOwn(independent,key),false);
  }
});

test('guided contact loads validate exact native scopes, bounded allowances and complete original input size', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const check=policy=>build({...scenarioState(),contactLoads:JSON.stringify(policy)});
  for(const policy of [null,[],{},contactPolicy({schema:'wrong'}),contactPolicy({unknown:true})])assert.throws(()=>check(policy));
  for(const key of ['schema','max_vertex_samples','max_pair_evaluations']) {
    const missing=contactPolicy();delete missing[key];assert.throws(()=>check(missing),/missing/);
  }
  for(const key of ['max_vertex_samples','max_pair_evaluations']) {
    for(const value of [0,-1,64000001,1.5,null,'1',Infinity])assert.throws(()=>check(contactPolicy({[key]:value})),/integer vertex\/pair budgets/);
    for(const value of [1,64000000])assert.equal(JSON.parse(check(contactPolicy({[key]:value}))).contact_loads[key],value);
  }
  for(const instances of [[],[''],[' \t'],['front','front'],[1],{},Array.from({length:4097},(_,i)=>String(i)),['é'.repeat(2049)]]) {
    assert.throws(()=>check(contactPolicy({instances})));
  }
  const names=['é'.repeat(2048),'__proto__','front',' front '];
  assert.deepEqual(JSON.parse(check(contactPolicy({instances:names}))).contact_loads.instances,names);
  const limit=Array.from({length:4096},(_,i)=>'part-'+i);
  assert.equal(JSON.parse(check(contactPolicy({instances:limit}))).contact_loads.instances.length,4096);
  for(const contactLoads of [contactPolicy(),null,undefined,'null','[]','{bad','\ufeff'+JSON.stringify(contactPolicy())]) {
    assert.throws(()=>build({...scenarioState(),contactLoads}));
  }
  const original=JSON.stringify(contactPolicy()),contactLoads=original+' '.repeat(16*1024*1024-original.length);
  assert.throws(()=>build({...scenarioState(),contactLoads}),/16 MiB/,'Scenario framing shares the input limit with the exact contact source');
});

test('guided covered heat and vapor retain every original fragment without inventing optional policies', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  const coverage=' \r\n'+JSON.stringify(coveragePolicy()).replace('"band"','"\\u0062and"').replace('"inside_c":30','"inside_c":30.0')+'\r\n';
  const vaporSurface=' \r\n'+JSON.stringify(vaporSurfacePolicy({required_method:'ISO 11092',required_conditions:{},allow_mixed_protocols:true}))
    .replace('"latent_heat_j_kg":2500000','"latent_heat_j_kg":2.500000e6')+'\r\n';
  const vaporCoverage=' \r\n'+JSON.stringify(vaporCoveragePolicy()).replace('"inside_film_resistance_m2_pa_w":1','"inside_film_resistance_m2_pa_w":-0.0')+'\r\n';
  const motion=' \r\n'+JSON.stringify(motionPolicy({vapor_coverage_history:vaporHistoryPolicy()}))+'\r\n';
  const input={...scenarioState(),coverage,vaporSurface,vaporCoverage,motion,maxMotionTrials:4096,
    fitSpec:' {"source":{"sequence":18446744073709551615}}\r\n',surface:heatSource(),
    materialStrain:JSON.stringify(strainPolicy()),contactLoads:JSON.stringify(contactPolicy())};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  for(const original of [coverage,vaporSurface,vaporCoverage,motion,input.fitSpec,input.surface,input.materialStrain,input.contactLoads])assert.ok(raw.includes(original));
  for(const [field,key] of [['coverage','coverage'],['vaporSurface','vapor_surface'],['vaporCoverage','vapor_coverage']]) {
    assert.deepEqual(result[key],JSON.parse(input[field]));assert.equal(Object.hasOwn(JSON.parse(build(scenarioState())),key),false);
  }
  assert.match(raw,/"inside_c":30\.0/);assert.match(raw,/2\.500000e6/);assert.match(raw,/"inside_film_resistance_m2_pa_w":-0\.0/);
  for(const key of ['vaporSurface','vaporCoverage','motionVaporHistory','max_thermal_operations'])assert.equal(Object.hasOwn(result,key),false);
  assert.equal(Object.hasOwn(result.coverage,'thermal_bridges'),false);
  assert.equal(Object.hasOwn(result.vapor_coverage.conditions,'required_method'),false,'Native optional defaults are not inserted into source files');
});

const authoredCoverage = () => ({bodyTriangleIds:'4-6, 2\n9',
  instanceLayers:[{instance:'front.R',layer:'shell'},{instance:'lining.R',layer:'lining'}],
  settings:{max_distance_m:0.2,min_normal_cosine:0.1,separation_tolerance_m:0.00001,cells_per_layer:2,
    interfaces:[{inner:'lining',outer:'shell',resistance_m2_k_w:0,basis:'Explicit zero-resistance synthetic assumption'}]},
  boundary:{inside_c:30,outside_c:5,inside_resistance_m2_k_w:0.1,outside_resistance_m2_k_w:0.2}});

test('authored coverage expands bounded body ranges and preserves exact physical layer ownership through single and named cases', async () => {
  const {buildMaterialCoverageRequest:build,buildMaterialScenarioRequest:scenario,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const input=authoredCoverage();input.instanceLayers.push({instance:'__proto__',layer:'shell'},{instance:' front.R ',layer:'shell'});
  input.settings.interfaces[0].basis='Declared interface <café 🧵>';const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.body_triangle_ids,[4,5,6,2,9]);
  assert.deepEqual(result.instance_layers,JSON.parse('{"front.R":"shell","lining.R":"lining","__proto__":"shell"," front.R ":"shell"}'));
  assert.deepEqual(result.settings,input.settings);assert.deepEqual(result.conditions,{boundaries:[input.boundary]});
  assert.equal(Object.hasOwn(result,'schema'),false);assert.equal(Object.hasOwn(result,'thermal_bridges'),false);
  const first=scenario({...scenarioState(),maxVertexFrames:32000000,coverage:raw});assert.ok(first.includes(raw));
  input.boundary.outside_c=-10;input.settings.interfaces[0].resistance_m2_k_w=0.05;
  const second=scenario({...scenarioState(),maxVertexFrames:32000000,coverage:build(input)});
  const cases=JSON.parse(matrix({maxEvaluations:4,cases:[{key:'first',scenario:first},{key:'second',scenario:second}]})).cases;
  assert.deepEqual(cases.map(c=>c.scenario.coverage.conditions.boundaries[0].outside_c),[5,-10]);
  assert.deepEqual(cases.map(c=>c.scenario.coverage.settings.interfaces[0].resistance_m2_k_w),[0,0.05]);
});

test('guided steady coverage keeps qualified cloth exchange separate from explicitly authored bare-body films', async () => {
  const {buildMaterialCoverageRequest:build,buildMaterialScenarioRequest:scenario,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const legacy=authoredCoverage(),input={...legacy,boundary:{...legacy.boundary,outside_resistance_m2_k_w:0},
    exterior:exteriorPolicy({air_c:5}),bareBoundary:{...legacy.boundary,outside_c:10}};
  const before=JSON.stringify(input),raw=build(input),conditions=JSON.parse(raw).conditions;
  assert.equal(JSON.stringify(input),before);
  assert.deepEqual(conditions,{boundaries:[input.boundary],exterior_exchange:[input.exterior],bare_boundaries:[input.bareBoundary]});
  const first=scenario({...scenarioState(),coverage:raw,maxVertexFrames:32000000});
  input.exterior.convection_w_m2_k=12;input.bareBoundary.outside_c=15;
  const second=scenario({...scenarioState(),coverage:build(input),maxVertexFrames:32000000});
  const cases=JSON.parse(matrix({maxEvaluations:4,cases:[{key:'first',scenario:first},{key:'second',scenario:second}]})).cases;
  assert.deepEqual(cases.map(c=>c.scenario.coverage.conditions.exterior_exchange[0].convection_w_m2_k),[8,12]);
  assert.deepEqual(cases.map(c=>c.scenario.coverage.conditions.bare_boundaries[0].outside_c),[10,15]);
  assert.equal(build({...legacy,exterior:null,bareBoundary:null}),build(legacy));
  const missing={...input};delete missing.bareBoundary;assert.throws(()=>build(missing),/explicit bare-body/);
  for(const bareBoundary of [null,undefined,{},[],{...input.bareBoundary,inside_c:-274}])assert.throws(()=>build({...input,bareBoundary}));
  for(const exterior of [null,undefined])assert.throws(()=>build({...input,exterior}),/require covered exterior/);
  for(const boundary of [legacy.boundary,{...input.boundary,outside_c:0}])assert.throws(()=>build({...input,boundary}),/zero outside film/);
});

test('authored coverage refuses ambiguous or oversized body ranges before expansion', async () => {
  const {buildMaterialCoverageRequest:build}=await modulePromise;
  for(const bodyTriangleIds of ['',null,[],[1],'-1','1.5','1e2','1--2','3-1','1 - 2','1,1','1-3 3-5',
    '0-4096','0-4095,4096',Array.from({length:4097},(_,i)=>i).join(','),'9007199254740992','0-9007199254740991','0,',' '.repeat(16*1024*1024)+'0']) {
    assert.throws(()=>build({...authoredCoverage(),bodyTriangleIds}),String(bodyTriangleIds).slice(0,100));
  }
  const ids=JSON.parse(build({...authoredCoverage(),bodyTriangleIds:'0-4095'})).body_triangle_ids;
  assert.equal(ids.length,4096);assert.equal(ids[0],0);assert.equal(ids.at(-1),4095);
  assert.deepEqual(JSON.parse(build({...authoredCoverage(),bodyTriangleIds:'9007199254740991'})).body_triangle_ids,[Number.MAX_SAFE_INTEGER],
    'Actual body topology remains a native-consumer check');
});

test('authored coverage requires explicit complete geometry, physical assignments and sourced directed interfaces', async () => {
  const {buildMaterialCoverageRequest:build}=await modulePromise;
  const input=authoredCoverage();
  for(const instanceLayers of [null,[],{},[{instance:'front.R',layer:''}],
    [{instance:'front.R',layer:'shell'},{instance:'front.R',layer:'lining'}],
    [{instance:'front.R',layer:1}],[{instance:'front.R',layer:'shell',extra:1}],
    [{instance:'é'.repeat(2049),layer:'shell'}]])assert.throws(()=>build({...input,instanceLayers}));
  for(const [key,values] of Object.entries({max_distance_m:[0,-1,Infinity,'1'],min_normal_cosine:[0,1.01,NaN,'0.5'],
    separation_tolerance_m:[0,-1,0.2,1],cells_per_layer:[0,65,1.5,'1'],interfaces:[null,{},Array(4097).fill(input.settings.interfaces[0])]})) {
    for(const value of values)assert.throws(()=>build({...input,settings:{...input.settings,[key]:value}}),key+':'+value);
  }
  const interfaceRow=input.settings.interfaces[0];
  for(const row of [{...interfaceRow,inner:'missing'},{...interfaceRow,outer:'lining'},
    {...interfaceRow,basis:''},{...interfaceRow,resistance_m2_k_w:-0.1},{...interfaceRow,resistance_m2_k_w:'0'},
    {...interfaceRow,stagnant_gap:{}},{...interfaceRow,basis:'x'.repeat(4097)}]) {
    assert.throws(()=>build({...input,settings:{...input.settings,interfaces:[row]}}));
  }
  assert.throws(()=>build({...input,settings:{...input.settings,interfaces:[interfaceRow,interfaceRow]}}),/uniquely/);
  for(const key of Object.keys(input)) {const missing={...input};delete missing[key];assert.throws(()=>build(missing),/missing/);}
  for(const key of Object.keys(input.settings)) {const settings={...input.settings};delete settings[key];assert.throws(()=>build({...input,settings}),/missing/);}
  assert.throws(()=>build({...input,thermal_bridges:{}}),/unsupported/);
  assert.throws(()=>build({...input,boundary:{...input.boundary,outside_c:-273.151}}));
  assert.throws(()=>build({...input,boundary:{...input.boundary,inside_resistance_m2_k_w:-1}}));
  const oneLayer=JSON.parse(build({...input,instanceLayers:[{instance:'front.R',layer:'shell'}],settings:{...input.settings,interfaces:[]}}));
  assert.deepEqual(oneLayer.settings.interfaces,[],'An empty table declares no cross-layer resistance; actual adjacent paths remain native-owned');
});

test('moving coverage topology is independent of steady conditions and preserves the existing steady request', async () => {
  const {buildMaterialCoverageTopology:topology,buildMaterialCoverageRequest:steady}=await modulePromise;
  const {boundary,...input}=authoredCoverage(),before=JSON.stringify(input),raw=topology(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.deepEqual(Object.keys(result),['body_triangle_ids','instance_layers','settings']);
  assert.equal(steady({...input,boundary}),JSON.stringify({...result,conditions:{boundaries:[boundary]}}));
  for(const extra of [{boundary},{conditions:{boundaries:[boundary]}},{schema:'ftx-motion-coverage-heat/1'}]) {
    assert.throws(()=>topology({...input,...extra}),/unsupported/);
  }
  for(const key of Object.keys(input)) {const missing={...input};delete missing[key];assert.throws(()=>topology(missing),/missing/);}
});

async function guidedMovingCoverageHeat() {
  const {buildMaterialCoverageTopology:topology}=await modulePromise;
  const {boundary,...input}=authoredCoverage();
  return {schema:'ftx-motion-coverage-heat/1',...JSON.parse(topology(input)),initial_temperature_c:18,
    boundaries:[boundary],exposed_boundaries:[null],max_step_s:0.01,max_cell_steps:100000,max_geometry_operations:1000000};
}

test('guided moving coverage keeps independent temperatures and covered or adiabatic exposed intervals through saved cases', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const coverage=await guidedMovingCoverageHeat(),sheet=dryMotionHeat();
  coverage.initial_temperature_c=999;coverage.initial_instance_temperatures_c={'front.R':5,'lining.R':35};
  coverage.boundaries.push({...coverage.boundaries[0],inside_c:-5,outside_c:40});
  coverage.exposed_boundaries.push({inside_c:12,outside_c:-2,inside_resistance_m2_k_w:0,outside_resistance_m2_k_w:0.3});
  sheet.initial_instance_temperatures_c={'front.R':25};sheet.boundaries.push({...sheet.boundaries[0],inside_c:15});
  const input={...motionState(),keyframes:[{time_s:0.25,arm_abduction_deg:30},{time_s:0.5,arm_abduction_deg:40}],
    transient_surface:sheet,transient_coverage:coverage},before=JSON.stringify(input),raw=motion(input);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(JSON.parse(raw).transient_coverage,coverage);
  assert.deepEqual(JSON.parse(raw).transient_surface,sheet);
  assert.deepEqual(JSON.parse(raw).transient_coverage.exposed_boundaries,[null,coverage.exposed_boundaries[1]]);
  const first=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  assert.ok(first.includes(raw));assert.equal(Object.hasOwn(JSON.parse(first),'coverage'),false);
  assert.equal(Object.hasOwn(JSON.parse(first),'surface'),false,'Moving histories do not request either steady heat consumer');
  const comparison=JSON.stringify({basis:'nominal',objectives:[{metric:'transient_coverage_final_min_temperature_c',direction:'maximize'}],
    required_consumers:['transient_coverage']});
  coverage.initial_instance_temperatures_c['front.R']=-10;coverage.exposed_boundaries[0]={...coverage.boundaries[0]};
  const second=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  const cases=JSON.parse(matrix({maxEvaluations:4,cases:[{key:'warm lining',scenario:first,comparison},{key:'cold shell',scenario:second,comparison}]})).cases;
  assert.deepEqual(cases.map(c=>c.scenario.motion.transient_coverage.initial_instance_temperatures_c['front.R']),[5,-10]);
  assert.equal(cases[0].scenario.motion.transient_coverage.exposed_boundaries[0],null);
  assert.deepEqual(cases.map(c=>c.scenario.motion.transient_surface.initial_instance_temperatures_c['front.R']),[25,25]);
});

test('guided dissipation heat preserves independent channel choices and mapped initial states through named cases', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const channels=['plastic_dissipation_heat','relaxation_dissipation_heat','bending_plastic_dissipation_heat'];
  for(const consumer of ['transient_surface','transient_coverage'])for(let mask=0;mask<8;mask++) {
    const heat=consumer==='transient_surface'?dryMotionHeat():await guidedMovingCoverageHeat();
    heat.initial_temperature_c=999;heat.initial_instance_temperatures_c={'front.R':5,'lining.R':35};
    for(const [i,field] of channels.entries())if(mask&(1<<i))heat[field]='uniform_reference_thickness';
    const input={...motionState(),[consumer]:heat},before=JSON.stringify(input),raw=motion(input);
    assert.equal(JSON.stringify(input),before);assert.deepEqual(JSON.parse(raw)[consumer],heat);
    assert.equal(Object.hasOwn(JSON.parse(raw),consumer==='transient_surface'?'transient_coverage':'transient_surface'),false);
  }
  const sheet={...dryMotionHeat(),initial_instance_temperatures_c:{'front.R':5},
    plastic_dissipation_heat:'uniform_reference_thickness',relaxation_dissipation_heat:'uniform_reference_thickness'},
    coverage={...await guidedMovingCoverageHeat(),initial_instance_temperatures_c:{'front.R':35},
      bending_plastic_dissipation_heat:'uniform_reference_thickness',plastic_dissipation_heat:null};
  const input={...motionState(),transient_surface:sheet,transient_coverage:coverage};
  const first=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  sheet.bending_plastic_dissipation_heat='uniform_reference_thickness';delete sheet.plastic_dissipation_heat;
  coverage.relaxation_dissipation_heat='uniform_reference_thickness';coverage.initial_instance_temperatures_c['front.R']=25;
  const second=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});
  const cases=JSON.parse(matrix({maxEvaluations:4,cases:[{key:'original heating',scenario:first},{key:'changed heating',scenario:second}]})).cases;
  assert.equal(cases[0].scenario.motion.transient_surface.plastic_dissipation_heat,'uniform_reference_thickness');
  assert.equal(Object.hasOwn(cases[0].scenario.motion.transient_surface,'bending_plastic_dissipation_heat'),false);
  assert.equal(cases[0].scenario.motion.transient_coverage.plastic_dissipation_heat,null);
  assert.equal(Object.hasOwn(cases[0].scenario.motion.transient_coverage,'relaxation_dissipation_heat'),false);
  assert.equal(cases[0].scenario.motion.transient_coverage.initial_instance_temperatures_c['front.R'],35);
  assert.deepEqual(cases[1].scenario.motion.transient_surface,sheet);assert.deepEqual(cases[1].scenario.motion.transient_coverage,coverage);
});

test('guided dissipation heat accepts only native opt-ins and never accepts browser conversion fractions', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single}=await modulePromise;
  const channels=['plastic_dissipation_heat','relaxation_dissipation_heat','bending_plastic_dissipation_heat'];
  for(const consumer of ['transient_surface','transient_coverage']) {
    const heat=consumer==='transient_surface'?dryMotionHeat():await guidedMovingCoverageHeat();
    for(const field of channels) {
      for(const value of [null,'uniform_reference_thickness']) {
        assert.equal(JSON.parse(motion({...motionState(),[consumer]:{...heat,[field]:value}}))[consumer][field],value);
      }
      for(const value of [undefined,false,true,0,0.5,[],{},'uniform','UNIFORM_REFERENCE_THICKNESS',
        {distribution:'uniform_reference_thickness',fraction:1}]) {
        assert.throws(()=>motion({...motionState(),[consumer]:{...heat,[field]:value}}),/uniform_reference_thickness/);
      }
    }
    assert.throws(()=>motion({...motionState(),[consumer]:{...heat,heat_fraction:0.8}}),/unsupported/);
    const original=' \r\n'+JSON.stringify({schema:'ftx-drape-motion/1',...motionState(),[consumer]:{
      ...heat,initial_instance_temperatures_c:{'front.R':5},...Object.fromEntries(channels.map(field=>[field,'uniform_reference_thickness']))}})
      .replace('"initial_temperature_c":18','"initial_temperature_c":18.00')+'\r\n';
    const raw=single({...scenarioState(),motion:original,maxMotionTrials:1024,maxVertexFrames:16000000});
    assert.ok(raw.includes(original),'Original policy bytes remain authoritative, including native optional heat channels');
  }
});

test('guided moving coverage preserves independent covered, bare and exposed exterior interval choices', async () => {
  const {buildMaterialMotionRequest:build,buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const coverage=await guidedMovingCoverageHeat(),film=coverage.boundaries[0],covered=exteriorPolicy({air_c:film.outside_c});
  const exposed=exteriorPolicy({radiation:{ambient_c:-15,automatic_material_face:true,allow_opaque_gray_approximation:true}});
  coverage.boundaries=[{...film,outside_resistance_m2_k_w:0},film];
  coverage.exterior_exchange=[covered,null];coverage.bare_boundaries=[{...film,outside_c:12},null];
  coverage.exposed_boundaries=[null,dryMotionHeat().boundaries[0]];coverage.exposed_exterior_exchange=[null,exposed];
  const input={...motionState(),keyframes:[{time_s:0.25,arm_abduction_deg:30},{time_s:0.5,arm_abduction_deg:40}],transient_coverage:coverage};
  const before=JSON.stringify(input),raw=build(input);assert.equal(JSON.stringify(input),before);
  assert.deepEqual(JSON.parse(raw).transient_coverage,coverage);
  const scenario=single({...scenarioState(),motion:input,maxMotionTrials:1024,maxVertexFrames:16000000});assert.ok(scenario.includes(raw));
  const saved=JSON.parse(matrix({maxEvaluations:2,cases:[{key:'exposed after movement',scenario}]})).cases[0].scenario;
  assert.deepEqual(saved.motion.transient_coverage,coverage);assert.equal(Object.hasOwn(saved,'coverage'),false);
  const onlyExposed={...await guidedMovingCoverageHeat(),exposed_boundaries:[dryMotionHeat().boundaries[0]],exposed_exterior_exchange:[exposed]};
  assert.deepEqual(JSON.parse(build({...motionState(),transient_coverage:onlyExposed})).transient_coverage,onlyExposed);
  for(const value of [null,[null]]) {
    const filmOnly={...await guidedMovingCoverageHeat(),exterior_exchange:value,bare_boundaries:value,exposed_exterior_exchange:value};
    assert.deepEqual(JSON.parse(build({...motionState(),transient_coverage:filmOnly})).transient_coverage,filmOnly);
  }
});

test('moving coverage exterior schedules refuse missing bare films, misplaced exposed heat and mismatched interval counts', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise,coverage=await guidedMovingCoverageHeat(),film=coverage.boundaries[0];
  const exterior=exteriorPolicy({air_c:film.outside_c}),active={...coverage,
    boundaries:[{...film,outside_resistance_m2_k_w:0}],exterior_exchange:[exterior],bare_boundaries:[film]};
  const run=changes=>build({...motionState(),transient_coverage:{...coverage,...changes}});
  for(const field of ['exterior_exchange','bare_boundaries','exposed_exterior_exchange']) {
    for(const value of [undefined,{},[],[undefined],[null,null]])assert.throws(()=>run({[field]:value}),field);
  }
  const missingBare={...active};delete missingBare.bare_boundaries;assert.throws(()=>run(missingBare),/explicit bare-body/);
  for(const changes of [{bare_boundaries:null},{bare_boundaries:[null]},{exterior_exchange:null},{exterior_exchange:[null]}]) {
    assert.throws(()=>run({...active,...changes}),/explicit bare-body/);
  }
  assert.throws(()=>run({bare_boundaries:[film]}),/explicit bare-body/);
  assert.throws(()=>run({...active,bare_boundaries:[{...film,inside_c:-274}]}),/finite and at least/);
  for(const boundaries of [[film],[{...active.boundaries[0],outside_c:0}]])assert.throws(()=>run({...active,boundaries}),/zero outside film/);
  assert.throws(()=>run({exposed_exterior_exchange:[exterior]}),/explicit exposed film/);
  for(const exposed_boundaries of [[film],[{...active.boundaries[0],outside_c:0}]]) {
    assert.throws(()=>run({exposed_boundaries,exposed_exterior_exchange:[exterior]}),/zero outside film/);
  }
  assert.throws(()=>run({...active,exterior_exchange:[{...exterior,allow_estimates:false}]}),/estimate consent/);
});

test('guided moving coverage requires complete saved topology and independent explicit interval choices', async () => {
  const {buildMaterialMotionRequest:build}=await modulePromise,coverage=await guidedMovingCoverageHeat();
  const run=transient_coverage=>build({...motionState(),transient_coverage});
  for(const key of Object.keys(coverage)) {const missing={...coverage};delete missing[key];assert.throws(()=>run(missing),/missing/,key);}
  for(const changes of [{schema:'other'},{body_triangle_ids:[]},{body_triangle_ids:[1,1]},{body_triangle_ids:['1']},
    {body_triangle_ids:[-1]},{body_triangle_ids:[Number.MAX_SAFE_INTEGER+1]},{body_triangle_ids:Array.from({length:4097},(_,i)=>i)},
    {instance_layers:null},{instance_layers:[]},{instance_layers:{}},{instance_layers:{'front.R':1}},
    {settings:{...coverage.settings,cells_per_layer:65}},{settings:{...coverage.settings,min_normal_cosine:0}},
    {boundaries:[]},{boundaries:[null]},{boundaries:[coverage.boundaries[0],coverage.boundaries[0]]},
    {exposed_boundaries:[]},{exposed_boundaries:[undefined]},{exposed_boundaries:[{}]},{exposed_boundaries:[null,null]},
    {initial_instance_temperatures_c:{}},{initial_instance_temperatures_c:{'front.R':-274}},
    {initial_instance_temperatures_c:{'front.R':'5'}}])assert.throws(()=>run({...coverage,...changes}),JSON.stringify(changes));
  for(const field of ['boundaries','exposed_boundaries'])for(const changes of [{inside_c:-274},{outside_c:Infinity},
    {inside_resistance_m2_k_w:-0.1},{outside_resistance_m2_k_w:'0'},{extra:0}]) {
    assert.throws(()=>run({...coverage,[field]:[{...coverage.boundaries[0],...changes}]}));
  }
  const maximum={...coverage,body_triangle_ids:Array.from({length:4096},(_,i)=>i),
    settings:{...coverage.settings,cells_per_layer:64},max_step_s:1,max_cell_steps:10000000,max_geometry_operations:20000000};
  assert.deepEqual(JSON.parse(run(maximum)).transient_coverage,maximum);
  for(const [key,maximum] of [['max_cell_steps',10000000],['max_geometry_operations',20000000]]) {
    for(const value of [0,-1,1.5,maximum+1,NaN,Infinity,'1'])assert.throws(()=>run({...coverage,[key]:value}),key);
  }
  for(const value of [0,5e-10,1.001,NaN,Infinity,'0.01'])assert.throws(()=>run({...coverage,max_step_s:value}));
  for(const value of [-273.150001,NaN,Infinity,'18',null])assert.throws(()=>run({...coverage,initial_temperature_c:value}));
});

test('guided moving coverage retains its actual consumer capability and leaves advanced original motion policies untouched', async () => {
  const {buildMaterialMotionRequest:motion,buildMaterialScenarioRequest:single,buildMaterialStudyRequest:study}=await modulePromise;
  const coverage=await guidedMovingCoverageHeat(),input={...motionState(),transient_coverage:coverage};
  const objectives=[{metric:'transient_coverage_inside_energy_j',direction:'minimize'},
    {metric:'transient_coverage_final_min_temperature_c',direction:'maximize'}];
  const constraints=[{metric:'transient_coverage_max_sample_temperature_c',maximum:40}];
  const result=JSON.parse(study({...state(),objectives,constraints,requiredConsumers:['transient_coverage']},
    {drape:true,surface:false,motion:true,transientCoverage:true}));
  assert.deepEqual(result.comparison.objectives,objectives);assert.deepEqual(result.comparison.constraints,constraints);
  for(const context of [{drape:true,surface:false,motion:true},{drape:true,surface:false,motion:true,transientSurface:true}]) {
    assert.throws(()=>study({...state(),objectives},context),/transient/i);
  }
  for(const extra of [{cells_per_layer:2},{conditions:{}},{surface_water:{}},{thermal_bridges:null}]) {
    assert.throws(()=>motion({...input,transient_coverage:{...coverage,...extra}}),/unsupported/);
  }
  const advanced={...coverage,exterior_exchange:[null],bare_boundaries:[null],thermal_bridges:null};
  const original=' \r\n'+JSON.stringify({schema:'ftx-drape-motion/1',...input,transient_coverage:advanced})
    .replace('"initial_temperature_c":18','"initial_temperature_c":18.00')+'\r\n';
  const raw=single({...scenarioState(),motion:original,maxMotionTrials:1024,maxVertexFrames:16000000});
  assert.ok(raw.includes(original));assert.deepEqual(JSON.parse(raw).motion.transient_coverage,advanced);
  assert.equal(motion(motionState()),JSON.stringify({schema:'ftx-drape-motion/1',...motionState()}));
});

test('guided coverage envelopes refuse source coercion and wrappers while Rust retains physical admission', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  for(const [field,policy] of [['coverage',coveragePolicy],['vaporCoverage',vaporCoveragePolicy]]) {
    const check=value=>build({...scenarioState(),[field]:JSON.stringify(value)});
    for(const source of [policy(),undefined,null,'null','[]','{bad','\ufeff'+JSON.stringify(policy())]) {
      assert.throws(()=>build({...scenarioState(),[field]:source}));
    }
    for(const key of ['body_triangle_ids','instance_layers','settings','conditions']) {
      const missing=policy();delete missing[key];assert.throws(()=>check(missing),/missing/);
    }
    for(const extra of [{schema:'ftx-coverage/1'},{report:{}},{map:{}}])assert.throws(()=>check(policy(extra)),/unsupported/);
    for(const body_triangle_ids of [null,{},['2'],[1.5],[-1]])assert.throws(()=>check(policy({body_triangle_ids})),/integer body-triangle/);
    for(const key of ['instance_layers','settings','conditions'])for(const value of [null,[],1,'{}']) {
      assert.throws(()=>check(policy({[key]:value})),/must be an object/);
    }
    assert.throws(()=>check(policy({instance_layers:{band:1}})),/string instance-to-layer/);
    const unqualified=policy({body_triangle_ids:[99999,99999],instance_layers:{unknown:'absent-layer'},
      settings:{...policy().settings,max_distance_m:-1}});
    assert.deepEqual(JSON.parse(check(unqualified))[field==='coverage'?'coverage':'vapor_coverage'],unqualified,
      'Exact body IDs, layer presence and physical projection domains remain independent native-consumer checks');
    assert.ok(check(policy({body_triangle_ids:[],instance_layers:{}})));
  }
  for(const thermal_bridges of [null,{}])assert.ok(build({...scenarioState(),coverage:JSON.stringify(coveragePolicy({thermal_bridges}))}));
  for(const thermal_bridges of [[],1,'{}'])assert.throws(()=>build({...scenarioState(),coverage:JSON.stringify(coveragePolicy({thermal_bridges}))}),/object or null/);
  assert.throws(()=>build({...scenarioState(),vaporCoverage:JSON.stringify(vaporCoveragePolicy({thermal_bridges:null}))}),/unsupported/);
  for(const vaporSurface of [vaporSurfacePolicy(),'null','[]','{bad','\ufeff'+JSON.stringify(vaporSurfacePolicy()),
    ...[{},vaporSurfacePolicy({schema:'wrong'}),vaporSurfacePolicy({boundaries:[]}),vaporSurfacePolicy({boundaries:{}}),
      vaporSurfacePolicy({body_mesh_sha256:'a'.repeat(64)}),vaporSurfacePolicy({boundaries:Array(65537).fill(vaporBoundary())})].map(JSON.stringify)]) {
    assert.throws(()=>build({...scenarioState(),vaporSurface}));
  }
  const unqualified=JSON.stringify(vaporSurfacePolicy({boundaries:[{...vaporBoundary(),outside_vapor_pressure_pa:3000}],required_method:'unavailable-source-method'}));
  assert.ok(build({...scenarioState(),vaporSurface:unqualified}).includes(unqualified),'Pressure direction and source qualification are not fabricated or preflighted by authoring');
});

test('covered heat and vapor fragments share the complete scenario UTF-8 byte allowance', async () => {
  const {buildMaterialScenarioRequest:build}=await modulePromise;
  for(const [field,policy] of [['coverage',coveragePolicy],['vaporSurface',vaporSurfacePolicy],['vaporCoverage',vaporCoveragePolicy]]) {
    const source=JSON.stringify(policy()),raw=source+' '.repeat(16*1024*1024-source.length);
    assert.throws(()=>build({...scenarioState(),[field]:raw}),/16 MiB/,'Scenario framing also consumes the original-source allowance');
  }
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

const staticStrainMetrics=['min_warp_green_strain','max_warp_green_strain','min_weft_green_strain','max_weft_green_strain',
  'min_engineering_shear_green_strain','max_engineering_shear_green_strain'];
const motionStrainMetrics=staticStrainMetrics.map(metric=>'motion_'+metric);

test('directional strain and complete-motion metrics retain signed goals and independent consumer requirements', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,motion:true,materialStrain:true},metrics=[...staticStrainMetrics,...motionStrainMetrics,'motion_max_absolute_edge_strain_pct'];
  const input=state();delete input.cutting;
  input.objectives=metrics.map((metric,i)=>({metric,direction:i%2?'minimize':'maximize'}));
  input.constraints=[{metric:'min_warp_green_strain',minimum:-0.04,maximum:-0.01},
    {metric:'motion_max_engineering_shear_green_strain',minimum:-0.1,maximum:0.1}];
  input.requiredConsumers=['material_strain','motion','motion_material_strain'];
  const before=JSON.stringify(input),result=JSON.parse(build(input,context));
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  for(const metric of staticStrainMetrics) {
    const staticOnly={...input,objectives:[{metric,direction:'maximize'}],constraints:[],requiredConsumers:['material_strain']};
    assert.equal(JSON.parse(build(staticOnly,{...context,motion:false})).comparison.objectives[0].metric,metric);
    assert.throws(()=>build(staticOnly,{drape:true,surface:false}),/requested material_strain/);
  }
  for(const metric of motionStrainMetrics) {
    const moving={...input,objectives:[{metric,direction:'minimize'}],constraints:[],requiredConsumers:[]};
    for(const missing of [{motion:false},{materialStrain:false}])assert.throws(()=>build(moving,{...context,...missing}),/requested motion_material_strain/);
  }
  const edgeOnly={...input,objectives:[{metric:'motion_max_absolute_edge_strain_pct',direction:'minimize'}],constraints:[],requiredConsumers:['motion']};
  assert.equal(JSON.parse(build(edgeOnly,{...context,materialStrain:false})).comparison.objectives[0].metric,'motion_max_absolute_edge_strain_pct');
  assert.throws(()=>build(edgeOnly,{...context,motion:false}),/requested motion/);
  for(const consumer of ['material_strain','motion','motion_material_strain']) {
    assert.throws(()=>build({...state(),requiredConsumers:[consumer]},{drape:true,surface:false}),/requires requested/);
  }
});

test('directional strain and motion refuse reported ranges while retaining explicit nominal guards', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:true,motion:true,materialStrain:true},input={...state(),basis:'reported_bounds',
    objectives:[{metric:'independent_surface_power_w',direction:'minimize'}],constraints:[],requiredConsumers:['surface']};
  for(const metric of [...staticStrainMetrics,...motionStrainMetrics,'motion_max_absolute_edge_strain_pct']) {
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
    assert.throws(()=>build({...input,constraints:[{metric,minimum:-0.1,maximum:0.1}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
    const guard={metric,basis:'nominal',minimum:-0.1,maximum:0.1};
    assert.deepEqual(JSON.parse(build({...input,constraints:[guard]},context)).comparison.constraints,[guard]);
  }
});

const initialContactMetrics=['body_contact_resultant_n','max_body_contact_vertex_force_n'];
const movingContactMetrics=['motion_peak_body_contact_resultant_n','motion_max_body_contact_vertex_force_n'];

test('body contact objectives and guards require their own initial or complete-motion consumer', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,motion:true,contactLoads:true},input=state();delete input.cutting;
  input.objectives=[...initialContactMetrics,...movingContactMetrics].map((metric,i)=>({metric,direction:i%2?'minimize':'maximize'}));
  input.constraints=[{metric:'body_contact_resultant_n',minimum:0.1,maximum:2},{metric:'motion_max_body_contact_vertex_force_n',maximum:3}];
  input.requiredConsumers=['contact_loads','motion_contact_loads'];
  const before=JSON.stringify(input),raw=build(input,context),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);assert.deepEqual(result.comparison.required_consumers,input.requiredConsumers);
  for(const key of ['contactLoads','contact_loads','motion','scenario'])assert.equal(Object.hasOwn(result,key),false);
  for(const [metrics,consumer] of [[initialContactMetrics,'contact_loads'],[movingContactMetrics,'motion_contact_loads']]) {
    for(const metric of metrics)for(const policy of [{objectives:[{metric,direction:'minimize'}]},
      {constraints:[{metric,maximum:2}]},{requiredConsumers:[consumer]}]) {
      const request={...state(),...policy};
      assert.ok(build(request,context));
      for(const missing of [undefined,{drape:true,surface:false},{...context,contactLoads:false}]) {
        assert.throws(()=>build(request,missing),new RegExp(`requested ${consumer}`));
      }
      if(consumer==='motion_contact_loads')assert.throws(()=>build(request,{...context,motion:false}),/requested motion_contact_loads/);
      else assert.ok(build(request,{...context,motion:false}),'Initial loads need no prescribed motion or material-strain request');
    }
  }
});

test('contact-load criteria remain nominal and compose with independently bounded heat objectives', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:true,motion:true,contactLoads:true},input={...state(),basis:'reported_bounds',
    objectives:[{metric:'independent_surface_power_w',direction:'maximize'}],requiredConsumers:['contact_loads','motion_contact_loads']};
  for(const metric of [...initialContactMetrics,...movingContactMetrics]) {
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
    assert.throws(()=>build({...input,constraints:[{metric,maximum:2}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
    const constraint={metric,basis:'nominal',minimum:0,maximum:2};
    assert.deepEqual(JSON.parse(build({...input,constraints:[constraint]},context)).comparison.constraints,[constraint]);
  }
});

const coverageVaporMetrics=[
  ['body_coverage_power_w','coverage','coverage'],
  ['independent_vapor_potential_heat_flow_w','vapor_surface','vaporSurface'],
  ['independent_vapor_potential_mass_flow_kg_s','vapor_surface','vaporSurface'],
  ['covered_vapor_potential_heat_flow_w','vapor_coverage','vaporCoverage'],
  ['covered_vapor_potential_mass_flow_kg_s','vapor_coverage','vaporCoverage'],
  ['motion_covered_vapor_potential_energy_j','motion_vapor_coverage','motionVaporHistory'],
  ['motion_covered_vapor_potential_mass_kg','motion_vapor_coverage','motionVaporHistory'],
];

test('covered heat and vapor comparisons retain signed directions and source-reported bounds independently', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,coverage:true,vaporSurface:true,vaporCoverage:true,motion:true,motionVaporHistory:true,
    materialStrain:true,contactLoads:true};
  for(const basis of ['nominal','reported_bounds']) {
    const input={...state(),basis,objectives:coverageVaporMetrics.map(([metric],i)=>({metric,direction:i%2?'minimize':'maximize'})),
      constraints:coverageVaporMetrics.map(([metric],i)=>({metric,minimum:i===0?-50:0,maximum:i===0?-1:2})),
      requiredConsumers:['coverage','vapor_surface','vapor_coverage','motion_vapor_coverage']};
    delete input.cutting;
    input.constraints.push({metric:'min_warp_green_strain',basis:'nominal',minimum:-0.04},
      {metric:'body_contact_resultant_n',basis:'nominal',maximum:2});
    const before=JSON.stringify(input),result=JSON.parse(build(input,context));
    assert.equal(JSON.stringify(input),before);assert.deepEqual(result.comparison,{basis,objectives:input.objectives,
      constraints:input.constraints,required_consumers:input.requiredConsumers,allow_mixed_protocols:false});
    for(const key of ['coverage','vapor_surface','vapor_coverage','motion_vapor_coverage','scenario','reported_bounds'])assert.equal(Object.hasOwn(result,key),false);
  }
  for(const [metric,consumer,flag] of coverageVaporMetrics.slice(0,5)) {
    const input={...state(),basis:'reported_bounds',objectives:[{metric,direction:'maximize'}],requiredConsumers:[consumer]};
    assert.ok(build(input,{drape:true,surface:false,[flag]:true}),'Each static consumer remains independently requestable');
    assert.throws(()=>build(input,{drape:true,surface:true}),new RegExp(`requested ${consumer}`));
    assert.throws(()=>build(input),new RegExp(`requested ${consumer}`));
  }
});

test('motion vapor criteria require the actual history policy with motion and covered vapor', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true};
  for(const [metric,consumer] of coverageVaporMetrics.slice(5))for(const policy of [
    {basis:'reported_bounds',objectives:[{metric,direction:'minimize'}]},
    {constraints:[{metric,basis:'reported_bounds',minimum:0,maximum:1}]},{requiredConsumers:[consumer]}]) {
    const input={...state(),...policy};assert.ok(build(input,context));
    for(const key of ['motion','vaporCoverage','motionVaporHistory']) {
      const absent={...context};delete absent[key];
      for(const missing of [absent,{...context,[key]:false}])assert.throws(()=>build(input,missing),/requested motion_vapor_coverage/);
    }
    assert.throws(()=>build(input,{...context,vaporCoverage:false,vaporSurface:true}),/requested motion_vapor_coverage/);
  }
});

test('optional scenario consumers preserve old contexts and reject ambiguous availability flags', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const old={drape:true,surface:false},raw=build(state(),old);
  const keys=['motion','materialStrain','contactLoads','coverage','vaporSurface','vaporCoverage','motionVaporHistory','motionWetFilm',
    'transientSurface','transientCoverage','textileWater'];
  const flags=Object.fromEntries(keys.map(key=>[key,false]));
  assert.equal(build(state(),{...old,...flags}),raw);
  for(const key of keys)for(const value of [null,undefined,'true',1,{}]) {
    assert.throws(()=>build(state(),{...old,[key]:value}),/availability must be a boolean/);
  }
  const inherited=Object.assign(Object.create(Object.fromEntries(keys.map(key=>[key,true]))),old);
  for(const consumer of ['material_strain','motion','contact_loads','coverage','vapor_surface','vapor_coverage','motion_vapor_coverage','motion_wet_film',
    'transient_surface','transient_coverage']) {
    assert.throws(()=>build({...state(),requiredConsumers:[consumer]},inherited),new RegExp(`requested ${consumer}`));
  }
});

test('reported sheet heat retains signed objectives and inclusive guards without inventing source ranges', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const direction of ['minimize','maximize']) {
    const input=state();delete input.cutting;
    input.basis='reported_bounds';input.objectives=[{metric:'independent_surface_power_w',direction}];
    input.constraints=[{metric:'independent_surface_power_w',minimum:-50,maximum:-1}];
    input.requiredConsumers=['drape','surface'];
    const before=JSON.stringify(input),raw=build(input,{drape:true,surface:true}),result=JSON.parse(raw);
    assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(input.candidates[0].selection));
    assert.deepEqual(result.comparison,{basis:'reported_bounds',objectives:input.objectives,
      constraints:input.constraints,required_consumers:input.requiredConsumers,allow_mixed_protocols:false});
    for(const key of ['cutting','assembly_mass','scenario','surface','reported_bounds'])assert.equal(Object.hasOwn(result,key),false);
    assert.throws(()=>build(input,{drape:true,surface:false}),/requested surface/);
    assert.throws(()=>build({...input,cutting:cutting()}),/requested surface/);
  }
});

test('reported sheet heat guards retain their own basis under nominal mechanical and production goals', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:true},input={...state(),
    objectives:[{metric:'stock_area_m2',direction:'minimize'},{metric:'simulated_cloth_mass_kg',direction:'minimize'}],
    constraints:[{metric:'independent_surface_power_w',basis:'reported_bounds',minimum:-12,maximum:-2},
      {metric:'max_absolute_edge_strain_pct',basis:'nominal',maximum:8}],requiredConsumers:['surface']};
  const result=JSON.parse(build(input,context));
  assert.equal(result.comparison.basis,'nominal');assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);
  const sameMetric={...input,objectives:[{metric:'independent_surface_power_w',direction:'maximize'}]};
  assert.deepEqual(JSON.parse(build(sameMetric,context)).comparison.constraints,input.constraints);
  assert.throws(()=>build(input,{drape:true,surface:false}),/requested surface/);
  for(const constraint of [{metric:'independent_surface_power_w',basis:'reported_bounds',minimum:1,maximum:-1},
    {metric:'independent_surface_power_w',basis:'reported_bounds',maximum:Infinity}]) {
    assert.throws(()=>build({...input,constraints:[constraint]},context),/finite ordered inclusive bounds/);
  }
});

test('bounded complete mass composes with steady heat ranges and separately nominal mechanical guards', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),assemblyMass:mass(),basis:'reported_bounds',
    objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:[{metric:'independent_surface_power_w',minimum:-10,maximum:10,basis:'nominal'},
      {metric:'max_absolute_edge_strain_pct',maximum:8,basis:'nominal'}],requiredConsumers:['drape','surface']};
  const context={drape:true,surface:true},result=JSON.parse(build(input,context));
  assert.deepEqual(result.comparison.constraints,input.constraints);
  const constraint={metric:'independent_surface_power_w',minimum:-10,maximum:10};
  assert.deepEqual(JSON.parse(build({...input,constraints:[constraint]},context)).comparison.constraints,[constraint]);
  const objectives=[...input.objectives,{metric:'independent_surface_power_w',direction:'maximize'}];
  assert.deepEqual(JSON.parse(build({...input,objectives},context)).comparison.objectives,objectives);
  for(const metric of ['simulated_cloth_mass_kg','max_absolute_edge_strain_pct','stock_area_m2']) {
    assert.throws(()=>build({...input,objectives:[{metric,direction:'minimize'}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
    assert.throws(()=>build({...input,constraints:[{metric,maximum:10}]},context),/mass, steady heat, vapor potential and final body-water totals only/);
  }
});

test('motion objectives, guards and required results need actual motion and explicit nominal basis', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const metric='motion_max_absolute_edge_strain_pct',context={drape:true,surface:false,motion:true};
  const objective={...state(),objectives:[{metric,direction:'minimize'}]},guard={...state(),constraints:[{metric,maximum:8}]};
  const required={...state(),requiredConsumers:['motion']};
  for(const input of [objective,guard,required]) {
    const raw=build(input,context);assert.equal(Object.hasOwn(JSON.parse(raw),'motion'),false);
    for(const missing of [undefined,{drape:true,surface:false},{...context,motion:false}]) {
      assert.throws(()=>build(input,missing),/requested motion/);
    }
    for(const motion of [null,'true',1,undefined]) assert.throws(()=>build(input,{...context,motion}),/boolean/);
  }
  const bounded={...state(),assemblyMass:mass(),basis:'reported_bounds',objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:[{metric,maximum:8,basis:'nominal'}],requiredConsumers:['motion']};
  assert.deepEqual(JSON.parse(build(bounded,context)).comparison.constraints,bounded.constraints);
  assert.throws(()=>build({...bounded,objectives:objective.objectives},context),/mass, steady heat, vapor potential and final body-water totals only/);
  assert.throws(()=>build({...bounded,constraints:guard.constraints},context),/mass, steady heat, vapor potential and final body-water totals only/);
  const heat={...bounded,objectives:[{metric:'independent_surface_power_w',direction:'maximize'}]};
  assert.deepEqual(JSON.parse(build(heat,{...context,surface:true})).comparison.constraints,bounded.constraints,
    'A source-bounded heat objective composes with a nominal motion-strain guard');
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

test('guided seam adjustments retain canonical fixture identities and ordered operations in every search mode', async () => {
  const {buildMaterialStudyRequest:build,buildGuidedDesignAfterOperations:adjust}=await modulePromise;
  const source=JSON.parse(fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/synthetic-sewing-pattern.json'),'utf8'));
  const pieceIds=source.pieces.map(piece=>piece.id),seamIds=source.seams.map(seam=>seam.id);
  const after=[{op:'true_seam',seam:'side',end:'start'},
    {op:'cap_ease',piece:'front',seams:['side'],target_ratio:1.1},{op:'true_seam',seam:'side',end:'end'}];
  const before=JSON.stringify(after),copy=adjust(after,pieceIds,seamIds);
  assert.deepEqual(copy,after);assert.notEqual(copy,after);
  copy[0].end='end';copy[1].seams[0]='changed';assert.equal(JSON.stringify(after),before);
  const legacy=linkedDesignInput({pieceIds:['front'],values:[0,10],after});
  const grid=multiDesignInput({variables:[{key:'length',values:[0,10],sites:[
    {piece:'front',axis:'y',at_mm:50,multiplier:1}]}],after});
  const adaptive=adaptiveDesignInput({variables:[{key:'length',boundsMm:[0,20],initialMm:0,
    initialStepMm:10,minimumStepMm:1,sites:grid.variables[0].sites}],after});
  for(const design of [legacy,grid,adaptive]) {
    const input={...state(),pieceIds,seamIds,target:{scope:'pieces',ids:['back']},design},frozen=JSON.stringify(input);
    const raw=build(input),result=JSON.parse(raw),search=result.design_adaptive??result.design_grid;
    assert.equal(JSON.stringify(input),frozen);assert.deepEqual(search.after,after);
    assert.equal(Object.hasOwn(result,'seamIds'),false);assert.equal(Object.hasOwn(search,'seamIds'),false);
    assert.ok(raw.includes(design.fitSpec));assert.ok(raw.includes(input.candidates[0].selection));
    assert.deepEqual(Object.keys(result.variants[1].changes.pieces),['back'],'Seam adjustments do not change independent material targets');
  }
});

test('seam adjustment authoring rejects ambiguous sources and malformed explicit operations', async () => {
  const {buildGuidedDesignAfterOperations:build}=await modulePromise;
  const pieces=['front','lining'],seams=['side','cap'],truing={op:'true_seam',seam:'side',end:'end'},
    cap={op:'cap_ease',piece:'front',seams:['side','cap'],target_ratio:1.1};
  for(const after of [null,{},'[]',[null],Array(1),Array(17).fill(truing)]) assert.throws(()=>build(after,pieces,seams));
  for(const operation of [{},{op:'move',piece:'front',vertex:'a',dx_mm:1,dy_mm:0},
    {...truing,extra:1},{...cap,extra:1}]) assert.throws(()=>build([operation],pieces,seams));
  for(const end of [undefined,null,'START',0,'both']) assert.throws(()=>build([{...truing,end}],pieces,seams),/start or end/);
  for(const seam of ['','missing',null,1]) assert.throws(()=>build([{...truing,seam}],pieces,seams));
  for(const sourceIds of [undefined,null,{},[],['side','side']]) assert.throws(()=>build([truing],pieces,sourceIds));
  for(const piece of ['missing','',null,1]) assert.throws(()=>build([{...cap,piece}],pieces,seams));
  for(const selected of [[],null,{},['missing'],['side','side'],['side',undefined]]) {
    assert.throws(()=>build([{...cap,seams:selected}],pieces,seams));
  }
  for(const target_ratio of [0,-0,-1,NaN,Infinity,-Infinity,'1',null,undefined]) {
    assert.throws(()=>build([{...cap,target_ratio}],pieces,seams),/finite and positive/);
  }
  for(const target_ratio of [0.1,4,Number.MIN_VALUE,Number.MAX_VALUE]) {
    assert.equal(build([{...cap,target_ratio}],pieces,seams)[0].target_ratio,target_ratio,
      'The native 0.3–3 cap-height search interval is not a target-ratio limit; root refusal remains native');
  }
  assert.deepEqual(build([truing],pieces,['side','other','other',undefined,null]),[truing],
    'Only the selected source identity must be unique here; Rust still validates the complete pattern');
  const inherited=Object.create({end:'end'});Object.assign(inherited,{op:'true_seam',seam:'side'});
  assert.throws(()=>build([inherited],pieces,seams),/missing/);
});

test('all linked sites and seam adjustments share the native sixteen-operation allowance before source reads', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const operation={op:'true_seam',seam:'side',end:'end'};
  for(const design of [linkedDesignInput(),multiDesignInput(),adaptiveDesignInput()]) {
    const sites=design.variables?.reduce((sum,variable)=>sum+variable.sites.length,0)??design.pieceIds.length;
    const after=Array.from({length:16-sites},()=>({...operation}));
    const result=JSON.parse(build({...state(),seamIds:['side'],design:{...design,after}}));
    assert.equal((result.design_grid??result.design_adaptive).after.length,16-sites);
    let reads=0;const candidates=[{key:'alternative',get selection(){reads++;throw Error('Unread material snapshot');}}];
    assert.throws(()=>build({...state(),candidates,seamIds:['side'],design:{...design,after:[...after,operation]}}),/16 ordered alteration operations/);
    assert.equal(reads,0);
  }
});

test('omitted seam adjustments preserve old request bytes and explicit empty lists retain authored intent', async () => {
  const {buildMaterialStudyRequest:build,buildGuidedDesignAfterOperations:adjust}=await modulePromise;
  assert.deepEqual(adjust([]),[]);
  for(const design of [linkedDesignInput(),multiDesignInput(),adaptiveDesignInput()]) {
    const input={...state(),design},before=build(input);
    assert.equal(build({...input,seamIds:['side','side']}),before,'Unused source identity context does not enter the request');
    const empty=build({...input,design:{...design,after:[]}}),result=JSON.parse(empty);
    assert.deepEqual((result.design_grid??result.design_adaptive).after,[]);
    assert.equal(empty.replace(',"after":[]',''),before);
  }
});

test('seam adjustment names share the complete search request byte budget without rewriting source identities', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const seam='é'.repeat(128*1024),after=[{op:'true_seam',seam,end:'start'}];
  for(const design of [linkedDesignInput(),adaptiveDesignInput()]) {
    let reads=0;const candidates=[{key:'alternative',get selection(){reads++;throw Error('Unread material snapshot');}}];
    assert.throws(()=>build({...state(),candidates,seamIds:[seam],design:{...design,after}}),/exceeds 256 KiB/);
    assert.equal(reads,0);
  }
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

const adaptiveDesignInput = (changes = {}) => ({mode:'adaptive',objective:'fit_band_violation_mm',
  variables:multiDesignInput().variables.map((v,i)=>({key:v.key,boundsMm:[-10,40],initialMm:i,
    initialStepMm:10,minimumStepMm:0.001,sites:v.sites})),maxEvaluations:96,maxDesigns:2,
  fitSpec:linkedFitSource(),...changes});

test('adaptive additional starts retain complete authored coordinates without increasing trial reservations', async () => {
  const {buildMaterialStudyRequest:build,buildGuidedDesignRestartPoints:points}=await modulePromise;
  const design=adaptiveDesignInput({restartPointsMm:[
    {'local shape':-2,length:20,ease:10},{ease:0,length:0,'local shape':0}]});
  const before=JSON.stringify(design),result=JSON.parse(build({...state(),design}));
  assert.equal(JSON.stringify(design),before);
  assert.deepEqual(result.design_adaptive.restart_points_mm,design.restartPointsMm);
  assert.equal(result.design_adaptive.max_evaluations,96);
  const reversed={...design,variables:[...design.variables].reverse()};
  assert.deepEqual(JSON.parse(build({...state(),design:reversed})).design_adaptive.restart_points_mm,design.restartPointsMm);
  for(const starts of [undefined,[]]) {
    const input=adaptiveDesignInput();if(starts)input.restartPointsMm=starts;
    assert.equal(Object.hasOwn(JSON.parse(build({...state(),design:input})).design_adaptive,'restart_points_mm'),false);
  }
  const maximum=Array.from({length:31},(_,i)=>({ease:i,length:0,'local shape':0}));
  assert.equal(points(maximum,design.variables).length,31);
  const single=adaptiveDesignInput({objective:'consumer_pareto',materialKeys:['baseline','alternative'],caseKey:'cold',
    maxEvaluations:3,maxDesigns:1,restartPointsMm:design.restartPointsMm});
  const matrix=JSON.parse(build({...state(),design:single},namedContext(undefined,20)));
  assert.deepEqual(matrix.design_adaptive.restart_points_mm,design.restartPointsMm);
});

test('adaptive additional starts refuse incomplete stale repeated and out-of-domain coordinates', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const valid={ease:3,length:4,'local shape':5},initial={ease:-0,length:1,'local shape':2};
  for(const starts of [null,{},[{}],[{ease:3,length:4}],[{...valid,obsolete:1}],
    [{...valid,ease:41}],[{...valid,ease:NaN}],[{...valid,ease:Infinity}],
    [{...valid,ease:'3'}],[initial],[valid,{...valid}],[{...valid,ease:0},{...valid,ease:-0}],
    Array.from({length:32},(_,i)=>({...valid,ease:i}))]) {
    assert.throws(()=>build({...state(),design:adaptiveDesignInput({restartPointsMm:starts})}));
  }
  const design=multiDesignInput({restartPointsMm:[valid]});
  assert.throws(()=>build({...state(),design}),/unsupported/,'Grid mode must not consume adaptive starts');
  const variable={key:'tiny',boundsMm:[0,1],initialMm:1,initialStepMm:1,minimumStepMm:0.1,
    sites:[{piece:'front',axis:'x',at_mm:1,multiplier:Number.MIN_VALUE}]};
  assert.throws(()=>build({...state(),design:adaptiveDesignInput({variables:[variable],
    restartPointsMm:[{tiny:0.25}]})}),/underflows/,'Actual restart displacements must be representable');
});

test('adaptive authoring preserves ordered mixed sites, original fit bytes and native discovery controls', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),target:{scope:'pieces',ids:['lining']},
    cutting:' {"copies":1,"gap_mm":2.0,"edge_margin_mm":-0.0,"end_margin_mm":0.0}\r\n',design:adaptiveDesignInput()};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.equal(result.schema,'ftx-design-study/1');
  assert.deepEqual(result.design_adaptive,{objective:'fit_band_violation_mm',
    variables:input.design.variables.map(v=>({key:v.key,bounds_mm:v.boundsMm,initial_mm:v.initialMm,
      initial_step_mm:v.initialStepMm,minimum_step_mm:v.minimumStepMm,sites:v.sites})),max_evaluations:96,max_designs:2});
  for(const source of [input.design.fitSpec,input.cutting,input.candidates[0].selection])assert.ok(raw.includes(source));
  assert.deepEqual(result.design_fit,{fit_spec:JSON.parse(input.design.fitSpec)});
  assert.deepEqual(Object.keys(result.variants[1].changes.pieces),['lining']);
  for(const key of ['design_grid','mode','material_key','case_key','restart_points_mm'])assert.equal(Object.hasOwn(result.design_adaptive,key),false);
  assert.equal(Object.hasOwn(result,'design_grid'),false);
});

test('adaptive domains reject nonfinite spans, out-of-domain starts, invalid steps and lossy displacements', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=adaptiveDesignInput(),first=design.variables[0];
  const check=changes=>build({...state(),design:{...design,variables:[{...first,...changes}]}});
  for(const boundsMm of [[],[0],[0,1,2],[0,0],[2,1],[0,,1],[,1],[NaN,1],['0',1],[-Number.MAX_VALUE,Number.MAX_VALUE]]) {
    assert.throws(()=>check({boundsMm}));
  }
  for(const initialMm of [-11,41,NaN,Infinity,'0',null])assert.throws(()=>check({initialMm}),/in-domain/);
  for(const initialStepMm of [0,-1,51,NaN,Infinity,'10',null])assert.throws(()=>check({initialStepMm}),/step/);
  for(const minimumStepMm of [0,-1,11,NaN,Infinity,'1',null])assert.throws(()=>check({minimumStepMm}),/step/);
  assert.equal(JSON.parse(check({initialMm:-10,initialStepMm:50,minimumStepMm:50})).design_adaptive.variables[0].initial_mm,-10);
  assert.equal(JSON.parse(check({initialMm:40})).design_adaptive.variables[0].initial_mm,40);
  const move={op:'move',piece:'front',vertex:'d',dx_multiplier:0,dy_multiplier:2};
  assert.throws(()=>check({boundsMm:[0,Number.MAX_VALUE],initialMm:0,sites:[move]}),/overflows/);
  assert.throws(()=>check({boundsMm:[0,Number.MIN_VALUE*2],initialMm:Number.MIN_VALUE,
    initialStepMm:Number.MIN_VALUE,minimumStepMm:Number.MIN_VALUE,sites:[{...move,dy_multiplier:0.5}]}),/underflows/);
  for(const maxEvaluations of [0,257,1.5,NaN,Infinity,'1',null]) {
    assert.throws(()=>build({...state(),design:{...design,maxEvaluations}}),/evaluation allowance.*integer/);
  }
  for(const maxEvaluations of [1,256])assert.equal(JSON.parse(build({...state(),design:{...design,maxEvaluations}})).design_adaptive.max_evaluations,maxEvaluations);
});

test('grid and adaptive inputs never mix policies and explicit grid mode preserves existing requests', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const grid=multiDesignInput(),adaptive=adaptiveDesignInput(),check=design=>build({...state(),design});
  assert.equal(check({...grid,mode:'grid'}),check(grid));
  for(const mode of ['',null,undefined,'adaptive_grid'])assert.throws(()=>check({...grid,mode}),/grid or adaptive/);
  for(const extra of [{maxCandidates:2},{values:[0,1]},{caseKey:'winter'},
    {after:null},{maxEvaluations:undefined}])assert.throws(()=>check({...adaptive,...extra}));
  for(const extra of [{objective:'fit_band_violation_mm'},{materialKey:'baseline'},{maxEvaluations:8},{restartPointsMm:[]}]) {
    assert.throws(()=>check({...grid,...extra}),/unsupported/);
  }
  assert.throws(()=>check({...adaptive,variables:[{...adaptive.variables[0],values:[0,1]}]}),/unsupported/);
  assert.throws(()=>check({...grid,variables:[{...grid.variables[0],boundsMm:[0,1]}]}),/unsupported/);
  for(const key of ['objective','maxEvaluations','fitSpec','maxDesigns']) {
    const missing={...adaptive};delete missing[key];assert.throws(()=>check(missing),/missing/);
  }
  for(const key of ['boundsMm','initialMm','initialStepMm','minimumStepMm']) {
    const variable={...adaptive.variables[0]};delete variable[key];assert.throws(()=>check({...adaptive,variables:[variable]}),/missing/);
  }
  assert.throws(()=>check({...linkedDesignInput(),mode:'adaptive'}),/unsupported/);
});

test('adaptive consumer guidance nominates an exact material and retains ordinary scalar or Pareto comparisons', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=adaptiveDesignInput({objective:'consumer_comparison',materialKey:'alternative'}),input={...state(),design};
  for(const materialKey of ['baseline','alternative']) {
    const result=JSON.parse(build({...input,design:{...design,materialKey}}));
    assert.equal(result.design_adaptive.material_key,materialKey);
    assert.deepEqual(result.comparison.objectives,input.objectives);
  }
  for(const materialKey of ['removed choice','',null,undefined,1])assert.throws(()=>build({...input,design:{...design,materialKey}}),/materialKey/);
  const fitOnly=adaptiveDesignInput();
  for(const materialKey of ['baseline',null,undefined])assert.throws(()=>build({...state(),design:{...fitOnly,materialKey}}),/does not accept/);
  for(const objective of ['unknown','consumer_worst_case','consumer_matrix_pareto',null]) {
    assert.throws(()=>build({...input,design:{...design,objective}}),/guidance/);
  }
  const objectives=[...input.objectives,{metric:'stock_mass_kg',direction:'minimize'}];
  assert.throws(()=>build({...input,objectives}),/exactly one/);
  assert.throws(()=>build({...input,objectives:[]}),/exactly one/);
  assert.throws(()=>build({...input,design:{...design,objective:'consumer_pareto'}}),/2–16/);
  assert.throws(()=>build({...input,objectives:Array(17).fill(input.objectives[0]),design:{...design,objective:'consumer_pareto'}}),/2–16/);
  const pareto={...input,objectives,requiredConsumers:['cutting'],constraints:[{metric:'stock_mass_kg',maximum:0.1}],
    design:{...design,objective:'consumer_pareto'}};
  const result=JSON.parse(build(pareto));assert.equal(result.design_adaptive.objective,'consumer_pareto');
  assert.deepEqual(result.comparison,{basis:'nominal',objectives,constraints:pareto.constraints,
    required_consumers:['cutting'],allow_mixed_protocols:false});
  assert.throws(()=>build({...pareto,objectives:[...objectives,{metric:'assembly_mass_kg',direction:'minimize'}]}),/assembly_mass/);
  assert.equal(JSON.parse(build({...pareto,design:fitOnly})).comparison.objectives.length,2,'Fit discovery keeps independent downstream objectives');
});

test('multi-material adaptive authoring preserves explicit order and exact sources with one or several goals', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),design:adaptiveDesignInput({objective:'consumer_pareto',materialKeys:['alternative','baseline']})};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  assert.deepEqual(result.design_adaptive.material_keys,['alternative','baseline']);
  assert.equal(Object.hasOwn(result.design_adaptive,'material_key'),false);
  assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.ok(raw.includes(input.candidates[0].selection));assert.ok(raw.includes(input.design.fitSpec));
  assert.equal(result.design_adaptive.max_evaluations,96,'The allowance still counts unique geometries');
  const several={...input,objectives:[...input.objectives,{metric:'stock_mass_kg',direction:'minimize'}]};
  assert.equal(JSON.parse(build(several)).comparison.objectives.length,2);
  assert.throws(()=>build({...input,objectives:[]}),/1–16/);
  assert.throws(()=>build({...input,objectives:Array(17).fill(input.objectives[0])}),/1–16/);
});

test('multi-material adaptive authoring refuses ambiguous stale or unsupported nominations', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=adaptiveDesignInput({objective:'consumer_pareto',materialKeys:['baseline','alternative']});
  for(const materialKeys of [[],['baseline'],['baseline','baseline'],['baseline','removed'],['baseline',''],
    null,'baseline',Array.from({length:17},(_,i)=>'choice-'+i)]) {
    assert.throws(()=>build({...state(),design:{...design,materialKeys}}),/material|Material|names/);
  }
  for(const materialKey of ['baseline',null,undefined])assert.throws(()=>build({...state(),design:{...design,materialKey}}),/cannot also/);
  for(const objective of ['fit_band_violation_mm','consumer_comparison','consumer_worst_case']) {
    assert.throws(()=>build({...state(),design:{...design,objective}}),/does not accept|require Pareto/);
  }
  const legacy={...design,materialKey:'baseline'};delete legacy.materialKeys;
  assert.throws(()=>build({...state(),design:legacy}),/2–16/,'One nominated material retains its existing Pareto contract');
});

test('multi-material adaptive matrices reserve every material and case probe before source reads', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=adaptiveDesignInput({objective:'consumer_pareto',materialKeys:['alternative','baseline'],caseKey:'cold',
    maxEvaluations:3,maxDesigns:1}),input={...state(),design};
  // (three geometries * two guidance materials + two retained designs * two materials) * two cases.
  const context=namedContext(undefined,20),result=JSON.parse(build(input,context));
  assert.deepEqual(result.design_adaptive.material_keys,['alternative','baseline']);
  assert.equal(result.design_adaptive.case_key,'cold');
  let reads=0;const candidates=[{key:'alternative',get selection(){reads++;throw Error('Source should not be read');}}];
  assert.throws(()=>build({...input,candidates},namedContext(undefined,19)),/20 reserved case evaluations/);
  assert.equal(reads,0);
  const allCases={...design,objective:'consumer_matrix_pareto'};delete allCases.caseKey;
  const all=JSON.parse(build({...input,design:allCases},context));
  assert.equal(all.design_adaptive.objective,'consumer_matrix_pareto');
  assert.equal(Object.hasOwn(all.design_adaptive,'case_key'),false);
  assert.equal(JSON.parse(build({...input,design:allCases},namedContext([{key:'one',surface:false}],10)))
    .design_adaptive.objective,'consumer_matrix_pareto','Each nominated material may have a single case/objective coordinate');
  assert.throws(()=>build({...input,design:{...allCases,caseKey:'cold'}},context),/does not accept a caseKey/);
});

test('adaptive retention reserves all declared output slots before snapshots and preserves independent probe allowance', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  let reads=0;
  const candidates=Array.from({length:7},(_,i)=>({key:'choice-'+i,get selection(){reads++;throw Error('Unread snapshot');}}));
  const design=adaptiveDesignInput({maxDesigns:4,maxEvaluations:1});
  assert.throws(()=>build({...state(),candidates,design}),/32 expanded/);assert.equal(reads,0);
  assert.throws(()=>build({...state(),design:{...design,maxDesigns:16}}),/32 expanded/);
  const real=candidates.map((c,i)=>({key:c.key,selection:selection('cloth-'+i,'abcdef0'[i])}));
  const result=JSON.parse(build({...state(),candidates:real,design:{...design,maxDesigns:3,maxEvaluations:256,
    objective:'consumer_comparison',materialKey:'choice-6'}}));
  assert.equal(result.design_adaptive.max_designs,3);assert.equal(result.design_adaptive.max_evaluations,256);
  assert.equal(result.variants.length,8,'Probe allowance does not consume the32 retained design/material rows');
  const long='v'.repeat(256*1024),variable={...design.variables[0],sites:[
    {op:'move',piece:'front',vertex:long,dx_multiplier:1,dy_multiplier:0}]};
  assert.throws(()=>build({...state(),design:{...design,variables:[variable]}}),/adaptive exceeds 256 KiB/);
  for(const fitSpec of ['\ufeff'+linkedFitSource(),{},'{}',' '.repeat(256*1024)+linkedFitSource()]) {
    assert.throws(()=>build({...state(),design:{...design,fitSpec}}));
  }
});

test('adaptive guidance composes with finite jobs, source-reported mass and separately declared scenarios', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const mode of ['cutting','finiteStock','cuttingJob']) {
    const input=mode==='cutting'?state():stockState(mode),context={drape:true,surface:true};
    input.objectives.push({metric:'independent_surface_power_w',direction:'maximize'});
    input.constraints=[{metric:'max_absolute_edge_strain_pct',maximum:8,basis:'nominal'}];input.requiredConsumers=['drape','surface'];
    const fixed=build(input,context),design=adaptiveDesignInput({objective:'consumer_pareto',materialKey:'baseline'});
    const raw=build({...input,design},context),result=JSON.parse(raw);
    assert.equal(result.design_adaptive.material_key,'baseline');
    if(typeof input[mode]==='string')assert.ok(raw.includes(input[mode]));
    delete result.design_adaptive;delete result.design_fit;result.schema='ftx-material-study/1';
    assert.deepEqual(result,JSON.parse(fixed));assert.throws(()=>build({...input,design}),/requested surface/);
  }
  const input={...state(),assemblyMass:mass(),basis:'reported_bounds',objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:[{metric:'stock_area_m2',maximum:0.25,basis:'nominal'}],requiredConsumers:['assembly_mass','cutting'],
    design:adaptiveDesignInput({objective:'consumer_comparison',materialKey:'alternative'})};
  assert.equal(JSON.parse(build(input)).comparison.basis,'reported_bounds');
  assert.throws(()=>build({...input,assemblyMass:mass(false)}),/complete inventory/);
  assert.throws(()=>build({...input,constraints:[{metric:'stock_area_m2',maximum:0.25}]}),/mass, steady heat, vapor potential and final body-water totals only/);
});

const casePolicy = (metric='stock_area_m2',direction='minimize',changes={}) =>
  ({basis:'nominal',objectives:[{metric,direction}],...changes});
const namedContext = (cases=[{key:'cold',surface:false},{key:'warm',surface:false}],maxEvaluations=64) =>
  ({drape:true,surface:cases.some(item=>item.surface),motion:cases.some(item=>item.motion===true),
    materialStrain:cases.some(item=>item.materialStrain===true),contactLoads:cases.some(item=>item.contactLoads===true),
    coverage:cases.some(item=>item.coverage===true),vaporSurface:cases.some(item=>item.vaporSurface===true),
    vaporCoverage:cases.some(item=>item.vaporCoverage===true),motionVaporHistory:cases.some(item=>item.motionVaporHistory===true),
    motionWetFilm:cases.some(item=>item.motionWetFilm===true),
    transientSurface:cases.some(item=>item.transientSurface===true),transientCoverage:cases.some(item=>item.transientCoverage===true),
    textileWater:cases.some(item=>item.textileWater===true),
    textileWaterMechanics:cases.some(item=>item.textileWaterMechanics===true),
    matrix:{maxEvaluations,cases}});
const namedDesign = changes => adaptiveDesignInput({objective:'consumer_comparison',materialKey:'alternative',
  caseKey:'cold',maxEvaluations:2,maxDesigns:1,...changes});

test('guided named-case discovery uses the nominated effective policy without changing the original matrix', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const override=casePolicy('independent_surface_power_w','maximize',{
    constraints:[{metric:'max_absolute_edge_strain_pct',minimum:-0,maximum:8}],
    required_consumers:['drape','surface'],allow_mixed_protocols:true});
  const context=namedContext([{key:'cold',surface:false},{key:'warm',surface:true,comparison:override}]);
  const input={...state(),design:namedDesign({caseKey:'warm'})},original=JSON.stringify(context),before=JSON.stringify(input);
  const raw=build(input,context),result=JSON.parse(raw);
  assert.equal(JSON.stringify(context),original);assert.equal(JSON.stringify(input),before);
  assert.deepEqual(result.design_adaptive.case_key,'warm');assert.equal(result.design_adaptive.material_key,'alternative');
  assert.deepEqual(result.comparison.objectives,input.objectives,'The authored fallback is not replaced by the nominated override');
  assert.equal(Object.hasOwn(result,'matrix'),false);assert.equal(Object.hasOwn(result,'scenario'),false);
  assert.ok(raw.includes(input.design.fitSpec));assert.ok(raw.includes(input.candidates[0].selection));
  const fallback=JSON.parse(build({...input,design:namedDesign()},context));
  assert.equal(fallback.design_adaptive.case_key,'cold');
  const pareto=casePolicy('stock_area_m2','minimize',{objectives:[
    {metric:'stock_area_m2',direction:'minimize'},{metric:'stock_mass_kg',direction:'minimize'}]});
  const two=namedContext([{key:'cold',surface:false,comparison:pareto},{key:'warm',surface:false}]);
  assert.equal(JSON.parse(build({...input,design:namedDesign({objective:'consumer_pareto'})},two)).design_adaptive.objective,'consumer_pareto');
  assert.throws(()=>build({...input,design:namedDesign()},two),/exactly one/);
  assert.throws(()=>build({...input,design:namedDesign({objective:'consumer_pareto',caseKey:'warm'})},two),/2–16/);
});

test('guided matrix context bounds original case identities and refuses stale or ambiguous nominations', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),design:namedDesign()};
  for(const caseKey of ['',null,undefined,'gone'])assert.throws(()=>build({...input,design:namedDesign({caseKey})},namedContext()),/caseKey/);
  const missing={...input.design};delete missing.caseKey;
  assert.throws(()=>build({...input,design:missing},namedContext()),/caseKey/);
  for(const context of [undefined,{drape:true,surface:false}])assert.throws(()=>build(input,context),/requires a scenario matrix/);
  for(const cases of [[],Array.from({length:9},(_,i)=>({key:String(i),surface:false})),
    [{key:'same',surface:false},{key:'same',surface:false}],[{key:' ',surface:false}],
    [{key:'é'.repeat(65),surface:false}],[{key:'cold',surface:1}],[{key:'cold',surface:false,unknown:true}]]) {
    assert.throws(()=>build(input,namedContext(cases)));
  }
  const key='é'.repeat(64),context=namedContext([{key,surface:false}]);
  assert.equal(JSON.parse(build({...input,design:namedDesign({caseKey:key})},context)).design_adaptive.case_key,key);
  for(const maxEvaluations of [0,1,65,NaN,1.5,'64',null])assert.throws(()=>build(input,namedContext(undefined,maxEvaluations)),/allowance/);
  assert.throws(()=>build(input,{...namedContext(),surface:true}),/actual cases/);
  assert.throws(()=>build(input,{...namedContext(),matrix:null}),/must be an object/);
  const renamed=namedContext([{key:'replacement',surface:false},{key:'warm',surface:false}]);
  assert.throws(()=>build(input,renamed),/existing named scenario/);
});

test('all-case worst guidance retains independent guards while requiring an identical objective and basis', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=namedDesign({objective:'consumer_worst_case'});delete design.caseKey;
  const input={...state(),design},context=namedContext([
    {key:'cold',surface:false,comparison:casePolicy('stock_area_m2','minimize',{constraints:[{metric:'stock_mass_kg',maximum:0.2}]})},
    {key:'warm',surface:false,comparison:casePolicy('stock_area_m2','minimize',{required_consumers:['cutting']})}]);
  const before=JSON.stringify(context),result=JSON.parse(build(input,context));
  assert.equal(result.design_adaptive.objective,'consumer_worst_case');assert.equal(Object.hasOwn(result.design_adaptive,'case_key'),false);
  assert.equal(JSON.stringify(context),before);
  assert.equal(JSON.parse(build(input,namedContext())).design_adaptive.objective,'consumer_worst_case');
  assert.equal(JSON.parse(build(input,namedContext([{key:'cold',surface:false,comparison:null},{key:'warm',surface:false}]))).design_adaptive.objective,'consumer_worst_case');
  for(const comparison of [casePolicy('stock_mass_kg'),casePolicy('stock_area_m2','maximize'),
    casePolicy('stock_area_m2','minimize',{basis:'reported_bounds'}),
    casePolicy('stock_area_m2','minimize',{objectives:[...input.objectives,{metric:'stock_mass_kg',direction:'minimize'}]})]) {
    assert.throws(()=>build(input,namedContext([{key:'cold',surface:false},{key:'warm',surface:false,comparison}])),/identical metric/);
  }
  assert.throws(()=>build({...input,design:{...design,caseKey:'cold'}},context),/does not accept a caseKey/);
  assert.throws(()=>build(input),/requires a scenario matrix/);
});

test('all-case Pareto guidance preserves original advanced policies and independent case directions', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const design=namedDesign({objective:'consumer_matrix_pareto'});delete design.caseKey;
  const input={...state(),design},context=namedContext([
    {key:'cold',surface:false,comparison:casePolicy('transient_coverage_inside_energy_j','minimize',{
      basis:'reported_bounds',constraints:[{metric:'motion_max_absolute_edge_strain_pct',basis:'nominal',maximum:8}],
      required_consumers:['motion','transient_coverage'],allow_mixed_protocols:true})},
    {key:'warm',surface:false,comparison:casePolicy('transient_coverage_inside_energy_j','maximize')},
  ]);
  const before=JSON.stringify(context),result=JSON.parse(build(input,context));
  assert.equal(result.design_adaptive.objective,'consumer_matrix_pareto');assert.equal(Object.hasOwn(result.design_adaptive,'case_key'),false);
  assert.equal(JSON.stringify(context),before,'Raw engine policies are read only, never reduced to guided metrics');
  assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.throws(()=>build(input,namedContext([{key:'cold',surface:false}])),/at least two/);
  const two=casePolicy('stock_area_m2','minimize',{objectives:[...input.objectives,{metric:'stock_mass_kg',direction:'minimize'}]});
  assert.equal(JSON.parse(build(input,namedContext([{key:'cold',surface:false,comparison:two}]))).design_adaptive.objective,'consumer_matrix_pareto');
  assert.throws(()=>build({...input,design:{...design,caseKey:'cold'}},context),/does not accept a caseKey/);
  assert.throws(()=>build(input),/requires a scenario matrix/);
});

test('matrix guidance refuses malformed objective coordinates without rewriting other original policy fields', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),design:namedDesign()};
  for(const comparison of [false,[],{},casePolicy('stock_area_m2','minimize',{basis:null}),
    casePolicy('stock_area_m2','minimize',{objectives:[]}),casePolicy('stock_area_m2','minimize',{objectives:Array(17).fill(input.objectives[0])}),
    casePolicy('stock_area_m2','minimize',{objectives:[null]}),casePolicy('','minimize'),casePolicy('stock_area_m2','automatic')]) {
    assert.throws(()=>build(input,namedContext([{key:'cold',surface:false,comparison}])),/declared basis/);
  }
  const fit=adaptiveDesignInput({maxDesigns:1,maxEvaluations:256});
  assert.equal(JSON.parse(build({...state(),design:fit},namedContext())).design_adaptive.objective,'fit_band_violation_mm');
  for(const extra of [{caseKey:'cold'},{caseKey:null},{materialKey:'baseline'}]) {
    assert.throws(()=>build({...state(),design:{...fit,...extra}},namedContext()),/does not accept/);
  }
});

test('matrix reservation covers every probe, retained design and baseline before material snapshots are read', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),design:namedDesign()},context=namedContext(undefined,12);
  assert.equal(JSON.parse(build(input,context)).design_adaptive.max_evaluations,2,'Two probes plus four final rows, each in two cases');
  assert.throws(()=>build(input,namedContext(undefined,11)),/12 reserved case evaluations/);
  let reads=0;
  const candidates=[{key:'alternative',get selection(){reads++;throw Error('Snapshot must remain unread');}}];
  assert.throws(()=>build({...input,candidates},namedContext(undefined,11)),/reserved case evaluations/);assert.equal(reads,0);
  assert.equal(JSON.parse(build(state(),namedContext(undefined,4))).variants.length,2);
  assert.throws(()=>build(state(),namedContext(undefined,3)),/4 reserved case evaluations/);
  const fit={...state(),design:adaptiveDesignInput({maxDesigns:2,maxEvaluations:256})};
  assert.equal(JSON.parse(build(fit,namedContext(undefined,12))).design_adaptive.max_evaluations,256,'Fit-only probes do not invoke scenario callbacks');
  assert.throws(()=>build(fit,namedContext(undefined,11)),/12 reserved case evaluations/);
  const grid={...state(),design:linkedDesignInput({maxDesigns:31,values:[0]})};
  assert.equal(JSON.parse(build(grid,namedContext(undefined,8))).design_grid.max_designs,31,'A smaller grid domain retains its original authored cap');
  assert.throws(()=>build(grid,namedContext(undefined,7)),/8 reserved case evaluations/);
  const exact={...state(),design:namedDesign({maxDesigns:3,maxEvaluations:24})};
  assert.equal(JSON.parse(build(exact,namedContext())).design_adaptive.max_evaluations,24);
  assert.throws(()=>build({...exact,design:{...exact.design,maxEvaluations:25}},namedContext()),/66 reserved case evaluations/);
});

test('each inherited matrix heat policy needs its own surface while raw overrides retain native admission', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),objectives:[{metric:'independent_surface_power_w',direction:'minimize'}],design:namedDesign()};
  const context=namedContext([{key:'cold',surface:true},{key:'warm',surface:false}]);
  assert.throws(()=>build(input,context),/warm inherits.*surface/);
  const override={...context,matrix:{...context.matrix,cases:[context.matrix.cases[0],
    {...context.matrix.cases[1],comparison:casePolicy('stock_area_m2')}]}};
  assert.equal(JSON.parse(build(input,override)).design_adaptive.case_key,'cold');
  const guard={...state(),constraints:[{metric:'independent_surface_power_w',maximum:1}],design:namedDesign()};
  assert.throws(()=>build(guard,context),/warm inherits.*surface/);
  assert.throws(()=>build({...state(),requiredConsumers:['surface'],design:namedDesign()},context),/warm inherits.*surface/);
  const raw=namedContext([{key:'cold',surface:true,comparison:casePolicy('body_coverage_power_w','minimize',{
    required_consumers:['coverage']})},{key:'warm',surface:false,comparison:casePolicy('stock_area_m2')}]);
  assert.equal(JSON.parse(build(input,raw)).design_adaptive.case_key,'cold','Original policy consumers go to Rust instead of being silently omitted');
});

test('motion and strain matrix comparisons require each inherited consumer in the same actual case', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const contextFor=cases=>({...namedContext(cases),motion:cases.some(item=>item.motion===true),
    materialStrain:cases.some(item=>item.materialStrain===true)});
  const complete={key:'cold',surface:false,motion:true,materialStrain:true};
  for(const [metric,consumer,missing] of [
    ['max_warp_green_strain','material_strain',{motion:true}],
    ['motion_max_absolute_edge_strain_pct','motion',{materialStrain:true}],
    ['motion_min_engineering_shear_green_strain','motion_material_strain',{motion:true}],
  ]) {
    const cases=[complete,{key:'warm',surface:false,...missing}],context=contextFor(cases);
    for(const policy of [{constraints:[{metric,minimum:-0.1,maximum:0.1}]},{requiredConsumers:[consumer]},
      {objectives:[{metric,direction:'maximize'}]}]) {
      const input={...state(),...policy,design:namedDesign()};
      assert.throws(()=>build(input,context),new RegExp(`warm inherits.*${consumer}`));
      const override=contextFor([complete,{...cases[1],comparison:casePolicy('stock_area_m2')}]),before=JSON.stringify(override);
      const result=JSON.parse(build(input,override));
      assert.equal(result.design_adaptive.case_key,'cold');assert.equal(JSON.stringify(override),before);
      if(policy.constraints)assert.deepEqual(result.comparison.constraints,policy.constraints);
      if(policy.requiredConsumers)assert.deepEqual(result.comparison.required_consumers,policy.requiredConsumers);
    }
  }
  const separate=contextFor([{key:'cold',surface:false,motion:true},{key:'warm',surface:false,materialStrain:true}]);
  assert.throws(()=>build({...state(),requiredConsumers:['motion_material_strain']},separate),/requested motion_material_strain/);
  assert.throws(()=>build({...state(),objectives:[{metric:'motion_max_warp_green_strain',direction:'minimize'}]},separate),/requested motion_material_strain/);
  const both=contextFor([complete,{...complete,key:'warm'}]);
  const input={...state(),objectives:[{metric:'motion_max_warp_green_strain',direction:'minimize'}],design:namedDesign()};
  assert.equal(JSON.parse(build(input,both)).comparison.objectives[0].metric,'motion_max_warp_green_strain');
  for(const key of ['motion','materialStrain']) {
    const absent={...both};delete absent[key];assert.throws(()=>build(input,absent),/actual cases/);
    assert.throws(()=>build(input,{...both,[key]:false}),/actual cases/);
    for(const value of [null,undefined,'true',1])assert.throws(()=>build(state(),contextFor([{...complete,[key]:value}])),/availability must be a boolean/);
  }
});

test('named motion comparisons admit inherited consumers case by case and retain original policy authority', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const metric='motion_max_absolute_edge_strain_pct';
  const cases=[{key:'moving',surface:false,motion:true},{key:'static',surface:false,motion:false}];
  for(const input of [{...state(),objectives:[{metric,direction:'minimize'}]},
    {...state(),constraints:[{metric,maximum:8}]},{...state(),requiredConsumers:['motion']}]) {
    const context=namedContext(cases);assert.throws(()=>build(input,context),/static inherits.*motion/);
    assert.ok(build(input,namedContext([cases[0],{...cases[1],comparison:casePolicy()}])));
    assert.throws(()=>build(input,{...context,motion:false}),/motion availability/);
  }
  const advanced=casePolicy('motion_max_body_contact_vertex_force_n','minimize',{
    constraints:[{metric,maximum:8,basis:'nominal'}],required_consumers:['motion','motion_contact_loads'],allow_mixed_protocols:true});
  const context=namedContext([{...cases[0],comparison:advanced},{...cases[1],comparison:advanced}]);
  const before=JSON.stringify(context);assert.ok(build(state(),context));assert.equal(JSON.stringify(context),before,
    'Original supplied policies and their complete consumer admission remain with Rust, including a deliberately missing motion input');
  for(const motion of [null,'true',1,undefined]) {
    assert.throws(()=>build(state(),namedContext([{...cases[0],motion}])),/boolean/);
  }
});

test('named contact-load decisions preserve case policies and require motion and loads in the same case', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const complete={key:'cold',surface:false,motion:true,contactLoads:true};
  for(const [metrics,consumer] of [[initialContactMetrics,'contact_loads'],[movingContactMetrics,'motion_contact_loads']]) {
    for(const metric of metrics)for(const policy of [{objectives:[{metric,direction:'maximize'}]},
      {constraints:[{metric,maximum:2}]},{requiredConsumers:[consumer]}]) {
      const input={...state(),...policy,design:namedDesign()},missing={key:'warm',surface:false,motion:true};
      assert.throws(()=>build(input,namedContext([complete,missing])),new RegExp(`warm inherits.*${consumer}`));
      const override=namedContext([complete,{...missing,comparison:casePolicy()}]),before=JSON.stringify(override);
      const result=JSON.parse(build(input,override));
      assert.equal(result.design_adaptive.case_key,'cold');assert.equal(JSON.stringify(override),before);
      if(policy.objectives)assert.deepEqual(result.comparison.objectives,policy.objectives);
      if(policy.constraints)assert.deepEqual(result.comparison.constraints,policy.constraints);
      if(policy.requiredConsumers)assert.deepEqual(result.comparison.required_consumers,policy.requiredConsumers);
    }
  }
  const moving={...state(),requiredConsumers:['motion_contact_loads']};
  const split=namedContext([{key:'cold',surface:false,motion:true},{key:'warm',surface:false,contactLoads:true}]);
  assert.throws(()=>build(moving,split),/requested motion_contact_loads/);
  assert.throws(()=>build(moving,namedContext([complete,{key:'warm',surface:false,contactLoads:true}])) ,/warm inherits.*motion_contact_loads/);
  const both=namedContext([complete,{...complete,key:'warm'}]);assert.ok(build(moving,both));
  const absent={...both};delete absent.contactLoads;
  for(const context of [absent,{...both,contactLoads:false}])assert.throws(()=>build(moving,context),/contactLoads availability.*actual cases/);
  for(const contactLoads of [null,undefined,'true',1])assert.throws(()=>build(state(),namedContext([{...complete,contactLoads}])),/availability must be a boolean/);
  const advanced=casePolicy('motion_max_body_contact_vertex_force_n','minimize',{
    required_consumers:['motion_contact_loads'],allow_mixed_protocols:true});
  const original=namedContext([{key:'cold',surface:false,comparison:advanced}]),before=JSON.stringify(original);
  assert.ok(build(state(),original));assert.equal(JSON.stringify(original),before,'Native admission retains authority over original case policies');
});

test('named heat and vapor policies require each inherited consumer and a complete motion-history case', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const complete={key:'cold',surface:false,coverage:true,vaporSurface:true,vaporCoverage:true,motion:true,motionVaporHistory:true};
  for(const [metric,consumer,flag] of coverageVaporMetrics)for(const policy of [
    {basis:'reported_bounds',objectives:[{metric,direction:'maximize'}]},
    {constraints:[{metric,basis:'reported_bounds',minimum:metric==='body_coverage_power_w'?-2:0,maximum:3}]},
    {requiredConsumers:[consumer]}]) {
    const input={...state(),...policy,design:namedDesign()},missing={...complete,key:'warm',[flag]:false};
    assert.throws(()=>build(input,namedContext([complete,missing])),new RegExp(`warm inherits.*${consumer}`));
    const override=namedContext([complete,{...missing,comparison:casePolicy()}]),before=JSON.stringify(override);
    const result=JSON.parse(build(input,override));
    assert.equal(result.design_adaptive.case_key,'cold');assert.equal(JSON.stringify(override),before);
    if(policy.objectives)assert.deepEqual(result.comparison.objectives,policy.objectives);
    if(policy.constraints)assert.deepEqual(result.comparison.constraints,policy.constraints);
    if(policy.requiredConsumers)assert.deepEqual(result.comparison.required_consumers,policy.requiredConsumers);
  }
  const split=namedContext([{key:'moving',surface:false,motion:true,motionVaporHistory:true},
    {key:'covered',surface:false,vaporCoverage:true}]);
  assert.throws(()=>build({...state(),requiredConsumers:['motion_vapor_coverage']},split),/requested motion_vapor_coverage/);
  const both=namedContext([complete,{...complete,key:'warm'}]);
  for(const key of ['coverage','vaporSurface','vaporCoverage','motionVaporHistory']) {
    const absent={...both};delete absent[key];
    for(const context of [absent,{...both,[key]:false}])assert.throws(()=>build(state(),context),/availability.*actual cases/);
    for(const value of [null,undefined,'true',1])assert.throws(()=>build(state(),namedContext([{...complete,[key]:value}])),/availability must be a boolean/);
  }
  const advanced=casePolicy('motion_covered_vapor_potential_energy_j','minimize',{
    basis:'reported_bounds',required_consumers:['motion_vapor_coverage'],allow_mixed_protocols:true});
  const original=namedContext([{key:'cold',surface:false,comparison:advanced}]),before=JSON.stringify(original);
  assert.ok(build(state(),original));assert.equal(JSON.stringify(original),before,'Rust owns source and protocol admission of the complete original case policy');
  const worst=namedDesign({objective:'consumer_worst_case'});delete worst.caseKey;
  const bounded=casePolicy('body_coverage_power_w','maximize',{basis:'reported_bounds',required_consumers:['coverage']});
  const input={...state(),basis:'reported_bounds',objectives:bounded.objectives,requiredConsumers:['coverage'],design:worst};
  assert.equal(JSON.parse(build(input,namedContext([complete,{...complete,key:'warm',comparison:bounded}]))).design_adaptive.objective,'consumer_worst_case');
});

test('reported sheet heat respects effective named-case policies and adaptive guidance without changing sources', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),basis:'reported_bounds',
    objectives:[{metric:'independent_surface_power_w',direction:'maximize'}],
    constraints:[{metric:'independent_surface_power_w',minimum:-20,maximum:-1},
      {metric:'max_absolute_edge_strain_pct',basis:'nominal',maximum:8}],requiredConsumers:['surface'],design:namedDesign()};
  const inherited=namedContext([{key:'cold',surface:true},{key:'warm',surface:false}]);
  assert.throws(()=>build(input,inherited),/warm inherits.*surface/);
  const override=casePolicy('stock_area_m2','minimize',{constraints:[{metric:'stock_mass_kg',basis:'nominal',maximum:2}]}),
    context=namedContext([{key:'cold',surface:true},{key:'warm',surface:false,comparison:override}]);
  const before=JSON.stringify(input),original=JSON.stringify(context),result=JSON.parse(build(input,context));
  assert.equal(JSON.stringify(input),before);assert.equal(JSON.stringify(context),original);
  assert.equal(result.comparison.basis,'reported_bounds');assert.deepEqual(result.comparison.objectives,input.objectives);
  assert.deepEqual(result.comparison.constraints,input.constraints);assert.equal(result.design_adaptive.case_key,'cold');
  assert.equal(JSON.parse(build({...input,design:namedDesign({caseKey:'warm'})},context)).design_adaptive.case_key,'warm');

  const worst=namedDesign({objective:'consumer_worst_case'});delete worst.caseKey;
  const complete=namedContext([{key:'cold',surface:true},{key:'warm',surface:true,comparison:casePolicy(
    'independent_surface_power_w','maximize',{basis:'reported_bounds',constraints:[
      {metric:'independent_surface_power_w',minimum:-15,maximum:-5}],required_consumers:['surface']})}]);
  const preserved=JSON.stringify(complete);
  assert.equal(JSON.parse(build({...input,design:worst},complete)).design_adaptive.objective,'consumer_worst_case');
  assert.equal(JSON.stringify(complete),preserved);
  for(const changes of [{basis:'nominal'},{objectives:[{metric:'independent_surface_power_w',direction:'minimize'}]}]) {
    const changed=namedContext([complete.matrix.cases[0],{...complete.matrix.cases[1],
      comparison:{...complete.matrix.cases[1].comparison,...changes}}]);
    assert.throws(()=>build({...input,design:worst},changed),/identical metric, direction and comparison basis/);
  }
});

const scenarioSnapshot = (changes={}) => ' \r\n'+JSON.stringify({schema:'ftx-drape-scenario/1',
  options:scenarioState().options,fit_spec:{},max_mesh_points:32768,max_vertex_frames:8000000,...changes})+'\r\n';

test('named scenario authoring preserves every original scenario and case policy fragment in declared order', async () => {
  const {buildMaterialScenarioMatrixRequest:build,buildMaterialScenarioRequest:single}=await modulePromise;
  const fit=' {"schema":"fabric-fit/1","points":[],"source":{"seed":18446744073709551615,"zero":-0.0,"label":"é 🧵"}}\r\n';
  const first=single({...scenarioState(),allowNoFit:false,fitSpec:fit,surface:heatSource(),maxVertexFrames:32000000});
  const second=single({...scenarioState(),maxVertexFrames:32000000});
  const policy=' {"basis":"nominal","objectives":[{"metric":"independent_surface_power_w","direction":"maximize"}],'+
    '"constraints":[{"metric":"max_absolute_edge_strain_pct","minimum":-0.0,"maximum":8.0}],'+
    '"required_consumers":["drape","surface"],"allow_mixed_protocols":true}\r\n';
  const input={maxEvaluations:8,cases:[{key:'warm "outside"',scenario:first,comparison:policy},{key:'cold 🧵',scenario:second}]};
  const before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.equal(result.schema,'ftx-drape-scenario-matrix/1');assert.equal(result.max_evaluations,8);
  assert.deepEqual(result.cases.map(item=>item.key),['warm "outside"','cold 🧵']);
  for(const original of [first,second,policy,fit,heatSource()])assert.ok(raw.includes(original));
  assert.match(raw,/18446744073709551615/);assert.match(raw,/"minimum":-0\.0/);assert.match(raw,/"maximum":8\.0/);
  assert.deepEqual(result.cases[0].comparison,JSON.parse(policy));assert.equal(Object.hasOwn(result.cases[1],'comparison'),false);
  assert.equal(Object.hasOwn(result.cases[1].scenario,'surface'),false,'A case never inherits another case’s heat input');
  const explicit=build({maxEvaluations:1,cases:[{key:'null policy',scenario:second,comparison:' \nnull\r\n'}]});
  assert.ok(explicit.includes(' \nnull\r\n'));assert.equal(JSON.parse(explicit).cases[0].comparison,null);
});

test('named scenarios retain authored motion and strain budgets without sharing optional consumers across cases', async () => {
  const {buildMaterialScenarioMatrixRequest:build,buildMaterialScenarioRequest:single}=await modulePromise;
  const motion=' \r\n'+JSON.stringify(motionPolicy())+'\r\n',materialStrain=' \r\n'+JSON.stringify(strainPolicy({instances:['front:right']}))+'\r\n';
  const moving=single({...scenarioState(),motion,maxMotionTrials:16384,materialStrain,maxVertexFrames:32000000}),
    stationary=single({...scenarioState(),maxVertexFrames:32000000});
  const raw=build({maxEvaluations:4,cases:[{key:'moving',scenario:moving},{key:'stationary',scenario:stationary}]}),result=JSON.parse(raw);
  for(const original of [moving,stationary,motion,materialStrain])assert.ok(raw.includes(original));
  assert.equal(result.cases[0].scenario.max_motion_trials,16384);
  for(const key of ['motion','material_strain','max_motion_trials'])assert.equal(Object.hasOwn(result.cases[1].scenario,key),false);
  assert.equal(JSON.parse(build({maxEvaluations:4,cases:[{key:'one',scenario:moving},{key:'two',scenario:moving}]})).cases.length,2);
  const excessive=single({...scenarioState(),motion,maxMotionTrials:16385,materialStrain,maxVertexFrames:32000000});
  assert.throws(()=>build({maxEvaluations:4,cases:[{key:'one',scenario:moving},{key:'two',scenario:excessive}]}),/32768 active motion trials/);
});

test('named scenarios retain each contact scope and original allowance without rewriting independent requests', async () => {
  const {buildMaterialScenarioMatrixRequest:build,buildMaterialScenarioRequest:single}=await modulePromise;
  const contacts=' \r\n'+JSON.stringify(contactPolicy({instances:['front:right'],max_vertex_samples:64000000,max_pair_evaluations:64000000}))+'\r\n';
  const materialStrain=JSON.stringify(strainPolicy()),motion=JSON.stringify(motionPolicy());
  const moving=single({...scenarioState(),contactLoads:contacts,materialStrain,motion,maxMotionTrials:4096,maxVertexFrames:32000000});
  const staticOnly=single({...scenarioState(),contactLoads:contacts,maxVertexFrames:32000000});
  const raw=build({maxEvaluations:4,cases:[{key:'moving',scenario:moving},{key:'initial',scenario:staticOnly}]}),result=JSON.parse(raw);
  for(const original of [moving,staticOnly,contacts,materialStrain,motion])assert.ok(raw.includes(original));
  assert.equal(result.cases[1].scenario.contact_loads.max_vertex_samples,64000000);
  for(const key of ['motion','material_strain','max_motion_trials'])assert.equal(Object.hasOwn(result.cases[1].scenario,key),false);
  const independent=single({...scenarioState(),maxVertexFrames:32000000});
  const mixed=JSON.parse(build({maxEvaluations:4,cases:[{key:'loaded',scenario:moving},{key:'independent',scenario:independent}]}));
  assert.equal(Object.hasOwn(mixed.cases[1].scenario,'contact_loads'),false,'A case never inherits another case’s contact request');
});

test('named scenarios retain original coverage and motion-vapor policies without leaking them into other cases', async () => {
  const {buildMaterialScenarioMatrixRequest:build,buildMaterialScenarioRequest:single}=await modulePromise;
  const coverage=' \r\n'+JSON.stringify(coveragePolicy())+'\r\n',vaporSurface=' \r\n'+JSON.stringify(vaporSurfacePolicy())+'\r\n',
    vaporCoverage=' \r\n'+JSON.stringify(vaporCoveragePolicy())+'\r\n';
  const motion=' \r\n'+JSON.stringify(motionPolicy({vapor_coverage_history:vaporHistoryPolicy()}))+'\r\n';
  const moving=single({...scenarioState(),coverage,vaporSurface,vaporCoverage,motion,maxMotionTrials:4096,maxVertexFrames:32000000}),
    independent=single({...scenarioState(),maxVertexFrames:32000000});
  const comparison=' {"basis":"reported_bounds","objectives":[{"metric":"motion_covered_vapor_potential_energy_j","direction":"maximize"}],'+
    '"constraints":[{"metric":"body_coverage_power_w","minimum":-2.0,"maximum":-0.0}],'+
    '"required_consumers":["motion_vapor_coverage","coverage"],"allow_mixed_protocols":true}\r\n';
  const input={maxEvaluations:4,cases:[{key:'covered',scenario:moving,comparison},{key:'independent',scenario:independent}]},before=JSON.stringify(input);
  const raw=build(input),result=JSON.parse(raw);assert.equal(JSON.stringify(input),before);
  for(const original of [moving,independent,coverage,vaporSurface,vaporCoverage,motion,comparison])assert.ok(raw.includes(original));
  assert.deepEqual(result.cases[0].comparison,JSON.parse(comparison));assert.equal(result.cases[0].scenario.max_motion_trials,4096);
  for(const key of ['coverage','vapor_surface','vapor_coverage','motion','motionVaporHistory','max_motion_trials']) {
    assert.equal(Object.hasOwn(result.cases[1].scenario,key),false);
  }
  assert.equal(Object.hasOwn(result.cases[1],'comparison'),false);
});

test('matrix snapshot resources use native active-motion defaults and complete aggregate limits', async () => {
  const {buildMaterialScenarioMatrixRequest:build}=await modulePromise;
  const motion={schema:'ftx-drape-motion/1',keyframes:[{time_s:0.1,arm_abduction_deg:35}],
    max_step_s:0.01,max_trials:10,max_pairs:100,max_sample_vertices:1000};
  const cases=Array.from({length:8},(_,i)=>({key:'case-'+i,scenario:scenarioSnapshot({motion})}));
  const raw=build({maxEvaluations:8,cases}),result=JSON.parse(raw);
  assert.equal(result.cases.length,8);
  assert.ok(result.cases.every(item=>!Object.hasOwn(item.scenario,'max_motion_trials')),'The4096 default is counted, never inserted into retained source');
  const replace=(changes)=>[{...cases[0],scenario:scenarioSnapshot({motion,...changes})},...cases.slice(1)];
  assert.throws(()=>build({maxEvaluations:8,cases:replace({max_motion_trials:4097})}),/32768 active motion trials/);
  assert.throws(()=>build({maxEvaluations:8,cases:replace({max_vertex_frames:8000001})}),/64000000 aggregate vertex frames/);
  assert.equal(JSON.parse(build({maxEvaluations:8,cases:replace({motion:null,max_motion_trials:32768})})).cases[0].scenario.max_motion_trials,32768,
    'An inactive case’s declared trial budget does not consume the active-motion sum');
  const inactive={schema:'ftx-drape-scenario/1',options:scenarioState().options,fit_spec:{},max_mesh_points:32768,
    max_vertex_frames:32000000,max_motion_trials:32768};
  assert.equal(JSON.parse(build({maxEvaluations:2,cases:[{key:'one',scenario:JSON.stringify(inactive)},
    {key:'two',scenario:JSON.stringify(inactive)}]})).cases.length,2);
  for(const max_motion_trials of [0,-1,1.5,32769,null,'4096']) {
    assert.throws(()=>build({maxEvaluations:1,cases:[{key:'invalid',scenario:scenarioSnapshot({max_motion_trials})}]}),/motion-trial budgets/);
  }
  for(const max_vertex_frames of [undefined,0,-1,1.5,64000001,null,'8000000']) {
    assert.throws(()=>build({maxEvaluations:1,cases:[{key:'invalid',scenario:scenarioSnapshot({max_vertex_frames})}]}),/explicit integer vertex-frame/);
  }
});

test('named snapshot authoring rejects ambiguous envelopes, duplicate keys and invalid source encodings', async () => {
  const {buildMaterialScenarioMatrixRequest:build}=await modulePromise;
  const sample={key:'case',scenario:scenarioSnapshot()},input={maxEvaluations:1,cases:[sample]};
  for(const value of [null,[],{}, {...input,unknown:1}, {...input,cases:[]},
    {...input,cases:Array.from({length:9},(_,i)=>({...sample,key:String(i)}))}])assert.throws(()=>build(value));
  for(const maxEvaluations of [0,65,1.5,NaN,Infinity,'1',null])assert.throws(()=>build({...input,maxEvaluations}),/allowance/);
  assert.throws(()=>build({maxEvaluations:1,cases:[sample,{...sample,key:'second'}]}),/covering every case/);
  assert.throws(()=>build({maxEvaluations:2,cases:[sample,sample]}),/unique/);
  for(const key of ['',null,42,'é'.repeat(65)])assert.throws(()=>build({...input,cases:[{...sample,key}]}));
  assert.equal(JSON.parse(build({...input,cases:[{...sample,key:'é'.repeat(64)}]})).cases[0].key,'é'.repeat(64));
  for(const item of [null,{}, {...sample,unknown:true}, {...sample,scenario:JSON.parse(sample.scenario)},
    {...sample,scenario:'\ufeff'+sample.scenario}, {...sample,scenario:'null'}, {...sample,scenario:'['},
    {...sample,scenario:'{"schema":"ftx-drape-scenario-matrix/1"}'}])assert.throws(()=>build({...input,cases:[item]}));
  for(const comparison of [{},undefined,'[]','false','{}','{bad','\ufeffnull',
    JSON.stringify(casePolicy('stock_area_m2','automatic'))]) {
    assert.throws(()=>build({...input,cases:[{...sample,comparison}]}));
  }
});

test('matrix envelopes retain advanced original scenarios and policies without recreating numerical admission', async () => {
  const {buildMaterialScenarioMatrixRequest:build}=await modulePromise;
  const original=scenarioSnapshot({max_motion_trials:5000,
    motion:{schema:'ftx-drape-motion/1',keyframes:[{time_s:0.2,arm_abduction_deg:50,leg_pose:{posture:'seated',fraction:0.5}}],
      max_step_s:0.01,max_trials:40,max_pairs:50000,max_sample_vertices:40000},
    material_strain:{schema:'ftx-material-strain/1',max_triangle_samples:100000}});
  const comparison=' \n'+JSON.stringify(casePolicy('motion_min_warp_green_strain','maximize',{
    constraints:[{metric:'motion_max_body_contact_vertex_force_n',basis:'nominal',maximum:5}],
    required_consumers:['motion','motion_material_strain','motion_contact_loads'],allow_mixed_protocols:true}))+'\r\n';
  const raw=build({maxEvaluations:1,cases:[{key:'prescribed motion',scenario:original,comparison}]});
  assert.ok(raw.includes(original));assert.ok(raw.includes(comparison));
  assert.deepEqual(JSON.parse(raw).cases[0].scenario,JSON.parse(original));
  assert.deepEqual(JSON.parse(raw).cases[0].comparison,JSON.parse(comparison));
});

test('complete named scenario snapshots share an exact UTF-8 request limit including framing bytes', async () => {
  const {buildMaterialScenarioMatrixRequest:build}=await modulePromise;
  const base=scenarioSnapshot({max_vertex_frames:1});
  const source=padding=>base.replace('"fit_spec":{}','"fit_spec":{"note":"'+padding+'"}');
  const input=padding=>({maxEvaluations:2,cases:[{key:'first 🧵',scenario:source(padding)},
    {key:'second',scenario:base,comparison:'null'}]});
  const overhead=Buffer.byteLength(build(input(''))),remaining=16*1024*1024-overhead;
  const padding='é'.repeat(Math.floor(remaining/2))+'x'.repeat(remaining%2);
  const raw=build(input(padding));assert.equal(Buffer.byteLength(raw),16*1024*1024);
  assert.throws(()=>build(input(padding+'x')),/16 MiB/);
  const shared='é'.repeat(3*1024*1024);
  assert.throws(()=>build({maxEvaluations:3,cases:Array.from({length:3},(_,i)=>({key:String(i),scenario:source(shared)}))}),/16 MiB/);
});

const transientDomains=[['transient_surface','transientSurface'],['transient_coverage','transientCoverage']];
const transientSuffixes=['inside_energy_j','net_boundary_energy_j','min_sample_temperature_c','max_sample_temperature_c',
  'final_min_temperature_c','final_max_temperature_c'];
const transientMetrics=transientDomains.flatMap(([consumer,flag])=>transientSuffixes.map(suffix=>[consumer+'_'+suffix,consumer,flag]));

test('all twelve transient heat criteria preserve signed nominal readings independently of steady heat', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,coverage:false,motion:true,transientSurface:true,transientCoverage:true};
  const input={...state(),objectives:transientMetrics.map(([metric],i)=>({metric,direction:i%2?'maximize':'minimize'})),
    constraints:transientMetrics.map(([metric])=>({metric,minimum:-20,maximum:-1})),
    requiredConsumers:['transient_surface','transient_coverage']};
  delete input.cutting;
  const before=JSON.stringify(input),raw=build(input,context),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(input.candidates[0].selection));
  assert.deepEqual(result.comparison,{basis:'nominal',objectives:input.objectives,constraints:input.constraints,
    required_consumers:input.requiredConsumers,allow_mixed_protocols:false});
  for(const key of ['cutting','scenario','motion','surface','coverage','transient_surface','transient_coverage']) {
    assert.equal(Object.hasOwn(result,key),false,'Context does not create or replace a physical request');
  }
  for(const [metric,consumer,flag] of transientMetrics) {
    const independent={...state(),objectives:[{metric,direction:'maximize'}],requiredConsumers:[consumer]};
    assert.ok(build(independent,{drape:true,surface:false,motion:true,[flag]:true}));
  }
});

test('transient objectives, limits and required consumers need their own requested history plus motion', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const [consumer,flag] of transientDomains)for(const policy of [
    {objectives:[{metric:consumer+'_inside_energy_j',direction:'minimize'}]},
    {constraints:[{metric:consumer+'_final_min_temperature_c',minimum:-10}]},
    {requiredConsumers:[consumer]}]) {
    const input={...state(),...policy},context={drape:true,surface:false,motion:true,[flag]:true};
    assert.ok(build(input,context));
    assert.throws(()=>build(input),new RegExp(`requested ${consumer}`));
    for(const key of ['motion',flag]) {
      const absent={...context};delete absent[key];
      for(const missing of [absent,{...context,[key]:false}]) {
        assert.throws(()=>build(input,missing),new RegExp(`requested ${consumer}`));
      }
    }
    const other=flag==='transientSurface'?'transientCoverage':'transientSurface';
    assert.throws(()=>build(input,{drape:true,surface:true,coverage:true,motion:true,[other]:true}),new RegExp(`requested ${consumer}`));
    const inherited=Object.assign(Object.create({[flag]:true}),{drape:true,surface:false,motion:true});
    assert.throws(()=>build(input,inherited),new RegExp(`requested ${consumer}`));
  }
});

test('transient heat stays nominal while signed guards coexist with independently bounded material objectives', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={drape:true,surface:false,motion:true,transientSurface:true,transientCoverage:true};
  const input={...state(),assemblyMass:mass(),basis:'reported_bounds',
    objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
    constraints:transientMetrics.map(([metric])=>({metric,basis:'nominal',minimum:-5,maximum:-1}))};
  assert.deepEqual(JSON.parse(build(input,context)).comparison.constraints,input.constraints);
  for(const [metric] of transientMetrics) {
    assert.throws(()=>build({...state(),basis:'reported_bounds',objectives:[{metric,direction:'minimize'}]},context),/explicit nominal basis/);
    for(const basis of [undefined,'reported_bounds']) {
      const constraint={metric,minimum:-5};if(basis!==undefined)constraint.basis=basis;
      assert.throws(()=>build({...input,constraints:[constraint]},context),/explicit nominal basis/);
    }
  }
  for(const constraint of [{minimum:NaN},{maximum:Infinity},{minimum:'-5'},{minimum:-1,maximum:-5}]) {
    assert.throws(()=>build({...state(),constraints:[{metric:'transient_surface_final_min_temperature_c',...constraint}]},context),/finite ordered/);
  }
});

test('named transient comparisons require motion and their heat domain in the same inheriting case', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const [consumer,flag] of transientDomains) {
    const complete={key:'cold',surface:false,motion:true,[flag]:true};
    const split=namedContext([{key:'moving',surface:false,motion:true},{key:'heated',surface:false,[flag]:true}]);
    assert.throws(()=>build({...state(),requiredConsumers:[consumer]},split),new RegExp(`requested ${consumer}`));
    for(const policy of [{objectives:[{metric:consumer+'_inside_energy_j',direction:'maximize'}]},
      {constraints:[{metric:consumer+'_final_min_temperature_c',minimum:-5}]},{requiredConsumers:[consumer]}]) {
      const input={...state(),...policy,design:namedDesign()},missing={key:'warm',surface:false,motion:true};
      assert.throws(()=>build(input,namedContext([complete,missing])),new RegExp(`warm inherits.*${consumer}`));
      const context=namedContext([complete,{...missing,comparison:casePolicy()}]),before=JSON.stringify(context);
      assert.ok(build(input,context));assert.equal(JSON.stringify(context),before);
    }
    const both=namedContext([complete,{...complete,key:'warm'}]);
    const absent={...both};delete absent[flag];
    for(const context of [absent,{...both,[flag]:false}]) {
      assert.throws(()=>build(state(),context),new RegExp(`${flag} availability.*actual cases`));
    }
    for(const value of [null,undefined,'true',1]) {
      assert.throws(()=>build(state(),namedContext([{...complete,[flag]:value}])),/availability must be a boolean/);
    }
    const original=namedContext([{key:'cold',surface:false,comparison:casePolicy(consumer+'_inside_energy_j','minimize',{
      constraints:[{metric:consumer+'_final_min_temperature_c',minimum:-5}],required_consumers:[consumer],allow_mixed_protocols:true})}]);
    const before=JSON.stringify(original);assert.ok(build(state(),original));assert.equal(JSON.stringify(original),before,
      'An original case policy keeps native consumer and protocol admission rather than inheriting the guided fallback');
  }
});

test('transient criteria drive linked grids and every adaptive guidance mode without changing geometry or source policies', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  for(const [consumer,flag] of transientDomains) {
    const objectives=[{metric:consumer+'_final_min_temperature_c',direction:'maximize'},
      {metric:consumer+'_inside_energy_j',direction:'minimize'}];
    const input={...state(),objectives:objectives.slice(0,1),constraints:[{metric:consumer+'_final_max_temperature_c',maximum:30}],
      requiredConsumers:[consumer]},context={drape:true,surface:false,motion:true,[flag]:true};
    const grid={...input,design:linkedDesignInput({values:[0,5],maxDesigns:1})},raw=build(grid,context);
    assert.equal(JSON.parse(raw).schema,'ftx-design-study/1');assert.ok(raw.includes(grid.design.fitSpec));
    assert.ok(raw.includes(grid.candidates[0].selection));
    for(const objective of ['consumer_comparison','consumer_pareto']) {
      const candidate={...input,objectives:objective==='consumer_pareto'?objectives:objectives.slice(0,1),
        design:adaptiveDesignInput({objective,materialKey:'alternative',maxEvaluations:2,maxDesigns:1})};
      const result=JSON.parse(build(candidate,context));
      assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.objectives,candidate.objectives);
    }
    const cases=namedContext([{key:'cold',surface:false,motion:true,[flag]:true},
      {key:'warm',surface:false,motion:true,[flag]:true}]),before=JSON.stringify(cases);
    for(const objective of ['consumer_comparison','consumer_pareto','consumer_worst_case','consumer_matrix_pareto']) {
      const design=namedDesign({objective});
      if(['consumer_worst_case','consumer_matrix_pareto'].includes(objective))delete design.caseKey;
      const candidate={...input,design,objectives:objective==='consumer_pareto'?objectives:objectives.slice(0,1)};
      const result=JSON.parse(build(candidate,cases));
      assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.constraints,input.constraints);
      assert.deepEqual(result.comparison.required_consumers,[consumer]);assert.equal(JSON.stringify(cases),before);
    }
  }
});

test('original transient policies remain exact through single and named motion snapshots without creating steady domains', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const film='{"inside_c":30.0,"outside_c":-10.0,"inside_resistance_m2_k_w":-0.0,"outside_resistance_m2_k_w":1.0}';
  const surface=' {\r\n"schema":"ftx-motion-surface-heat/1","initial_temperature_c":-0.0,"cells_per_layer":1,'+
    '"boundaries":['+film+','+film+'],"max_step_s":0.01,"max_cell_steps":1000}\r\n';
  const coverage=' {\r\n"schema":"ftx-motion-coverage-heat/1","body_triangle_ids":[2],"instance_layers":{"band":"shell"},'+
    '"settings":'+JSON.stringify(coveragePolicy().settings)+',"initial_temperature_c":-0.0,"boundaries":['+film+','+film+'],'+
    '"exposed_boundaries":[null,null],"max_step_s":0.01,"max_cell_steps":1000,"max_geometry_operations":100000}\r\n';
  const motion=' \r\n'+JSON.stringify(motionPolicy()).slice(0,-1)+',"transient_surface":'+surface+',"transient_coverage":'+coverage+'}\r\n';
  const input={...scenarioState(),motion,maxMotionTrials:64,maxVertexFrames:32000000,allowNoFit:false,fitSpec:linkedFitSource()};
  const before=JSON.stringify(input),snapshot=single(input),parsed=JSON.parse(snapshot);
  assert.equal(JSON.stringify(input),before);
  for(const original of [motion,surface,coverage,input.fitSpec])assert.ok(snapshot.includes(original));
  for(const key of ['surface','coverage','transient_surface','transient_coverage'])assert.equal(Object.hasOwn(parsed,key),false);
  const comparison=' {"basis":"nominal","objectives":[{"metric":"transient_surface_final_min_temperature_c","direction":"maximize"}],'+
    '"constraints":[{"metric":"transient_coverage_inside_energy_j","maximum":-0.0}],'+
    '"required_consumers":["transient_surface","transient_coverage"],"allow_mixed_protocols":false}\r\n';
  const independent=single({...scenarioState(),maxVertexFrames:32000000});
  const raw=matrix({maxEvaluations:4,cases:[{key:'moving heat',scenario:snapshot,comparison},{key:'static',scenario:independent}]}),result=JSON.parse(raw);
  for(const original of [snapshot,motion,surface,coverage,comparison,independent])assert.ok(raw.includes(original));
  assert.deepEqual(result.cases[0].comparison,JSON.parse(comparison));assert.equal(Object.hasOwn(result.cases[1].scenario,'motion'),false);
  assert.equal(Object.hasOwn(result.cases[1],'comparison'),false);
  for(const item of result.cases)for(const key of ['transientSurface','transientCoverage']) {
    assert.equal(Object.hasOwn(item,key),false);assert.equal(Object.hasOwn(item.scenario,key),false);
  }
  const absent=single({...scenarioState(),maxMotionTrials:64,motion:JSON.stringify(motionPolicy({transient_surface:null,transient_coverage:null}))});
  assert.equal(JSON.parse(absent).motion.transient_surface,null);assert.equal(JSON.parse(absent).motion.transient_coverage,null);
});

test('contact allowance exhaustion stays a native partial refusal across alternatives, probes and independent cases', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix,buildMaterialStudyRequest:build}=await modulePromise;
  const contactLoads=' \r\n'+JSON.stringify(contactPolicy({max_vertex_samples:64000000,max_pair_evaluations:64000000}))+'\r\n';
  const snapshot=single({...scenarioState(),contactLoads,maxVertexFrames:32000000}),
    cases=[{key:'cold',scenario:snapshot},{key:'warm',scenario:snapshot}];
  const source=matrix({maxEvaluations:64,cases});
  assert.equal(source.split(contactLoads).length-1,2,'Each case retains its own full allowances rather than a cross-case sum');
  const context=namedContext(cases.map(({key})=>({key,surface:false,contactLoads:true})));
  const input={...state(),design:namedDesign(),objectives:[{metric:'body_contact_resultant_n',direction:'minimize'}]};
  assert.equal(JSON.parse(build(input,context)).design_adaptive.max_evaluations,2,
    'The authoring helper does not multiply optional declared contact work across consumer probes and retained alternatives');
  const optional=JSON.parse(build(state(),context));assert.deepEqual(optional.comparison.required_consumers,[]);
  assert.equal(optional.variants.length,2,'A full first reservation can refuse later optional contact results without blocking independent study outcomes');
});

const wetFilmMetrics=['motion_wet_film_evaporated_mass_kg','motion_wet_film_remaining_liquid_mass_kg',
  'motion_wet_film_unmet_demand_mass_kg','motion_wet_film_supplied_mass_kg','motion_wet_film_body_reservoir_latent_energy_j'];
const wetFilmContext=()=>({drape:true,surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true,motionWetFilm:true});

test('finite body-water criteria retain all five explicit nominal or source-bound decisions and independent nominal guards', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={...wetFilmContext(),contactLoads:true,transientSurface:true};
  for(const basis of ['nominal','reported_bounds']) {
    const input={...state(),basis,objectives:wetFilmMetrics.map((metric,i)=>({metric,direction:i%2?'maximize':'minimize'})),
      constraints:wetFilmMetrics.map(metric=>({metric,minimum:0,maximum:1})),requiredConsumers:['motion_wet_film']};
    delete input.cutting;
    input.constraints.push({metric:'body_contact_resultant_n',basis:'nominal',maximum:2},
      {metric:'transient_surface_final_min_temperature_c',basis:'nominal',minimum:-5});
    const before=JSON.stringify(input),raw=build(input,context),result=JSON.parse(raw);
    assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(input.candidates[0].selection));
    assert.deepEqual(result.comparison,{basis,objectives:input.objectives,constraints:input.constraints,
      required_consumers:input.requiredConsumers,allow_mixed_protocols:false});
    for(const key of ['motion','wet_film','motionWetFilm','vapor_coverage','reported_bounds'])assert.equal(Object.hasOwn(result,key),false);
    assert.throws(()=>build({...input,constraints:[{metric:wetFilmMetrics[0],minimum:1,maximum:0}]},context),/finite ordered/);
  }
  const input={...state(),constraints:wetFilmMetrics.map(metric=>({metric,basis:'reported_bounds',maximum:1}))};
  assert.deepEqual(JSON.parse(build(input,context)).comparison.constraints,input.constraints,
    'Bounded final water guards can accompany an independent nominal production objective');
});

test('finite body-water objectives, limits and required results need motion, coverage, history and an actual film policy', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context=wetFilmContext();
  for(const metric of wetFilmMetrics)for(const policy of [
    {basis:'reported_bounds',objectives:[{metric,direction:'maximize'}]},
    {constraints:[{metric,basis:'reported_bounds',minimum:0,maximum:1}]},{requiredConsumers:['motion_wet_film']}]) {
    const input={...state(),...policy};assert.ok(build(input,context));
    assert.throws(()=>build(input),/requested motion_wet_film/);
    for(const key of ['motion','vaporCoverage','motionVaporHistory','motionWetFilm']) {
      const absent={...context};delete absent[key];
      for(const missing of [absent,{...context,[key]:false}])assert.throws(()=>build(input,missing),/requested motion_wet_film/);
    }
    assert.throws(()=>build(input,{...context,motionWetFilm:false,transientSurface:true}),/requested motion_wet_film/,
      'Textile temperature or water does not supply a body-film policy');
  }
});

test('named finite body-water decisions require all dependencies in one inheriting case and retain native override authority', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const complete={key:'cold',surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true,motionWetFilm:true};
  for(const metric of wetFilmMetrics)for(const policy of [
    {basis:'reported_bounds',objectives:[{metric,direction:'maximize'}]},
    {constraints:[{metric,basis:'reported_bounds',maximum:1}]},{requiredConsumers:['motion_wet_film']}]) {
    const input={...state(),...policy,design:namedDesign()};
    for(const flag of ['motion','vaporCoverage','motionVaporHistory','motionWetFilm']) {
      const missing={...complete,key:'warm',[flag]:false};
      assert.throws(()=>build(input,namedContext([complete,missing])),/warm inherits.*motion_wet_film/);
      const original=namedContext([complete,{...missing,comparison:casePolicy()}]),before=JSON.stringify(original);
      assert.ok(build(input,original));assert.equal(JSON.stringify(original),before);
    }
  }
  const split=namedContext([{key:'covered motion',surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true},
    {key:'unrelated film',surface:false,motionWetFilm:true,comparison:casePolicy()}]);
  assert.throws(()=>build({...state(),requiredConsumers:['motion_wet_film']},split),/requested motion_wet_film/);
  const both=namedContext([complete,{...complete,key:'warm'}]),absent={...both};delete absent.motionWetFilm;
  for(const context of [absent,{...both,motionWetFilm:false}])assert.throws(()=>build(state(),context),/motionWetFilm availability.*actual cases/);
  for(const value of [null,undefined,'true',1])assert.throws(()=>build(state(),namedContext([{...complete,motionWetFilm:value}])),/availability must be a boolean/);
  const original=namedContext([{key:'cold',surface:false,comparison:casePolicy(wetFilmMetrics[0],'maximize',{
    basis:'reported_bounds',required_consumers:['motion_wet_film'],allow_mixed_protocols:true})}]),before=JSON.stringify(original);
  assert.ok(build(state(),original));assert.equal(JSON.stringify(original),before,
    'The complete original comparison retains native physical, source and protocol admission');
});

test('original body-film and motion sources survive single and named snapshots without inferred water or global optional-budget gates', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix,buildMaterialStudyRequest:build}=await modulePromise;
  const film=' {\r\n"schema":"ftx-body-wet-film/1","body_mesh_sha256":"'+'a'.repeat(64)+'",'+
    '"source":"declared synthetic water \\u03bc","basis":"prescribed local initial liquid and supply",'+
    '"initial_liquid_mass_kg":[0.0000000000000001000000000000001],"supply_rate_kg_s":[-0.0]}\r\n';
  const history=' {"schema":"ftx-motion-vapor-coverage/1","integration":"composite_trapezoid",'+
    '"max_geometry_operations":20000000,"wet_film":'+film+'}\r\n';
  const motion=' \r\n'+JSON.stringify(motionPolicy()).slice(0,-1)+',"vapor_coverage_history":'+history+'}\r\n';
  const vaporCoverage=' \r\n'+JSON.stringify(vaporCoveragePolicy())+'\r\n';
  const input={...scenarioState(),motion,vaporCoverage,maxMotionTrials:64,maxVertexFrames:32000000},before=JSON.stringify(input);
  const snapshot=single(input),independent=single({...scenarioState(),maxVertexFrames:32000000});
  assert.equal(JSON.stringify(input),before);
  const comparison=' {"basis":"reported_bounds","objectives":[{"metric":"'+wetFilmMetrics[0]+'","direction":"maximize"}],'+
    '"constraints":[{"metric":"'+wetFilmMetrics[1]+'","minimum":-0.0}],"required_consumers":["motion_wet_film"]}\r\n';
  const raw=matrix({maxEvaluations:64,cases:[{key:'cold',scenario:snapshot,comparison},{key:'static',scenario:independent}]}),parsed=JSON.parse(raw);
  for(const source of [film,history,motion,vaporCoverage,snapshot,independent,comparison])assert.ok(raw.includes(source));
  assert.equal(Object.hasOwn(parsed.cases[1].scenario,'motion'),false);assert.equal(Object.hasOwn(parsed.cases[1].scenario,'wet_film'),false);
  const unresolved=film.replace('a'.repeat(64),'stale-body').replace('[0.0000000000000001000000000000001]','[-1]');
  assert.ok(single({...input,motion:motion.replace(film,unresolved)}).includes(unresolved),
    'Body identity and finite-liquid semantic admission stay with the independent native history consumer');
  const empty=single({...input,motion:motion.replace(film,'null')});
  assert.equal(JSON.parse(empty).motion.vapor_coverage_history.wet_film,null);
  const available={surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true,motionWetFilm:true};
  const context=namedContext([{key:'cold',...available},{key:'warm',...available}]);
  assert.equal(JSON.parse(build({...state(),objectives:[{metric:wetFilmMetrics[0],direction:'maximize'}],design:namedDesign()},context)).design_adaptive.max_evaluations,2,
    'Optional geometry reservations are not multiplied across alternatives or probes by the authoring layer');
});

test('finite body-water criteria guide linked grids and all four adaptive modes without changing source policies', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const objectives=[{metric:wetFilmMetrics[0],direction:'maximize'},{metric:wetFilmMetrics[2],direction:'minimize'}];
  const input={...state(),basis:'reported_bounds',objectives:objectives.slice(0,1),
    constraints:[{metric:wetFilmMetrics[1],minimum:0}],requiredConsumers:['motion_wet_film']};
  const grid={...input,design:linkedDesignInput({values:[0,5],maxDesigns:1})},raw=build(grid,wetFilmContext());
  assert.equal(JSON.parse(raw).schema,'ftx-design-study/1');assert.ok(raw.includes(grid.design.fitSpec));assert.ok(raw.includes(grid.candidates[0].selection));
  const available={surface:false,motion:true,vaporCoverage:true,motionVaporHistory:true,motionWetFilm:true};
  const cases=namedContext([{key:'cold',...available},{key:'warm',...available}]),before=JSON.stringify(cases);
  for(const objective of ['consumer_comparison','consumer_pareto','consumer_worst_case','consumer_matrix_pareto']) {
    const design=namedDesign({objective});if(['consumer_worst_case','consumer_matrix_pareto'].includes(objective))delete design.caseKey;
    const candidate={...input,design,objectives:objective==='consumer_pareto'?objectives:objectives.slice(0,1)},result=JSON.parse(build(candidate,cases));
    assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.constraints,input.constraints);
    assert.deepEqual(result.comparison.required_consumers,['motion_wet_film']);assert.equal(JSON.stringify(cases),before);
  }
});

const textileVaporMetrics=['textile_water_final_vapor_pressure_pa','textile_water_min_sample_vapor_pressure_pa',
  'textile_water_max_sample_vapor_pressure_pa','textile_water_remaining_vapor_mass_kg','textile_water_isothermal_heat_removed_j'];
const textileWaterMetrics=['textile_water_evaporated_mass_kg','textile_water_remaining_mass_kg','textile_water_supplied_mass_kg',
  'textile_water_condensed_mass_kg','textile_water_drained_mass_kg','textile_water_latent_energy_j','textile_water_condensation_latent_energy_j',
  ...textileVaporMetrics];
const textileWaterContext=()=>({drape:true,surface:false,motion:true,transientSurface:true,textileWater:true});

test('textile-water criteria retain all twelve nominal decisions while sharing the existing transient surface consumer', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const input={...state(),objectives:textileWaterMetrics.map((metric,i)=>({metric,direction:i%2?'maximize':'minimize'})),
    constraints:textileWaterMetrics.map(metric=>({metric,minimum:0,maximum:1})),requiredConsumers:['transient_surface']};
  delete input.cutting;
  input.constraints.push({metric:'transient_surface_final_min_temperature_c',minimum:-5});
  const before=JSON.stringify(input),raw=build(input,textileWaterContext()),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.ok(raw.includes(input.candidates[0].selection));
  assert.deepEqual(result.comparison,{basis:'nominal',objectives:input.objectives,constraints:input.constraints,
    required_consumers:['transient_surface'],allow_mixed_protocols:false});
  for(const key of ['motion','surface_water','textileWater','transient_surface','textile_water'])assert.equal(Object.hasOwn(result,key),false);
  const dry={...state(),objectives:[{metric:'transient_surface_inside_energy_j',direction:'minimize'}],requiredConsumers:['transient_surface']};
  for(const context of [{drape:true,surface:false,motion:true,transientSurface:true},{...textileWaterContext(),textileWater:false}]) {
    assert.deepEqual(JSON.parse(build(dry,context)).comparison.required_consumers,['transient_surface'],
      'A dry transient heat requirement does not invent or require a water policy');
  }
  assert.throws(()=>build({...input,requiredConsumers:['textile_water']},textileWaterContext()),/Unsupported guided consumer: textile_water/);
});

test('finite-vapor criteria retain signed reservoir heat and guide existing material and design decisions', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const constraints=[{metric:textileVaporMetrics[4],minimum:-25,maximum:-1}],
    objectives=textileVaporMetrics.map(metric=>({metric,direction:'minimize'}));
  const input={...state(),objectives,constraints,requiredConsumers:['transient_surface']};
  assert.deepEqual(JSON.parse(build(input,textileWaterContext())).comparison.constraints,constraints);
  const cases=namedContext(['cold','warm'].map(key=>({key,surface:false,motion:true,transientSurface:true,textileWater:true})));
  for(const objective of ['consumer_comparison','consumer_pareto','consumer_worst_case','consumer_matrix_pareto']) {
    const design=namedDesign({objective});if(['consumer_worst_case','consumer_matrix_pareto'].includes(objective))delete design.caseKey;
    const goals=objective.includes('pareto')?objectives:objectives.slice(0,1);
    const result=JSON.parse(build({...input,objectives:goals,design},cases));
    assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.objectives,goals);
    assert.deepEqual(result.comparison.constraints,constraints);
  }
});

test('textile-water objectives and limits require actual water, motion and transient heat inputs together', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context=textileWaterContext();
  for(const metric of textileWaterMetrics)for(const policy of [
    {objectives:[{metric,direction:'maximize'}]},{constraints:[{metric,minimum:0,maximum:1}]}]) {
    const input={...state(),...policy};assert.ok(build(input,context));
    assert.throws(()=>build(input),/requested transient_surface/);
    for(const key of ['motion','transientSurface','textileWater']) {
      const absent={...context};delete absent[key];
      for(const missing of [absent,{...context,[key]:false}]) {
        assert.throws(()=>build(input,missing),key==='textileWater'?/Textile-water criteria/:/requested transient_surface/);
      }
    }
    assert.throws(()=>build(input,{...wetFilmContext(),transientSurface:true}),/Textile-water criteria/,
      'A finite body-water policy does not supply a textile-owned water policy');
    const inherited=Object.assign(Object.create({textileWater:true}),{drape:true,surface:false,motion:true,transientSurface:true});
    assert.throws(()=>build(input,inherited),/Textile-water criteria/);
  }
});

test('textile-water decisions remain nominal while independent body-water bounds and signed thermal guards retain their bases', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const context={...wetFilmContext(),...textileWaterContext()};
  const bounded={...state(),basis:'reported_bounds',objectives:[{metric:wetFilmMetrics[0],direction:'maximize'}]};
  for(const metric of textileWaterMetrics) {
    for(const input of [{...state(),basis:'reported_bounds',objectives:[{metric,direction:'minimize'}]},
      {...state(),constraints:[{metric,basis:'reported_bounds',maximum:1}]},
      {...bounded,constraints:[{metric,maximum:1}]}]) {
      assert.throws(()=>build(input,context),/reported-bound comparison supports/);
    }
    const input={...bounded,constraints:[{metric,basis:'nominal',minimum:0,maximum:1},
      {metric:'transient_surface_inside_energy_j',basis:'nominal',minimum:-20,maximum:-1}]};
    const result=JSON.parse(build(input,context));assert.equal(result.comparison.basis,'reported_bounds');
    assert.deepEqual(result.comparison.constraints,input.constraints);
    assert.throws(()=>build({...input,constraints:[{metric,basis:'nominal',minimum:2,maximum:1}]},context),/finite ordered/);
  }
});

test('named textile-water decisions require same-case water only for inherited water criteria and retain original overrides', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const complete={key:'cold',surface:false,motion:true,transientSurface:true,textileWater:true};
  for(const metric of textileWaterMetrics)for(const policy of [
    {objectives:[{metric,direction:'maximize'}]},{constraints:[{metric,maximum:1}]}]) {
    const input={...state(),...policy,design:namedDesign()};
    for(const flag of ['motion','transientSurface','textileWater']) {
      const missing={...complete,key:'warm',[flag]:false};
      assert.throws(()=>build(input,namedContext([complete,missing])),/warm inherits.*(transient_surface|textile-water)/);
      const original=namedContext([complete,{...missing,comparison:casePolicy()}]),before=JSON.stringify(original);
      assert.ok(build(input,original));assert.equal(JSON.stringify(original),before);
    }
  }
  const split=namedContext([{key:'dry heat',surface:false,motion:true,transientSurface:true},
    {key:'unrelated water',surface:false,textileWater:true,comparison:casePolicy()}]);
  assert.throws(()=>build({...state(),objectives:[{metric:textileWaterMetrics[0],direction:'maximize'}]},split),/Textile-water criteria/);
  const wetAndDry=namedContext([complete,{...complete,key:'warm',textileWater:false}]);
  assert.ok(build({...state(),requiredConsumers:['transient_surface']},wetAndDry),
    'Requiring complete heat results across cases does not make every case a wet case');
  const both=namedContext([complete,{...complete,key:'warm'}]),absent={...both};delete absent.textileWater;
  for(const context of [absent,{...both,textileWater:false}])assert.throws(()=>build(state(),context),/textileWater availability.*actual cases/);
  for(const value of [null,undefined,'true',1])assert.throws(()=>build(state(),namedContext([{...complete,textileWater:value}])),/availability must be a boolean/);
  const original=namedContext([{key:'cold',surface:false,comparison:casePolicy(textileWaterMetrics[0],'maximize',{
    required_consumers:['transient_surface'],allow_mixed_protocols:true})}]),before=JSON.stringify(original);
  assert.ok(build(state(),original));assert.equal(JSON.stringify(original),before,
    'An original comparison delegates physical, phase and source admission to the native consumer');
});

const textileMassMetrics=['textile_water_final_inertial_mass_kg','textile_water_max_sample_inertial_mass_kg'];
const textileMassContext=()=>({...textileWaterContext(),textileWaterMechanics:true});

test('wet inertial-mass criteria require an explicit direct or paired mechanical policy', async () => {
  const {hasTextileWaterMechanicsPolicy:mechanical,buildMaterialStudyRequest:build}=await modulePromise;
  for(const surface of [undefined,null,{}, {surface_water:{}}, {mechanical_coupling:{}},
    {surface_water_alternatives:[{}]}, {surface_water:{},mechanical_coupling:null}])assert.equal(mechanical(surface),false);
  for(const surface of [{surface_water:{},mechanical_coupling:{}},{coupled_water_alternatives:[{}]}])assert.equal(mechanical(surface),true);
  for(const metric of textileMassMetrics)for(const decision of [
    {objectives:[{metric,direction:'minimize'}]}, {constraints:[{metric,minimum:0,maximum:1}]}]) {
    const input={...state(),...decision};
    assert.ok(build(input,textileMassContext()));
    for(const context of [textileWaterContext(),{...textileMassContext(),textileWaterMechanics:false},
      Object.assign(Object.create({textileWaterMechanics:true}),textileWaterContext())]) {
      assert.throws(()=>build(input,context),/Wet inertial-mass criteria/);
    }
    for(const key of ['motion','transientSurface','textileWater']) {
      assert.throws(()=>build(input,{...textileMassContext(),[key]:false}),/transient_surface|Textile-water criteria/);
    }
    for(const value of [null,undefined,1,'true']) {
      assert.throws(()=>build(input,{...textileMassContext(),textileWaterMechanics:value}),/availability must be a boolean/);
    }
  }
});

test('wet inertial-mass goals and inclusive guards stay nominal across named adaptive decisions', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const objectives=textileMassMetrics.map(metric=>({metric,direction:'minimize'}));
  const constraints=textileMassMetrics.map(metric=>({metric,minimum:0,maximum:0.5}));
  const input={...state(),objectives,constraints,requiredConsumers:['transient_surface']},before=JSON.stringify(input);
  const single=JSON.parse(build(input,textileMassContext()));
  assert.deepEqual(single.comparison.objectives,objectives);assert.deepEqual(single.comparison.constraints,constraints);
  assert.equal(JSON.stringify(input),before);assert.equal(Object.hasOwn(single,'textileWaterMechanics'),false);
  const available={surface:false,motion:true,transientSurface:true,textileWater:true,textileWaterMechanics:true};
  const cases=namedContext([{key:'cold',...available},{key:'warm',...available}]),original=JSON.stringify(cases);
  for(const objective of ['consumer_comparison','consumer_pareto','consumer_worst_case','consumer_matrix_pareto']) {
    const design=namedDesign({objective});if(['consumer_worst_case','consumer_matrix_pareto'].includes(objective))delete design.caseKey;
    const goals=objective.includes('pareto')?objectives:objectives.slice(0,1);
    const result=JSON.parse(build({...input,objectives:goals,design},cases));
    assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.objectives,goals);
    assert.deepEqual(result.comparison.constraints,constraints);
  }
  assert.equal(JSON.stringify(cases),original);
  for(const metric of textileMassMetrics) {
    assert.throws(()=>build({...state(),basis:'reported_bounds',objectives:[{metric,direction:'minimize'}]},textileMassContext()),/reported-bound comparison/);
    assert.throws(()=>build({...state(),constraints:[{metric,maximum:1,basis:'reported_bounds'}]},textileMassContext()),/reported-bound comparison/);
    const mixed={...state(),assemblyMass:mass(),basis:'reported_bounds',objectives:[{metric:'assembly_mass_kg',direction:'minimize'}],
      constraints:[{metric,minimum:0,maximum:0.5,basis:'nominal'}]};
    assert.deepEqual(JSON.parse(build(mixed,textileMassContext())).comparison.constraints,mixed.constraints);
  }
});

test('wet mass cannot borrow mechanical qualification from another named case', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const coupled={key:'cold',surface:false,motion:true,transientSurface:true,textileWater:true,textileWaterMechanics:true};
  for(const metric of textileMassMetrics)for(const decision of [
    {objectives:[{metric,direction:'minimize'}]}, {constraints:[{metric,maximum:1}]}]) {
    const input={...state(),...decision,design:namedDesign()},uncoupled={...coupled,key:'warm',textileWaterMechanics:false};
    assert.throws(()=>build(input,namedContext([coupled,uncoupled])),/warm inherits.*mechanical coupling/);
    const explicit=namedContext([coupled,{...uncoupled,comparison:casePolicy()}]),before=JSON.stringify(explicit);
    assert.ok(build(input,explicit));assert.equal(JSON.stringify(explicit),before);
  }
  const input={...state(),objectives:[{metric:textileMassMetrics[0],direction:'minimize'}]};
  const split=namedContext([{...coupled,textileWaterMechanics:false},
    {key:'unrelated mechanics',surface:false,textileWaterMechanics:true,comparison:casePolicy()}]);
  assert.throws(()=>build(input,split),/Wet inertial-mass criteria/);
  const context=namedContext([coupled]),missing={...context};delete missing.textileWaterMechanics;
  assert.throws(()=>build(state(),missing),/textileWaterMechanics availability.*actual cases/);
});

test('original textile-water and mechanical-coupling sources survive snapshots without inferred source qualification or optional-budget gates', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix,buildMaterialStudyRequest:build}=await modulePromise;
  const water=' {\r\n"schema":"ftx-textile-surface-water/1","model":"inner_cell_equilibrium_free_film_frozen_properties/1",'+
    '"bindings":{"band":{"material_sha256":"'+'a'.repeat(64)+'","initial_liquid_kg_m2":0.0000000000000001000000000000001}},'+
    '"replenishment_kg_m2_s":-0.0,"source":{"reference":"synthetic original \\u03bc","accessed_on":"2026-01-01"}}\r\n';
  const coupling=' {"schema":"ftx-evaporating-liquid-mechanics/1","qualification":"original source qualification \\u03bc"}\r\n';
  const surface=' {"schema":"ftx-motion-surface-heat/1","max_cell_steps":64000000,"surface_water":'+water+',"mechanical_coupling":'+coupling+'}\r\n';
  const motion=' \r\n'+JSON.stringify(motionPolicy()).slice(0,-1)+',"transient_surface":'+surface+'}\r\n';
  const input={...scenarioState(),motion,maxMotionTrials:64,maxVertexFrames:32000000},before=JSON.stringify(input);
  const snapshot=single(input),dry=single({...scenarioState(),motion:JSON.stringify(motionPolicy({transient_surface:{schema:'ftx-motion-surface-heat/1'}})),
    maxMotionTrials:64,maxVertexFrames:32000000});
  const comparison=' {"basis":"nominal","objectives":[{"metric":"'+textileWaterMetrics[0]+'","direction":"maximize"}],'+
    '"constraints":[{"metric":"'+textileWaterMetrics[1]+'","minimum":-0.0}],"required_consumers":["transient_surface"]}\r\n';
  const raw=matrix({maxEvaluations:64,cases:[{key:'wet',scenario:snapshot,comparison},{key:'dry',scenario:dry}]}),parsed=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);
  for(const source of [water,coupling,surface,motion,snapshot,dry,comparison])assert.ok(raw.includes(source));
  assert.equal(Object.hasOwn(parsed.cases[1].scenario.motion.transient_surface,'surface_water'),false);
  for(const item of parsed.cases)for(const object of [item,item.scenario])assert.equal(Object.hasOwn(object,'textileWater'),false);
  const unresolved=water.replace('a'.repeat(64),'stale-material').replace('0.0000000000000001000000000000001','-1');
  assert.ok(single({...input,motion:motion.replace(water,unresolved)}).includes(unresolved),
    'These opaque source-preservation inputs are not physical fixtures: complete policy and source qualification stay native');
  assert.equal(JSON.parse(single({...input,motion:motion.replace(water,'null')})).motion.transient_surface.surface_water,null);
  const available={surface:false,motion:true,transientSurface:true,textileWater:true};
  const context=namedContext([{key:'cold',...available},{key:'warm',...available}]);
  assert.equal(JSON.parse(build({...state(),objectives:[{metric:textileWaterMetrics[0],direction:'maximize'}],design:namedDesign()},context)).design_adaptive.max_evaluations,2,
    'Optional cell-step reservations stay with the native consumer across probes and retained alternatives');
});

test('named scenario authoring retains water qualification collections without replacing material sources', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix}=await modulePromise;
  const alternatives=' [ {"schema":"ftx-textile-surface-water/1","bindings":{"panel":'+
    '{"material_sha256":"'+'a'.repeat(64)+'","initial_liquid_kg_m2":-0.0}},"source":{"n":18446744073709551615}},'+
    '{"schema":"ftx-textile-surface-water/1","bindings":{"panel":{"material_sha256":"'+'b'.repeat(64)+'",'+
    '"initial_liquid_kg_m2":-0.0}},"source":{"n":18446744073709551615}} ]\r\n';
  const motion=JSON.stringify(motionPolicy()).slice(0,-1)+',"transient_surface":'+
    '{"schema":"ftx-motion-surface-heat/1","surface_water_alternatives":'+alternatives+'}}';
  const scenario=single({...scenarioState(),motion,maxMotionTrials:64,maxVertexFrames:32000000});
  const comparison=JSON.stringify({basis:'nominal',objectives:[{metric:textileWaterMetrics[0],direction:'maximize'}],
    constraints:[],required_consumers:['transient_surface']});
  const raw=matrix({maxEvaluations:2,cases:[{key:'first',scenario,comparison},{key:'second',scenario,comparison}]});
  assert.ok(raw.includes(alternatives));assert.ok(raw.includes(motion));assert.ok(raw.includes(scenario));
  for(const item of JSON.parse(raw).cases) {
    assert.equal(item.scenario.motion.transient_surface.surface_water_alternatives.length,2);
    assert.equal(Object.hasOwn(item.scenario.motion.transient_surface,'surface_water'),false);
  }
});

test('named scenario authoring preserves paired water and mechanical qualifications exactly', async () => {
  const {buildMaterialScenarioRequest:single,buildMaterialScenarioMatrixRequest:matrix,hasTextileWaterPolicy}=await modulePromise;
  // Source transport only: native consumers validate each complete physical qualification.
  const pair=' {"surface_water":{"bindings":{"panel":{"material_sha256":"'+'a'.repeat(64)+
    '","initial_liquid_kg_m2":-0.0}}},"mechanical_coupling":{"material_report_sha256":"'+'b'.repeat(64)+
    '","source":{"reference":"test:paired café 🧵","n":18446744073709551615}}}\r\n';
  const alternatives=' [\r\n'+pair+' ]\r\n';
  const motion=JSON.stringify(motionPolicy()).slice(0,-1)+',"transient_surface":'+
    '{"schema":"ftx-motion-surface-heat/1","coupled_water_alternatives":'+alternatives+'}}';
  const scenario=single({...scenarioState(),motion,maxMotionTrials:64,maxVertexFrames:32000000});
  const comparison=JSON.stringify({basis:'nominal',objectives:[{metric:textileWaterMetrics[0],direction:'maximize'}],
    constraints:[],required_consumers:['motion','transient_surface']});
  const raw=matrix({maxEvaluations:2,cases:[{key:'first',scenario,comparison},{key:'second',scenario,comparison}]});
  assert.ok(raw.includes(pair));assert.ok(raw.includes(alternatives));assert.ok(raw.includes(motion));
  for(const item of JSON.parse(raw).cases) {
    const surface=item.scenario.motion.transient_surface;
    assert.equal(hasTextileWaterPolicy(surface),true);
    assert.equal(surface.coupled_water_alternatives.length,1);
    for(const name of ['surface_water','surface_water_alternatives','mechanical_coupling'])
      assert.equal(Object.hasOwn(surface,name),false,name);
  }
  assert.equal(hasTextileWaterPolicy({coupled_water_alternatives:null}),false);
});

test('textile-water criteria guide linked grids and all four adaptive modes while retaining exact source inputs', async () => {
  const {buildMaterialStudyRequest:build}=await modulePromise;
  const objectives=[{metric:textileWaterMetrics[0],direction:'maximize'},{metric:textileWaterMetrics[1],direction:'minimize'}];
  const input={...state(),objectives:objectives.slice(0,1),constraints:[{metric:textileWaterMetrics[4],maximum:1}],requiredConsumers:['transient_surface']};
  const grid={...input,design:linkedDesignInput({values:[0,5],maxDesigns:1})},raw=build(grid,textileWaterContext());
  assert.equal(JSON.parse(raw).schema,'ftx-design-study/1');assert.ok(raw.includes(grid.design.fitSpec));assert.ok(raw.includes(grid.candidates[0].selection));
  const available={surface:false,motion:true,transientSurface:true,textileWater:true};
  const cases=namedContext([{key:'cold',...available},{key:'warm',...available}]),before=JSON.stringify(cases);
  for(const objective of ['consumer_comparison','consumer_pareto','consumer_worst_case','consumer_matrix_pareto']) {
    const design=namedDesign({objective});if(['consumer_worst_case','consumer_matrix_pareto'].includes(objective))delete design.caseKey;
    const candidate={...input,design,objectives:objective==='consumer_pareto'?objectives:objectives.slice(0,1)},result=JSON.parse(build(candidate,cases));
    assert.equal(result.design_adaptive.objective,objective);assert.deepEqual(result.comparison.constraints,input.constraints);
    assert.deepEqual(result.comparison.required_consumers,['transient_surface']);assert.equal(JSON.stringify(cases),before);
  }
});

const workbenchPattern = ' {"schema":"fabric-pattern/1","pieces":[],"source":{"seed":18446744073709551615,"x":-0.0,"scale":1.0,"label":"é 🧵"}}\n';
const workbenchModel = () => ({sha256:'a'.repeat(64),byte_length:3});
const workbenchBody = ' {"schema":"fabric-measurements/1","source":{"seed":18446744073709551615,"x":-0.0,"label":"🧵"}}\n';
const workbenchRequest = () => ({type:'study',pattern:workbenchPattern,
  studyRequest:' {"schema":"ftx-material-study/1","source":{"scale":1.0,"seed":18446744073709551615}}\n',onBody:false});

test('saved workbench studies round-trip exact original source text without results or model bytes', async () => {
  const {buildWorkbenchStudyBundle:build,parseWorkbenchStudyBundle:parse,MAX_WORKBENCH_STUDY_BYTES}=await modulePromise;
  const input=workbenchRequest(),before=JSON.stringify(input),raw=build(input),saved=parse(raw);
  assert.equal(MAX_WORKBENCH_STUDY_BYTES,128*1024*1024);
  assert.equal(JSON.stringify(input),before);assert.equal(Object.isFrozen(input),false);
  assert.deepEqual(JSON.parse(raw),{schema:'ftx-workbench-study/1',request:input});
  assert.equal(saved.request.pattern,input.pattern);assert.equal(saved.request.studyRequest,input.studyRequest);
  assert.equal(Object.isFrozen(saved),true);assert.equal(Object.isFrozen(saved.request),true);
  assert.throws(()=>Object.defineProperty(saved.request,'pattern',{value:'replacement'}));
  input.pattern='later edit';assert.equal(saved.request.pattern,workbenchPattern);
  for(const key of ['request','bytes','result','report'])assert.equal(Object.hasOwn(saved.request,key),false,key);
  assert.equal(Object.hasOwn(saved,'body_model'),false);
});

test('saved body-dependent manufacturing, design and named scenario studies require a bounded exact model identity', async () => {
  const {buildWorkbenchStudyBundle:build,parseWorkbenchStudyBundle:parse}=await modulePromise;
  const design=workbenchRequest().studyRequest.replace('ftx-material-study/1','ftx-design-study/1');
  const requests=[{...workbenchRequest(),onBody:true,studyRequest:design,measurements:workbenchBody},
    ...['ftx-drape-scenario/1','ftx-drape-scenario-matrix/1'].map(schema=>({type:'scenario',pattern:workbenchPattern,
      studyRequest:design,scenarioRequest:' {"schema":"'+schema+'","source":1.0}\n',measurements:'{}'}))];
  for(const request of requests) {
    assert.throws(()=>build(request),/body model identity/i);
    const model=workbenchModel(),saved=parse(build(request,model));
    assert.deepEqual(JSON.parse(JSON.stringify(saved.request)),request);
    assert.deepEqual(JSON.parse(JSON.stringify(saved.body_model)),model);assert.ok(Object.isFrozen(saved.body_model));
    model.sha256='b'.repeat(64);assert.equal(saved.body_model.sha256,'a'.repeat(64));
    for(const invalid of [{...model,sha256:'A'.repeat(64)},{...model,sha256:'a'},
      {...model,byte_length:0},{...model,byte_length:1.5},{...model,byte_length:256*1024*1024+1},
      {...model,bytes:[1,2,3]}])assert.throws(()=>build(request,invalid),/body model identity/i);
  }
  assert.throws(()=>build({...requests[1],onBody:false},workbenchModel()),/Only manufacturing/);
  assert.throws(()=>build(workbenchRequest(),workbenchModel()),/unused body/);
});

test('saved study supplemental fusion, preview pose, accepted fit and outfit provenance retain unfused originals', async () => {
  const {buildWorkbenchStudyBundle:build,parseWorkbenchStudyBundle:parse}=await modulePromise;
  const extra=' {"schema":"fabric-measurements/1","source":{"value":1.0,"seed":18446744073709551615}}\n';
  const outfit={request_json:' {"schema":"ftx-outfit-drape/1","source":-0.0}\n',
    composition_json:' {"schema":"ftx-outfit-composition/1","source":1.0}\n'};
  const request={...workbenchRequest(),onBody:true,measurements:workbenchBody,measurementSources:JSON.stringify([extra]),
    measurementConsent:true,measurementPrimary:workbenchBody,outfit};
  const pose={arm_abduction_deg:35,legs:'squat',leg_fraction:0.25};
  const accepted=' {"schema":"ftx-body-fit/1","source":{"seed":18446744073709551615,"scale":1.0}}\n';
  for(const metadata of [{},{measurementPose:pose},{measurementAcceptedFit:accepted,measurementArmDeg:35}]) {
    const input={...request,...metadata},saved=parse(build(input,workbenchModel()));
    assert.deepEqual(JSON.parse(JSON.stringify(saved.request)),input);
    assert.equal(saved.request.measurementSources,JSON.stringify([extra]));
    assert.equal(JSON.parse(saved.request.measurementSources)[0],extra);
    assert.ok(Object.isFrozen(saved.request.outfit));
    if(saved.request.measurementPose)assert.ok(Object.isFrozen(saved.request.measurementPose));
  }
  const saved=parse(build({...request,measurementPose:pose},workbenchModel()));
  pose.legs='seated';outfit.request_json='later edit';
  assert.equal(saved.request.measurementPose.legs,'squat');assert.match(saved.request.outfit.request_json,/ftx-outfit-drape\/1/);
});

test('saved study imports refuse unknown transport fields, malformed sources, unrelated bodies and unsupported schemas', async () => {
  const {buildWorkbenchStudyBundle:build,parseWorkbenchStudyBundle:parse}=await modulePromise;
  const input=workbenchRequest(),raw=build(input);
  for(const invalid of ['',null,'\ufeff'+raw,'[]','null','{"schema":"ftx-workbench-study/2"}',
    JSON.stringify({...JSON.parse(raw),report:{}})])assert.throws(()=>parse(invalid));
  for(const extension of [{type:'exports'},{request:99},{bytes:[1,2,3]},{result:{}},{onBody:'true'},
    {measurements:'{}'},{scenarioRequest:'{"schema":"ftx-drape-scenario/1"}'},
    {pattern:'{}'},{pattern:'[]'},{pattern:'{broken'},{studyRequest:'{"schema":"other/1"}'},
    {outfit:{request_json:'{}',composition_json:'{}'}},{outfit:{request_json:'{}',composition_json:'{}',report:{}}}]) {
    assert.throws(()=>build({...input,...extension}));
  }
  assert.throws(()=>build({type:'scenario',pattern:workbenchPattern,studyRequest:input.studyRequest,
    scenarioRequest:'{"schema":"ftx-material-scenario/1"}',measurements:'{}'},workbenchModel()),/scenario request.*schema/);
  assert.equal(parse(raw).request.pattern,workbenchPattern,'A refused import cannot mutate an earlier replay');
});

test('saved supplemental replay requires consent and keeps preview and accepted-fit metadata mutually exclusive', async () => {
  const {buildWorkbenchStudyBundle:build}=await modulePromise;
  const pose={arm_abduction_deg:35,legs:'standing',leg_fraction:1};
  const request={...workbenchRequest(),onBody:true,measurements:workbenchBody,measurementSources:JSON.stringify([workbenchBody]),
    measurementConsent:true,measurementPrimary:workbenchBody};
  const fit='{"schema":"ftx-body-fit/1"}';
  for(const extension of [{measurementConsent:false},{measurementSources:'[]'},
    {measurementSources:JSON.stringify(Array(9).fill('{}'))},{measurementSources:'[{}]'},
    {measurementSources:'["null"]'},{measurementSources:'[broken'},
    {measurementPose:{...pose,legs:'unknown'}},{measurementPose:{...pose,arm_abduction_deg:181}},
    {measurementPose:{...pose,leg_fraction:-1}},{measurementPose:{...pose,unexpected:true}},
    {measurementPose:pose,measurementAcceptedFit:fit,measurementArmDeg:35},
    {measurementAcceptedFit:fit},{measurementAcceptedFit:'{}',measurementArmDeg:35},
    {measurementArmDeg:35}])assert.throws(()=>build({...request,...extension},workbenchModel()));
  const noPrimary={...request,measurementPose:pose};delete noPrimary.measurementPrimary;
  assert.throws(()=>build(noPrimary,workbenchModel()),/primary measurements/);
  const noSources={...request};delete noSources.measurementSources;
  assert.throws(()=>build(noSources,workbenchModel()),/requires original additional/);
});

test('saved study limits count UTF-8 source bytes and encoded files while retaining large original-file requests', async () => {
  const {buildWorkbenchStudyBundle:build,parseWorkbenchStudyBundle:parse,MAX_WORKBENCH_STUDY_BYTES}=await modulePromise;
  const input=workbenchRequest();
  const largeStudy='{"schema":"ftx-material-study/1","source":"'+'x'.repeat(16*1024*1024)+'"}';
  assert.equal(parse(build({...input,studyRequest:largeStudy})).request.studyRequest,largeStudy,
    'The guided 16 MiB authoring cap must not restrict an existing original-file request within the worker budget');
  const utf8='é'.repeat(16*1024*1024);
  const largePattern='{"schema":"fabric-pattern/1","source":"'+utf8+'"}';
  const combined='{"schema":"ftx-material-study/1","source":"'+utf8+'"}';
  assert.throws(()=>build({...input,pattern:largePattern,studyRequest:combined}),/64 MiB/);
  const extra='{"schema":"fabric-measurements/1","source":"'+'é'.repeat(8*1024*1024)+'"}';
  assert.throws(()=>build({...input,onBody:true,measurements:'{}',measurementConsent:true,
    measurementSources:JSON.stringify([extra])},workbenchModel()),/Additional measurements exceed 16 MiB/);
  assert.throws(()=>parse(' '.repeat(MAX_WORKBENCH_STUDY_BYTES+1)),/128 MiB/);
});

test('authored fit targets preserve explicit native point kinds, signed bands and body transformations',async()=>{
  const {buildGuidedFitSpec:build,GUIDED_FIT_BODY_MEASUREMENTS:names}=await modulePromise;
  const base={body:'stature',body_scale:0.1,body_offset_mm:-3,target_mm:[-2,0,4]};
  const points=[
    {...base,id:'length <β>',label:'Explicit synthetic target',kind:'vertical',instance:'front.R',from:'a',to:'d'},
    {...base,id:'width-at-vertex',kind:'width',instance:'sleeve.L',at_vertex:'cap',dy:-2},
    {...base,id:'width-at-landmark',kind:'width',instance:'sleeve.R',landmark:'elbow',dy:3},
    {...base,id:'circumference',kind:'girth',instances:['front.R','front.L','back'],landmark:'chest',dy:-5},
    {...base,id:'neckline',kind:'path',refs:['front.R/neck','front.L/neck+shoulder'],scale:0.5},
  ];
  const original=JSON.stringify(points),raw=build(points),result=JSON.parse(raw);
  assert.deepEqual(result,{schema:'fabric-fit/1',points});assert.equal(JSON.stringify(points),original);
  const rust=fs.readFileSync(path.join(__dirname,'../crates/ftx-body/src/fit.rs'),'utf8');
  const canonical=[.../pub const TAPE_FIELDS:[\s\S]*?= \[([\s\S]*?)\];/.exec(rust)[1].matchAll(/"([^"]+)"/g)].map(m=>m[1]);
  assert.deepEqual([...names],canonical,'Guided choices are canonical model tape readings in millimetres');
  for(const body of names)assert.equal(JSON.parse(build([{...points[0],body}])).points[0].body,body);
  points[0].target_mm[0]=-99;assert.equal(JSON.parse(raw).points[0].target_mm[0],-2);
});

test('authored fit targets reject incomplete bands, unknown anatomy, ambiguous references and oversized contracts',async()=>{
  const {buildGuidedFitSpec:build}=await modulePromise;
  const point={id:'length',kind:'vertical',instance:'front',from:'a',to:'d',body:'stature',target_mm:[-1,0,1]};
  for(const points of [[],Array(1),[null],[point,point],Array.from({length:33},(_,i)=>({...point,id:String(i)}))])assert.throws(()=>build(points));
  for(const change of [{id:' '},{kind:'unknown'},{body:'shoulder_slope_deg'},{body:'invented'},
    {target_mm:[]},{target_mm:[0,1]},{target_mm:[0,1,2,3]},{target_mm:[0,-1,2]},
    {target_mm:[0,1,0]},{target_mm:[0,NaN,2]},{target_mm:[0,'1',2]},{target_mm:Array(3)},
    {body_scale:Infinity},{body_offset_mm:'2'},{instance:''},{from:''},{to:''},{dy:1},{label:''}]) {
    assert.throws(()=>build([{...point,...change}]),JSON.stringify(change));
  }
  const width={id:'width',kind:'width',instance:'front',body:'chest',target_mm:[0,0,0]};
  for(const change of [{},{at_vertex:'a',landmark:'chest'},{at_vertex:''},{landmark:null},{at_vertex:'a',dy:NaN}]) {
    assert.throws(()=>build([{...width,...change}]));
  }
  const pathPoint={id:'path',kind:'path',body:'neck_base',target_mm:[-10,-5,0],refs:['front/neck']};
  for(const change of [{refs:[]},{refs:['']},{refs:[null]},{refs:Array(1)},{refs:Array(65).fill('front/neck')},{scale:Infinity}]) {
    assert.throws(()=>build([{...pathPoint,...change}]));
  }
  assert.throws(()=>build([{...pathPoint,kind:'girth',refs:undefined,instances:['front'],landmark:'chest'}]));
  assert.throws(()=>build([{...point,label:'🧵'.repeat(65536)}]),/256 KiB/);
  assert.equal(JSON.parse(build(Array.from({length:32},(_,i)=>({...point,id:String(i)})))).points.length,32);
});

test('authored fit contracts enter existing linked and adaptive studies without numeric or target substitution',async()=>{
  const {buildGuidedFitSpec:fit,buildMaterialStudyRequest:build}=await modulePromise;
  const raw=fit([{id:'explicit length',kind:'vertical',instance:'front',from:'a',to:'d',body:'stature',
    body_scale:0.1,body_offset_mm:0,target_mm:[-0.2,0,0.2]}]);
  const input=state();input.design=linkedDesignInput();input.design.fitSpec=raw;
  const request=build(input);assert.ok(request.includes(raw));
  assert.deepEqual(JSON.parse(request).design_fit.fit_spec,JSON.parse(raw));
  input.design={...namedDesign(),fitSpec:raw};
  const adaptive=build(input,namedContext(undefined,12));assert.ok(adaptive.includes(raw));
  assert.deepEqual(JSON.parse(adaptive).design_fit.fit_spec,JSON.parse(raw));
});

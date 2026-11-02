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
  assert.throws(()=>build({...absent,assemblyMass:mass()}),/Stock objectives require/);
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

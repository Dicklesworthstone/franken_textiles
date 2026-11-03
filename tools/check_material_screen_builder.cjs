// Request authoring only; the existing native screen owns material admission and the actual frontier.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const modulePromise = (async () => {
  const module = new vm.SourceTextModule(fs.readFileSync(path.join(__dirname, '../web/materials-screen-builder.js'), 'utf8'));
  await module.link(() => { throw Error('The builder must not import dependencies.'); });
  await module.evaluate();
  return module.namespace;
})();
const state = changes => ({query:'',forms:[],basis:'nominal',allowCatalogEstimates:false,allowMixedProtocols:false,
  requiredCapabilities:[],attributes:[],...changes});
const attribute = changes => ({key:'areal_density',unit:'kg/m2',maximum:0.3,direction:'minimize',...changes});

test('guided catalog discovery retains explicit form, construction query and compiler choices without inventing a score', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const input=state({query:' wool single tuck ',forms:['knit','woven'],basis:'reported_bounds',
    requiredCapabilities:['mechanics','steady_conduction']}),before=JSON.stringify(input),result=JSON.parse(build(input));
  assert.equal(JSON.stringify(input),before);
  assert.deepEqual(result,{schema:'ftx-material-screen/1',basis:'reported_bounds',query:' wool single tuck ',
    forms:['knit','woven'],allow_catalog_estimates:false,allow_mixed_protocols:false,
    required_capabilities:['mechanics','steady_conduction'],constraints:[],objectives:[]});
  for(const key of ['candidates','selection','selection_json','score','physical_validation_claim'])assert.equal(Object.hasOwn(result,key),false);
  assert.equal(build({basis:'nominal',attributes:[]}),build(state()),'Omitted filters and consents retain native discovery defaults');
});

test('ordinary insulation choices produce the existing source-qualified Thinsulate screening request', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const fixture=JSON.parse(fs.readFileSync(path.join(__dirname,'../tests/fixtures/materials/screen-insulation.json'),'utf8'));
  const input=state({query:'thinsulate',forms:['nonwoven','fill'],requiredCapabilities:['steady_conduction'],attributes:[
    attribute(),{key:'thickness',unit:'m',maximum:0.02},{key:'thermal_resistance',unit:'m2*K/W',direction:'maximize'},
  ]});
  assert.deepEqual(JSON.parse(build(input)),{...fixture,allow_catalog_estimates:false,allow_mixed_protocols:false});
});

test('attribute rows keep both inclusive bounds and their independent objective in authored order', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const input=state({attributes:[attribute({minimum:0.1}),
    {key:'shrinkage_warp',unit:'1',minimum:-0.1,maximum:-0.01,direction:'maximize'},
    {key:'width',unit:'m',minimum:1.2},{key:'custom response',unit:'N/custom',direction:'minimize'}]});
  const result=JSON.parse(build(input));
  assert.deepEqual(result.constraints,[{attribute:{key:'areal_density',unit:'kg/m2'},minimum:0.1,maximum:0.3},
    {attribute:{key:'shrinkage_warp',unit:'1'},minimum:-0.1,maximum:-0.01},
    {attribute:{key:'width',unit:'m'},minimum:1.2}]);
  assert.deepEqual(result.objectives,[{attribute:{key:'areal_density',unit:'kg/m2'},direction:'minimize'},
    {attribute:{key:'shrinkage_warp',unit:'1'},direction:'maximize'},
    {attribute:{key:'custom response',unit:'N/custom'},direction:'minimize'}]);
});

test('exact method and original condition qualifiers survive both native clauses without normalization', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const conditions=' \r\n{"\\u0061ir_temperature_c":"20.0","humidity":"065","specimen":" linen 🧵 ","__proto__":"exact"}\r\n';
  const input=state({attributes:[attribute({method:' ASTM D3776 ',conditions})]}),before=JSON.stringify(input),raw=build(input),result=JSON.parse(raw);
  assert.equal(JSON.stringify(input),before);assert.equal(raw.split(conditions).length-1,2);
  assert.deepEqual(result.constraints[0].attribute,{key:'areal_density',unit:'kg/m2',method:' ASTM D3776 ',conditions:JSON.parse(conditions)});
  assert.deepEqual(result.objectives[0].attribute,result.constraints[0].attribute);
  const custom=JSON.parse(build(state({attributes:[{key:'__proto__',unit:'custom/1 ',minimum:0,conditions:' {}\r\n'}]})));
  assert.equal(custom.constraints[0].attribute.key,'__proto__');assert.equal(custom.constraints[0].attribute.unit,'custom/1 ');
  const absent=JSON.parse(build(state({attributes:[attribute()]}))).constraints[0].attribute;
  assert.equal(Object.hasOwn(absent,'method'),false);assert.equal(Object.hasOwn(absent,'conditions'),false);
  assert.equal(JSON.parse(build(state({attributes:[attribute({unit:'g/m2'})]}))).constraints[0].attribute.unit,'g/m2',
    'Authoring never converts a unit or replaces the native registry admission');
});

test('estimate and cross-protocol permission require independent explicit booleans and ignore inherited properties', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  for(const key of ['allowCatalogEstimates','allowMixedProtocols']) {
    const field=key==='allowCatalogEstimates'?'allow_catalog_estimates':'allow_mixed_protocols';
    for(const value of [true,false])assert.equal(JSON.parse(build(state({[key]:value})))[field],value);
    for(const value of [undefined,null,1,0,'true','false',{}])assert.throws(()=>build(state({[key]:value})),/consent must be a boolean/);
  }
  const inherited=Object.assign(Object.create({allowCatalogEstimates:true,allowMixedProtocols:true,
    query:'silently added',forms:['knit'],requiredCapabilities:['mechanics']}),{basis:'nominal',attributes:[]});
  assert.equal(build(inherited),build(state()));
  const row=Object.assign(Object.create({method:'inherited',conditions:'{"hidden":"filter"}',minimum:99,direction:'maximize'}),
    {key:'width',unit:'m',maximum:2});
  assert.deepEqual(JSON.parse(build(state({attributes:[row]}))).constraints,[{attribute:{key:'width',unit:'m'},maximum:2}]);
  assert.throws(()=>build(state({attributes:[Object.create(attribute())]})),/missing/);
  assert.throws(()=>build(Object.create(state())),/missing/);
});

test('all native material forms and independent compiler capabilities are available with exact enum identities', async()=>{
  const {buildMaterialScreenRequest:build,SCREEN_FORMS,SCREEN_CAPABILITIES}=await modulePromise;
  const forms=['sheet','fiber','yarn','woven','knit','nonwoven','hide','fill','laminate'];
  const requiredCapabilities=['mechanics','steady_conduction','transient_thermal'];
  assert.deepEqual(Array.from(SCREEN_FORMS),forms);assert.deepEqual(Array.from(SCREEN_CAPABILITIES),requiredCapabilities);
  assert.deepEqual(JSON.parse(build(state({forms,requiredCapabilities}))).forms,forms);
  assert.deepEqual(JSON.parse(build(state({forms,requiredCapabilities}))).required_capabilities,requiredCapabilities);
  for(const [key,valid] of [['forms','knit'],['requiredCapabilities','mechanics']]) {
    for(const value of [null,undefined,{},valid,[valid,valid],[valid.toUpperCase()],[' '+valid],[1]]) {
      assert.throws(()=>build(state({[key]:value})),/supported values/);
    }
  }
});

test('malformed discovery and attribute fields refuse rather than silently becoming an unrestricted search', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  for(const input of [null,[],{}, {...state(),unknown:1},{...state(),candidates:[]},
    {...state(),basis:undefined},{...state(),basis:'automatic'},{...state(),attributes:null},{...state(),attributes:{}},
    {...state(),query:undefined},{...state(),query:null},{...state(),query:1}])assert.throws(()=>build(input));
  for(const row of [null,[],{},attribute({unknown:true}),{unit:'kg/m2',minimum:0},{key:'density',maximum:1},
    {key:'areal_density',unit:'kg/m2'},attribute({key:''}),attribute({key:' \t'}),attribute({unit:''}),
    attribute({unit:1}),attribute({key:null})])assert.throws(()=>build(state({attributes:[row]})));
  const sparse=Array(1);assert.throws(()=>build(state({attributes:sparse})),/must be an object/);
});

test('native numerical boundaries remain finite, inclusive and signed without coercion or a guessed physical range', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  for(const key of ['minimum','maximum'])for(const value of [undefined,null,NaN,Infinity,-Infinity,'0',false,[]]) {
    assert.throws(()=>build(state({attributes:[attribute({[key]:value})]})),/finite numbers/);
  }
  assert.throws(()=>build(state({attributes:[attribute({minimum:0.4,maximum:0.3})]})),/ordered inclusive/);
  const equal=JSON.parse(build(state({attributes:[attribute({minimum:-3,maximum:-3})]}))).constraints[0];
  assert.equal(equal.minimum,-3);assert.equal(equal.maximum,-3);
  for(const direction of [undefined,null,'','lowest','MAXIMIZE',0])assert.throws(()=>build(state({attributes:[attribute({direction})]})),/minimize or maximize/);
});

test('source qualifiers need original object text and exact nonempty string values', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  for(const method of [undefined,null,'',' \t',0,{}])assert.throws(()=>build(state({attributes:[attribute({method})]})),/Attribute method/);
  for(const conditions of [undefined,null,{},[],0,'',' ','null','[]','"a"','{bad','\ufeff{}',
    '{"temperature":20}','{"temperature":null}','{"temperature":true}','{"temperature":[]}',
    '{"temperature":{}}','{"":"20"}','{" ":"20"}','{"temperature":""}','{"temperature":" \t"}']) {
    assert.throws(()=>build(state({attributes:[attribute({conditions})]})));
  }
  assert.deepEqual(JSON.parse(build(state({attributes:[attribute({conditions:'{}'})]}))).constraints[0].attribute.conditions,{});
});

test('duplicate property rows cannot silently conflict in units, protocols, bounds or objective direction', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  for(const duplicate of [attribute(),attribute({unit:'g/m2'}),attribute({method:'other'}),
    attribute({conditions:'{"humidity":"65"}'}),attribute({maximum:0.4}),attribute({direction:'maximize'})]) {
    assert.throws(()=>build(state({attributes:[attribute(),duplicate]})),/one row per property key/);
  }
  const exact=JSON.parse(build(state({attributes:[attribute(),attribute({key:'areal_density '})]})));
  assert.deepEqual(exact.constraints.map(row=>row.attribute.key),['areal_density','areal_density '],
    'Exact keys are not trimmed into another source property');
});

test('the complete request enforces independent native limits for 64 constraints and 16 objectives', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const rows=(count,kind)=>Array.from({length:count},(_,i)=>({key:kind+'-'+i,unit:'1',
    ...(kind==='bound'?{minimum:0,maximum:1}:{direction:'maximize'})}));
  const constraints=rows(64,'bound'),objectives=rows(16,'objective');
  const result=JSON.parse(build(state({attributes:[...constraints,...objectives]})));
  assert.equal(result.constraints.length,64);assert.equal(result.objectives.length,16);
  assert.throws(()=>build(state({attributes:rows(65,'bound')})),/at most 64 constraints/);
  assert.throws(()=>build(state({attributes:rows(17,'objective')})),/at most 16 objectives/);
  const both=constraints.map((row,i)=>({...row,...(i<16?{direction:'minimize'}:{})}));
  assert.equal(JSON.parse(build(state({attributes:both}))).objectives.length,16);
  assert.throws(()=>build(state({attributes:[...constraints,...objectives,{key:'extra',unit:'1',minimum:0}]})),/80 attribute rows/);
});

test('query and repeated exact qualifiers share the complete UTF-8 request allowance including framing', async()=>{
  const {buildMaterialScreenRequest:build}=await modulePromise;
  const limit=16*1024*1024,overhead=Buffer.byteLength(build(state())),remaining=limit-overhead;
  const query='é'.repeat(Math.floor(remaining/2))+'x'.repeat(remaining%2);
  assert.equal(Buffer.byteLength(build(state({query}))),limit);
  assert.throws(()=>build(state({query:query+'x'})),/16 MiB/);
  const conditions=JSON.stringify({specimen:'é'.repeat(4*1024*1024)});
  assert.ok(Buffer.byteLength(conditions)<limit);
  assert.throws(()=>build(state({attributes:[attribute({conditions})]})),/16 MiB/,
    'The same original qualifiers in a constraint and objective both consume output space');
});

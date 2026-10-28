// Focused checks of the actual photo page/worker with explicit DOM/WASM doubles.
// Numerical/image fidelity is covered by portable Rust tests and check_photo_web.cjs.
const test=require('node:test'), assert=require('node:assert/strict');
const fs=require('node:fs'), path=require('node:path'), vm=require('node:vm');
const root=path.resolve(__dirname,'..'), read=n=>fs.readFileSync(path.join(root,'web',n),'utf8');

function cameraReport(overrides={}) {
  return {schema:'ftx-camera-calibration/1',status:'ACCEPTED',solution:{intrinsics:{focal_px:[900,900]}},
    input:{lens_id:'declared-camera',image_model:'pinhole_zero_skew',views:[]},flags:[],advisories:[],
    checks:[{frame:{stream:'capture',timestamp_us:12},partition:'held_out',failure:null,
      corners:[{id:'1',partition:'fit',error_px:0.2},{id:'2',partition:'held_out',error_px:0.4}]}],...overrides};
}
async function photoWorker({failConstruction=false,failMeasure=false,failImage=false,calibrationReport=cameraReport()}={}) {
  const sent=[],calls=[],instances=[];
  class WebPhoto {
    constructor(bytes,paper) { calls.push({kind:'plain',bytes:[...bytes],paper}); instances.push(this); }
    static from_reference(bytes,sheet,calibration,capture) {
      calls.push({kind:'charuco',bytes:[...bytes],sheet,calibration,capture});
      if(failConstruction) throw Error('retained calibration refused');
      const photo=Object.create(WebPhoto.prototype); instances.push(photo); return photo;
    }
    report() { return '{"ok":true,"reference":"charuco","intrinsic_uncertainty":{"retained":true}}'; }
    preview() { return new Uint8Array([2,3]); }
    measure(points,options) { if(failMeasure) throw Error('measurement refused'); return JSON.stringify({points,options,records:[]}); }
    free() { assert.equal(this.freed,undefined); this.freed=true; }
  }
  class WebCameraCalibration {
    constructor(input,sheet,noise){this.input=JSON.parse(input);calls.push({kind:'calibration',input,sheet,noise});instances.push(this);}
    image_ids(){return JSON.stringify(this.input.views.map(v=>v.image));}
    add_image(id,bytes){calls.push({kind:'image',id,bytes:[...bytes]});if(failImage)throw Error('source acquisition refused');return JSON.stringify({image:id,retained_corners:16});}
    finish(){calls.push({kind:'finish'});return JSON.stringify(calibrationReport);}
    free(){assert.equal(this.freed,undefined);this.freed=true;}
  }
  function camera_calibration(input,noise){calls.push({kind:'coordinates',input,noise});return JSON.stringify(calibrationReport);}
  const context=vm.createContext({Uint8Array,JSON,Error,URL,self:{postMessage:m=>sent.push(m)}});
  const imports=new vm.SyntheticModule(['default','WebPhoto','WebCameraCalibration','camera_calibration'],function(){
    this.setExport('default',async()=>{}); this.setExport('WebPhoto',WebPhoto);
    this.setExport('WebCameraCalibration',WebCameraCalibration);this.setExport('camera_calibration',camera_calibration);
  },{context});
  await imports.link(()=>{}); await imports.evaluate();
  const module=new vm.SourceTextModule(read('photo-worker.js'),{
    context,initializeImportMeta:m=>{m.url='https://unit.invalid/photo-worker.js';},
    importModuleDynamically:()=>imports,
  });
  await module.link(()=>{}); await module.evaluate();
  return {calls,instances,sent,send:async m=>{await context.self.onmessage({data:m});return sent.at(-1);}};
}

test('photo worker passes exact ChArUco sources and capture assertions to the portable authority',async()=>{
  const w=await photoWorker(), source={request:7,op:'analyze',bytes:new Uint8Array([1,4]).buffer,
    referenceMode:'charuco',sheet:' {"sheet":"original λ"}\n',calibration:' {"retained":"report"} ',capture:'{"rotation_cw_deg":90}'};
  const result=await w.send(source);
  assert.equal(result.type,'result'); assert.equal(result.request,7); assert.equal(w.calls.length,1);
  assert.deepEqual(w.calls[0],{kind:'charuco',bytes:[1,4],sheet:source.sheet,calibration:source.calibration,capture:source.capture});
  assert.equal(result.scale.intrinsic_uncertainty.retained,true); assert.deepEqual([...result.preview],[2,3]);
  assert.equal(w.instances[0].freed,true);
});

test('photo worker never falls back after a calibrated refusal and frees failed measurements',async()=>{
  const failed=await photoWorker({failConstruction:true});
  assert.equal((await failed.send({op:'analyze',bytes:[1],referenceMode:'charuco',sheet:'{}',calibration:'bad',capture:'{}'})).type,'error');
  assert.deepEqual(failed.calls.map(c=>c.kind),['charuco']); assert.equal(failed.instances.length,0);
  const w=await photoWorker({failMeasure:true});
  assert.equal((await w.send({op:'measure',bytes:[1],referenceMode:'plain',paper:'letter',points:'{}',options:'{}'})).type,'error');
  assert.equal(w.instances[0].freed,true); assert.equal(w.calls[0].kind,'plain');
  assert.equal((await w.send({op:'analyze',bytes:[1],referenceMode:'unknown'})).type,'error');
  assert.equal(w.calls.length,1);
});

class Element {
  constructor(){this._value='';this.files=[];this.children=[];this.handlers={};this.hidden=false;this.disabled=false;this.textContent='';}
  get value(){return this._value;}
  set value(v){this._value=this.type==='textarea'?v.replace(/\r\n?/g,'\n'):v;if(this.type==='file' && v==='')this.files=[];}
  addEventListener(event,fn){this.handlers[event]=fn;}
  append(...items){this.children.push(...items);}
  replaceChildren(...items){this.children=items;}
  getContext(){return {drawImage(){},beginPath(){},moveTo(){},lineTo(){},closePath(){},stroke(){},arc(){},fill(){}};}
}
const file=(text,name='source.json')=>{const bytes=new TextEncoder().encode(text);return {name,size:bytes.length,arrayBuffer:async()=>bytes.buffer};};
async function photoPage({decode}={}) {
  const e=Object.fromEntries([...read('photo.html').matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
  for(const id of ['photo','sheet','calibration','cameraInput','cameraSheet','cameraImages','cameraDirectory','cameraNoise'])e[id].type='file';
  for(const id of ['pointFile','optionsFile','draftFile'])if(e[id])e[id].type='file';e.points.type='textarea';
  for(const [id,value] of Object.entries({referenceMode:'plain',paper:'auto',points:'{}',distance:'2500',depth:'0',click:'3',level:'1.5',bootstrap:'200',name:'across_back',cameraMode:'images'}))e[id].value=value;
  const workers=[],urls=new Map(),closed=[]; let serial=0;
  class Worker {
    constructor(){this.messages=[];workers.push(this);}
    postMessage(m){assert.equal(this.dead,undefined);this.messages.push(m);}
    terminate(){this.dead=true;}
    async emit(m){await this.onmessage({data:m});}
  }
  class LocalURL extends URL {}
  LocalURL.createObjectURL=blob=>{const u=`blob:${++serial}`;urls.set(u,blob);return u;};
  LocalURL.revokeObjectURL=u=>{assert.equal(urls.delete(u),true);};
  const bitmap=()=>({width:1600,height:1200,close(){closed.push(this);}});
  const context=vm.createContext({Worker,URL:LocalURL,Blob,TextDecoder,JSON,Number,Set,Uint8Array,
    createImageBitmap:decode || (async()=>bitmap()),
    document:{getElementById:id=>{assert.ok(e[id],`missing photo control ${id}`);return e[id];},createElement:()=>new Element()},
    window:{addEventListener(){}},
  });
  const module=new vm.SourceTextModule(read('photo-page.js'),{context,initializeImportMeta:m=>{m.url='https://unit.invalid/photo-page.js';}});
  await module.link(()=>{throw Error('unexpected photo page import');});await module.evaluate();
  const current=()=>workers.at(-1);
  function select(calibrated=true){
    e.photo.files=[file('image')];e.referenceMode.value='charuco';e.sheet.files=[file(' {"sheet":"λ"}\n')];
    e.calibration.files=calibrated?[file(' {"calibration":"retained"}\n')]:[];
    e.lensId.value='photo-lens';e.rotation.value='0';e.independence.value='Separate acquisition';
    e.referenceMode.handlers.change();
  }
  function scaleResult(request){const scale={ok:true,reference:'charuco',paper:'letter',mm_per_px:0.5,inliers:12,corners_detected:12,inlier_ids:[3],
    intrinsic_uncertainty:{retained:true},lens_input:{recomputed:true},warnings:[],image_input:{charuco_detection:{reference_input:{corners:[{id:3,point_px:[40,50]}]}}}};
    return {type:'result',request,scale,scaleText:JSON.stringify(scale),preview:new Uint8Array([1,2])};}
  async function analyze(){await e.analyze.onclick();const w=current();await w.emit(scaleResult(w.messages.at(-1).request));return w;}
  async function measure(){e.points.value='{"across_back":[[30,40],[80,40]]}';await e.measure.onclick();const w=current();await w.emit({type:'result',request:w.messages.at(-1).request,report:'{"records":[{"name":"across_back","value_mm":25,"sigma_mm":2}]}'});return w;}
  function selectCamera(mode='images'){
    e.cameraMode.value=mode;e.cameraInput.files=[file(mode==='images'?' {"views":[{"image":"first.png"},{"image":"second.png"}]}\n':' {"schema":"ftx-camera-calibration-input/1","views":[]}\n')];
    e.cameraSheet.files=[file(' {"sheet":"original λ"}\n')];e.cameraNoise.files=[file(' {"noise":"explicit source"}\n')];
    e.cameraImages.files=[file('first pixels','first.png'),file('second pixels','second.png')];e.cameraMode.handlers.change();
  }
  async function calibrate(report=cameraReport()){await e.cameraRun.onclick();const w=current();await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(report)});return w;}
  return {e,workers,urls,closed,current,select,analyze,measure,scaleResult,bitmap,selectCamera,calibrate};
}

test('photo controls retain exact local documents and explicitly asserted capture settings',async()=>{
  const p=await photoPage();p.select();await p.analyze();
  const m=p.current().messages[0];
  assert.equal(m.referenceMode,'charuco');assert.equal(m.sheet,' {"sheet":"λ"}\n');assert.equal(m.calibration,' {"calibration":"retained"}\n');
  assert.deepEqual(JSON.parse(m.capture),{capture_lens_id:'photo-lens',rotation_cw_deg:0,independent_calibration_evidence:'Separate acquisition'});
  assert.equal(p.e.image.width,1600);assert.equal(p.e.image.height,1200);assert.equal(p.e.measure.disabled,false);
  assert.match(p.e.summary.textContent,/Verified conditional lens uncertainty included/);
  for(const [name,expected] of [['photo-sheet.json',m.sheet],['photo-calibration.json',m.calibration],['photo-capture.json',m.capture]]){
    const link=p.e.sourceDownloads.children.find(a=>a.download===name);assert.ok(link);assert.equal(await p.urls.get(link.href).text(),expected);
  }
  await p.measure();assert.equal(p.e.result.hidden,false);
  p.e.clearCalibration.onclick();assert.equal(p.e.calibration.files.length,0);assert.equal(p.e.preview.hidden,true);assert.equal(p.e.measure.disabled,true);assert.equal(p.urls.size,0);
  await p.analyze();assert.equal(p.current().messages[0].calibration,'');assert.equal(p.current().messages[0].capture,'{}');
});

test('photo pair removal keeps other annotations and cancels stale measurement replies',async()=>{
  for(const completed of [false,true]){
    const p=await photoPage();p.select();await p.analyze();
    assert.ok(p.e.removePair,'Remove only the named photo pair');
    const other={points:[[90,110],[110,140]],view:'front',depth_offset_mm:0};
    p.e.points.value=JSON.stringify({across_back:[[30,40],[80,40]],arm_length:other});p.e.points.handlers.input();
    const sourceLinks=p.e.sourceDownloads.children.map(a=>a.href);
    await p.e.measure.onclick();const old=p.current(),request=old.messages[0].request;
    if(completed){
      await old.emit({type:'result',request,report:'{"records":[{"name":"across_back","value_mm":25,"sigma_mm":2}]}'});
      assert.equal(p.e.result.hidden,false);assert.equal(p.e.downloads.children.length,3);
    }
    assert.equal(p.e.removePair.disabled,false,'Saved pairs can be edited during or after a measurement');
    p.e.removePair.onclick();
    assert.equal(old.dead,true);assert.deepEqual(JSON.parse(p.e.points.value),{arm_length:other});
    assert.equal(p.e.result.hidden,true);assert.equal(p.e.downloads.children.length,0);
    assert.match(p.e.status.textContent,/removed/);
    assert.equal(p.e.preview.hidden,false);assert.equal(p.e.measure.disabled,false);
    assert.deepEqual(p.e.sourceDownloads.children.map(a=>a.href),sourceLinks);
    assert.equal(p.urls.size,sourceLinks.length);for(const url of sourceLinks)assert.equal(p.urls.has(url),true);
    await old.emit({type:'result',request,report:'{"records":[]}'});
    assert.equal(p.e.result.hidden,true);assert.equal(p.e.downloads.children.length,0);
    await p.e.measure.onclick();assert.deepEqual(JSON.parse(p.current().messages[0].points),{arm_length:other});
    for(const name of ['missing','  ']){
      p.e.name.value=name;p.e.name.handlers.input();
      const unchanged=p.e.points.value;assert.equal(p.e.removePair.disabled,true);
      p.e.removePair.onclick();assert.equal(p.e.points.value,unchanged);
    }
    for(const text of ['{','[]','null']){
      p.e.points.value=text;p.e.points.handlers.input();
      assert.equal(p.e.removePair.disabled,true);p.e.removePair.onclick();assert.equal(p.e.points.value,text);
    }
  }
});

test('pending photo click cancellation preserves exact saved pairs and permits re-addition',async()=>{
  const p=await photoPage();p.select();await p.analyze();
  assert.ok(p.e.cancelPoint,'Cancel only the first click');
  const saved=' {"across_back":[[30,40],[80,40]],"arm_length":{"points":[[5,6],[9,10]],"view":"front"}}\n';
  p.e.points.value=saved;p.e.points.handlers.input();
  p.e.image.getBoundingClientRect=()=>({left:0,top:0,width:800,height:600});
  p.e.image.onclick({clientX:50.25,clientY:60.25});
  assert.equal(p.e.measure.disabled,true);assert.equal(p.e.cancelPoint.disabled,false);
  p.e.cancelPoint.onclick();assert.equal(p.e.points.value,saved);
  assert.match(p.e.status.textContent,/cancelled/);
  assert.equal(p.e.cancelPoint.disabled,true);assert.equal(p.e.measure.disabled,false);
  p.e.image.onclick({clientX:30.25,clientY:40.25});assert.equal(p.e.points.value,saved);
  p.e.image.onclick({clientX:70.25,clientY:80.25});
  const pairs=JSON.parse(p.e.points.value);
  assert.deepEqual(pairs.across_back,[[60,80],[140,160]]);assert.deepEqual(pairs.arm_length,JSON.parse(saved).arm_length);
  p.e.image.onclick({clientX:40.25,clientY:50.25});p.e.removePair.onclick();
  assert.equal(p.e.cancelPoint.disabled,true);assert.deepEqual(JSON.parse(p.e.points.value),{arm_length:pairs.arm_length});
  p.e.image.onclick({clientX:60.25,clientY:70.25});assert.equal(p.e.measure.disabled,true);
  assert.equal(JSON.parse(p.e.points.value).across_back,undefined,'Removal also discards the pending replacement');
  p.e.cancelPoint.onclick();p.e.clearPoints.onclick();
  assert.equal(p.e.points.value,'{}');assert.equal(p.e.removePair.disabled,true);assert.equal(p.e.cancelPoint.disabled,true);
  assert.match(p.e.clickStatus.textContent,/cleared/);
  assert.match(p.e.status.textContent,/cleared/);
  p.e.image.onclick({clientX:60.25,clientY:70.25});p.e.name.value='shoulder_length';p.e.name.handlers.input();
  assert.equal(p.e.clickStatus.textContent,'');assert.equal(p.e.cancelPoint.disabled,true);
});

test('photo annotation drafts save work before measurement and restore exact bound sources',async()=>{
  const p=await photoPage();assert.ok(p.e.saveDraft,'Save unfinished annotation work');
  assert.equal(p.e.saveDraft.disabled,true);p.select();await p.analyze();
  const points=' {\r\n"across_back":[[30,40],[80,40]]\r\n}\r\n',opts=' {"seed":18446744073709551615,"bootstrap":19}\r\n';
  p.e.pointFile.files=[file(points)];await p.e.pointFile.handlers.change();
  p.e.optionsFile.files=[file(opts)];await p.e.optionsFile.handlers.change();
  p.e.saveDraft.onclick();
  const link=p.e.draftDownloads.children.find(a=>a.download==='photo-annotations.json');assert.ok(link);
  const text=await p.urls.get(link.href).text(),saved=JSON.parse(text);
  assert.deepEqual(saved,{schema:'ftx-photo-annotations/1',scale:p.e.scale.textContent,points,options:opts});
  assert.equal(p.e.downloads.children.length,0);assert.equal(p.e.result.hidden,true);
  p.e.clearPoints.onclick();assert.equal(p.e.draftDownloads.children.length,0);
  p.e.useFormOptions.onclick();
  p.e.draftFile.files=[file(text)];await p.e.draftFile.handlers.change();
  assert.equal(p.e.points.value,points.replace(/\r\n/g,'\n'));assert.equal(p.e.optionsSource.textContent,opts);
  assert.equal(p.e.draftFile.files.length,0);assert.equal(p.e.click.disabled,true);
  await p.e.measure.onclick();const w=p.current();assert.equal(w.messages[0].points,points);assert.equal(w.messages[0].options,opts);
  await w.emit({type:'error',request:w.messages[0].request,message:'bootstrap refused'});
  assert.equal(p.e.result.hidden,true);assert.equal(p.e.saveDraft.disabled,false);
  p.e.saveDraft.onclick();assert.ok(p.e.draftDownloads.children.find(a=>a.download==='photo-annotations.json'),'Refused work remains saveable');
  p.e.image.getBoundingClientRect=()=>({left:0,top:0,width:800,height:600});
  p.e.image.onclick({clientX:30.25,clientY:40.25});assert.equal(p.e.saveDraft.disabled,true);
  assert.equal(p.e.draftDownloads.children.length,0);p.e.cancelPoint.onclick();assert.equal(p.e.saveDraft.disabled,false);
});

test('photo annotation draft loading refuses wrong sources atomically and ignores superseded reads',async()=>{
  const p=await photoPage();assert.ok(p.e.draftFile,'Restore source-bound annotation work');p.select();await p.analyze();
  const points=' {"arm_length":[[3,4],[8,4]]}\n',opts=' {"seed":7}\n';
  p.e.pointFile.files=[file(points)];await p.e.pointFile.handlers.change();
  p.e.optionsFile.files=[file(opts)];await p.e.optionsFile.handlers.change();
  const draft={schema:'ftx-photo-annotations/1',scale:p.e.scale.textContent,points:'{"across_back":[[1,2],[3,4]]}',options:'{"seed":9}'};
  const load=async f=>{p.e.draftFile.files=[f];await p.e.draftFile.handlers.change();};
  for(const bad of [null,[],{}, {...draft,scale:draft.scale+' '},{...draft,schema:'fabric-measurements/1'},
    {...draft,extra:true},{...draft,points:'{"bad":null}'},{...draft,options:'{"records":[]}'},
    {...draft,options:7},{...draft,points:' '.repeat(256*1024)+'{}'},{...draft,options:' '.repeat(256*1024)+'{}'}]){
    await load(file(JSON.stringify(bad)));assert.ok(p.e.error.textContent);
    assert.equal(p.e.points.value,points);assert.equal(p.e.optionsSource.textContent,opts);
  }
  for(const bytes of [new Uint8Array([255]),new TextEncoder().encode('\ufeff{}'),new TextEncoder().encode('{')]){
    await load({size:bytes.length,arrayBuffer:async()=>bytes.buffer});assert.ok(p.e.error.textContent);
    assert.equal(p.e.points.value,points);assert.equal(p.e.optionsSource.textContent,opts);
  }
  let reads=0;await load({size:32*1024*1024+1,arrayBuffer:async()=>{reads++;return new ArrayBuffer(0);}});assert.equal(reads,0);
  await load({size:1,arrayBuffer:async()=>{throw Error('active draft read failure');}});
  assert.match(p.e.error.textContent,/active draft read failure/);assert.equal(p.e.optionsSource.textContent,opts);assert.equal(p.e.points.value,points);
  for(const reject of [false,true])for(const change of ['source','points','options','cancel','replacement']){
    const q=await photoPage();q.select();await q.analyze();let finish;
    const text=JSON.stringify({...draft,scale:q.e.scale.textContent}),read=new Promise((resolve,fail)=>{finish=()=>reject?fail(Error('late draft failure')):resolve(new TextEncoder().encode(text).buffer);});
    q.e.draftFile.files=[{size:text.length,arrayBuffer:()=>read}];const old=q.e.draftFile.handlers.change();
    if(change==='source'){q.e.photo.handlers.change();await q.analyze();}
    if(change==='points'){q.e.points.value=points;q.e.points.handlers.input();}
    if(change==='options'){q.e.optionsFile.files=[file(opts)];await q.e.optionsFile.handlers.change();}
    if(change==='cancel')q.e.cancel.onclick();
    if(change==='replacement'){q.e.draftFile.files=[file(text)];await q.e.draftFile.handlers.change();}
    await q.e.measure.onclick();const worker=q.current(),before=[q.e.points.value,q.e.optionsSource.textContent,q.e.status.textContent];
    finish();await old;assert.deepEqual([q.e.points.value,q.e.optionsSource.textContent,q.e.status.textContent],before,change);
    assert.equal(worker.dead,undefined,change);
  }
});

test('saved photo settings replay exact source and explicitly replace inactive form values',async()=>{
  const p=await photoPage();assert.ok(p.e.optionsFile,'Replay saved measurement settings');
  p.select();await p.analyze();
  const raw=' {\r\n"camera_distance_mm":3100,"click_px":2,"bootstrap":80,"seed":18446744073709551615\r\n}\r\n';
  p.e.optionsFile.files=[file(raw)];await p.e.optionsFile.handlers.change();
  assert.equal(p.e.optionsSource.textContent,raw);assert.equal(p.e.optionsFile.files.length,0);
  for(const id of ['distance','depth','click','level','bootstrap'])assert.equal(p.e[id].disabled,true,id);
  assert.equal(p.e.useFormOptions.disabled,false);assert.match(p.e.optionsSummary.textContent,/imported/i);
  p.e.points.value='{"across_back":[[30,40],[80,40]]}';await p.e.measure.onclick();
  const w=p.current();assert.equal(w.messages[0].options,raw);
  await w.emit({type:'result',request:w.messages[0].request,report:'{"records":[]}'});
  const link=p.e.downloads.children.find(a=>a.download==='photo-options.json');
  assert.equal(await p.urls.get(link.href).text(),raw);
  p.e.name.handlers.input();p.e.cancel.onclick();p.e.photo.handlers.change();await p.analyze();
  await p.measure();assert.equal(p.current().messages[0].options,raw,'Accepted settings remain explicit across source changes');
  p.e.useFormOptions.onclick();assert.equal(p.e.optionsSource.textContent,'');
  for(const id of ['distance','depth','click','level','bootstrap'])assert.equal(p.e[id].disabled,false,id);
  assert.equal(p.e.downloads.children.length,0);assert.equal(p.e.result.hidden,true);
  await p.measure();assert.deepEqual(JSON.parse(p.current().messages[0].options),{depth_offset_mm:0,camera_distance_mm:2500,click_px:3,sheet_level_sigma_deg:1.5,bootstrap:200,seed:0});
});

test('saved photo settings refuse wrong files atomically and discard stale reads',async()=>{
  const p=await photoPage();assert.ok(p.e.optionsFile,'Atomic saved-settings reader');p.select();await p.analyze();
  const raw=' {"bootstrap":100,"seed":17}\n';
  const load=async f=>{p.e.optionsFile.files=[f];await p.e.optionsFile.handlers.change();};
  await load(file(raw));
  for(const text of ['{','null','[]','\ufeff{}','{"records":[]}','{"across_back":[[1,2],[3,4]]}']){
    await load(file(text));assert.equal(p.e.optionsSource.textContent,raw);assert.ok(p.e.error.textContent);
  }
  let oversizedReads=0;
  await load({size:256*1024+1,arrayBuffer:async()=>{oversizedReads++;return new ArrayBuffer(0);}});
  assert.equal(oversizedReads,0);assert.equal(p.e.optionsSource.textContent,raw);
  await load({size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer});
  assert.ok(p.e.error.textContent);assert.equal(p.e.optionsSource.textContent,raw);
  for(const reject of [false,true])for(const change of ['source','points','form','cancel','replacement']){
    const q=await photoPage();q.select();await q.analyze();
    q.e.optionsFile.files=[file(raw)];await q.e.optionsFile.handlers.change();
    let finish;const read=new Promise((resolve,fail)=>{finish=()=>reject?fail(Error('late read failure')):resolve(new TextEncoder().encode('{"seed":99}').buffer);});
    q.e.optionsFile.files=[{size:11,arrayBuffer:()=>read}];const old=q.e.optionsFile.handlers.change();
    if(change==='source')q.e.photo.handlers.change();
    if(change==='points')q.e.points.handlers.input();
    if(change==='form')q.e.useFormOptions.onclick();
    if(change==='cancel')q.e.cancel.onclick();
    if(change==='replacement'){q.e.optionsFile.files=[file('{"seed":42}')];await q.e.optionsFile.handlers.change();}
    if(change==='source')await q.analyze();
    q.e.points.value='{"across_back":[[30,40],[80,40]]}';await q.e.measure.onclick();
    const worker=q.current(),before=q.e.optionsSource.textContent,message=q.e.status.textContent;
    finish();await old;assert.equal(q.e.optionsSource.textContent,before,change);
    assert.equal(q.e.status.textContent,message,change);assert.equal(worker.dead,undefined,change);
    await worker.emit({type:'result',request:worker.messages[0].request,report:'{"records":[]}'});
    assert.equal(q.e.result.hidden,false,change);
  }
});

test('saved photo points reload exact source while display and edits retain their own text',async()=>{
  const p=await photoPage();assert.ok(p.e.pointFile,'Load saved point annotations directly');
  assert.equal(p.e.pointFile.disabled,true);p.select();await p.analyze();
  const raw=' {\r\n"across_back":[[30,40],[80,40]],"arm_length":{"points":[[5,6],[9,10]],"view":"front"}\r\n}\r\n';
  const load=async()=>{p.e.pointFile.files=[file(raw,'saved-points.json')];await p.e.pointFile.handlers.change();};
  const sent=async()=>{await p.e.measure.onclick();return p.current().messages[0].points;};
  await load();assert.equal(p.e.points.value,raw.replace(/\r\n/g,'\n'));assert.equal(p.e.pointFile.files.length,0);
  p.e.image.getBoundingClientRect=()=>({left:0,top:0,width:800,height:600});
  p.e.image.onclick({clientX:50.25,clientY:60.25});p.e.cancelPoint.onclick();
  p.e.distance.handlers.input();assert.equal(await sent(),raw);
  const w=p.current();await w.emit({type:'result',request:w.messages[0].request,report:'{"records":[]}'});
  const download=p.e.downloads.children.find(a=>a.download==='photo-points.json');
  assert.equal(await p.urls.get(download.href).text(),raw);
  for(const edit of ['input','remove','canvas','clear']){
    await load();
    if(edit==='input')p.e.points.handlers.input();
    if(edit==='remove')p.e.removePair.onclick();
    if(edit==='canvas'){p.e.image.onclick({clientX:30.25,clientY:40.25});p.e.image.onclick({clientX:70.25,clientY:80.25});}
    if(edit==='clear')p.e.clearPoints.onclick();
    assert.equal(await sent(),p.e.points.value,edit);assert.notEqual(p.current().messages[0].points,raw,edit);
  }
});

test('saved photo point reads are bounded, atomic and cannot replace newer edits or sources',async()=>{
  const p=await photoPage();assert.ok(p.e.pointFile,'Bounded saved-point reader');p.select();await p.analyze();
  const saved=' {"across_back":[[30,40],[80,40]]}\n';p.e.points.value=saved;p.e.points.handlers.input();
  for(const text of ['{','[]','null','\ufeff{}','{"schema":"fabric-measurements/1","records":[]}',
    '{"bad":null}','{"bad":[[1,2]]}','{"bad":[[1,2],[3,"four"]]}']){
    p.e.pointFile.files=[file(text)];await p.e.pointFile.handlers.change();
    assert.equal(p.e.points.value,saved);assert.ok(p.e.error.textContent);assert.equal(p.e.cancel.disabled,true);
  }
  let oversizedReads=0;
  for(const invalid of [
    {size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer},
    {size:256*1024+1,arrayBuffer:async()=>{++oversizedReads;return new TextEncoder().encode(saved).buffer;}},
  ]){
    p.e.pointFile.files=[invalid];await p.e.pointFile.handlers.change();
    assert.equal(p.e.points.value,saved);assert.ok(p.e.error.textContent);
  }
  assert.equal(oversizedReads,0,'Oversized point JSON must be rejected before reading');
  for(const change of ['source','input','clear','cancel','remove','replace'])for(const fails of [false,true]){
    const q=await photoPage();q.select();await q.analyze();q.e.points.value=saved;q.e.points.handlers.input();
    let resolve,reject;q.e.pointFile.files=[{size:32,arrayBuffer:()=>new Promise((yes,no)=>{resolve=yes;reject=no;})}];
    const old=q.e.pointFile.handlers.change();assert.equal(q.e.cancel.disabled,false);let newest;
    if(change==='source')q.e.photo.handlers.change();
    if(change==='input'){q.e.points.value='{"arm_length":[[4,5],[6,7]]}';q.e.points.handlers.input();}
    if(change==='clear')q.e.clearPoints.onclick();
    if(change==='cancel')q.e.cancel.onclick();
    if(change==='remove')q.e.removePair.onclick();
    if(change==='replace'){
      q.e.pointFile.files=[file('{"arm_length":[[4,5],[6,7]]}')];await q.e.pointFile.handlers.change();
      await q.e.measure.onclick();newest=q.current();
    }
    const before=q.e.points.value,message=q.e.status.textContent;
    if(fails)reject(Error('Old source read failed'));else resolve(new TextEncoder().encode('{"old":[[1,2],[3,4]]}').buffer);
    await old;assert.equal(q.e.points.value,before,change);assert.equal(q.e.status.textContent,message,change);
    if(newest)assert.equal(newest.dead,undefined,'An old read cannot terminate a newer measurement');
  }
});

test('every reference or capture edit cancels pending measurement and withdraws old scale and downloads',async()=>{
  for(const id of ['referenceMode','sheet','calibration','lensId','rotation','independence']){
    const p=await photoPage();p.select();await p.analyze();await p.measure();
    await p.e.measure.onclick();const pending=p.current(),request=pending.messages[0].request;
    p.e[id].handlers[['lensId','independence'].includes(id)?'input':'change']();
    assert.equal(pending.dead,true,id);assert.equal(p.e.measure.disabled,true,id);assert.equal(p.e.preview.hidden,true,id);assert.equal(p.urls.size,0,id);
    await pending.emit({type:'result',request,report:'{"records":[]}'});
    assert.equal(p.e.result.hidden,true,id);assert.equal(p.e.downloads.children.length,0,id);
  }
});

test('superseded source reads and preview decodes cannot publish stale calibrated output',async()=>{
  const p=await photoPage();p.select();let release;
  p.e.calibration.files=[{size:10,arrayBuffer:()=>new Promise(resolve=>{release=resolve;})}];
  const run=p.e.analyze.onclick();p.e.sheet.handlers.change();release(new TextEncoder().encode('{}').buffer);await run;
  assert.equal(p.workers.length,0);assert.equal(p.urls.size,0);
  let decoded;const q=await photoPage({decode:()=>new Promise(resolve=>{decoded=resolve;})});q.select();await q.e.analyze.onclick();
  const pending=q.current().emit(q.scaleResult(q.current().messages[0].request));q.e.independence.handlers.input();
  const bitmap=q.bitmap();decoded(bitmap);await pending;
  assert.equal(q.closed[0],bitmap);assert.equal(q.e.preview.hidden,true);assert.equal(q.urls.size,0);
});

test('a rejected stale preview decode cannot cancel a newer worker or erase its published scale',async()=>{
  for(const publishNew of [false,true]){
    let rejectOld,decodes=0;
    const p=await photoPage({decode:()=>++decodes===1?new Promise((_,reject)=>{rejectOld=reject;}):Promise.resolve(p.bitmap())});
    p.select();await p.e.analyze.onclick();const old=p.current();
    const pending=old.emit(p.scaleResult(old.messages[0].request));
    p.e.calibration.handlers.change();await p.e.analyze.onclick();const current=p.current();
    if(publishNew)await current.emit(p.scaleResult(current.messages[0].request));
    rejectOld(Error('old preview cannot decode'));await pending;
    assert.equal(p.e.error.textContent,'');
    if(publishNew){assert.equal(p.e.preview.hidden,false);assert.equal(p.e.measure.disabled,false);assert.ok(p.urls.size>0);}
    else {assert.equal(current.dead,undefined);assert.equal(p.e.cancel.disabled,false);}
  }
});

test('photo source budgets and malformed UTF-8 refuse before worker creation',async()=>{
  for(const [id,size] of [['sheet',256*1024+1],['calibration',16*1024*1024+1]]){
    const p=await photoPage();p.select();let read=false;p.e[id].files=[{size,arrayBuffer:async()=>{read=true;return new ArrayBuffer(0);}}];
    await p.e.analyze.onclick();assert.equal(read,false);assert.equal(p.workers.length,0);assert.match(p.e.error.textContent,/at most/);
  }
  const p=await photoPage();p.select();p.e.sheet.files=[{size:2,arrayBuffer:async()=>new Uint8Array([0xc3,0x28]).buffer}];
  await p.e.analyze.onclick();assert.equal(p.workers.length,0);assert.notEqual(p.e.error.textContent,'');
});

test('ordinary point edits and cancellation preserve current scale but remove previous measurements',async()=>{
  const p=await photoPage();p.select(false);await p.analyze();await p.measure();const sourceCount=p.e.sourceDownloads.children.length;
  p.e.points.handlers.input();assert.equal(p.e.preview.hidden,false);assert.equal(p.e.measure.disabled,false);assert.equal(p.e.downloads.children.length,0);
  assert.equal(p.urls.size,sourceCount);await p.e.measure.onclick();const w=p.current();p.e.cancel.onclick();
  assert.equal(w.dead,true);assert.equal(p.e.cancel.disabled,true);assert.equal(p.urls.size,sourceCount);
});

test('manual point JSON edits abandon the previous unfinished canvas pair',async()=>{
  const p=await photoPage();p.select(false);await p.analyze();
  p.e.image.getBoundingClientRect=()=>({left:0,top:0,width:800,height:600});
  const click=(x,y)=>p.e.image.onclick({clientX:(x+.5)/2,clientY:(y+.5)/2});
  click(100,120);
  const typed='{"across_back":[[30,40],[80,40]]}';
  p.e.points.value=typed;p.e.points.handlers.input();
  click(200,240);
  assert.equal(p.e.points.value,typed,'the next click starts a new pair, not a pair with the old first click');
  click(500,540);
  assert.deepEqual(JSON.parse(p.e.points.value).across_back,[[200,240],[500,540]]);
});

test('an unfinished canvas pair withdraws results and cannot launch measurement',async()=>{
  const p=await photoPage();p.select(false);await p.analyze();await p.measure();
  p.e.image.getBoundingClientRect=()=>({left:0,top:0,width:800,height:600});
  const click=x=>p.e.image.onclick({clientX:(x+.5)/2,clientY:20.25});
  click(100);assert.equal(p.e.measure.disabled,true);assert.equal(p.e.downloads.children.length,0);
  const workers=p.workers.length;await p.e.measure.onclick();assert.equal(p.workers.length,workers);
  click(200);assert.equal(p.e.measure.disabled,false);
  assert.deepEqual(JSON.parse(p.e.points.value).across_back,[[100,40],[200,40]]);
  click(300);assert.equal(p.e.measure.disabled,true);
  p.e.clearPoints.onclick();assert.equal(p.e.measure.disabled,false);
  assert.equal(p.e.points.value,'{}');
});

test('camera worker acquires each exact declared image sequentially and retains the ordinary report',async()=>{
  const w=await photoWorker(),reads=[],input=' {"views":[{"image":"a.png"},{"image":"nested/b.jpg"}]}\n';
  let releaseFirst,notifyRead;const firstRead=new Promise(resolve=>{notifyRead=resolve;});
  const running=w.send({request:41,op:'calibrate-images',input,sheet:' sheet λ ',noise:' explicit noise ',images:[
    {id:'nested/b.jpg',file:{size:1,arrayBuffer:async()=>{reads.push('b');assert.ok(w.calls.some(c=>c.kind==='image'&&c.id==='a.png'));return new Uint8Array([2]).buffer;}}},
    {id:'a.png',file:{size:1,arrayBuffer:()=>{reads.push('a');return new Promise(resolve=>{releaseFirst=resolve;notifyRead();});}}},
  ]});
  await Promise.race([firstRead,running.then(()=>{throw Error('worker finished before acquiring the first image');})]);
  assert.deepEqual(reads,['a']);releaseFirst(new Uint8Array([1]).buffer);
  const result=await running;
  assert.deepEqual(reads,['a','b']);assert.equal(result.type,'result');assert.equal(result.request,41);
  assert.deepEqual(w.calls[0],{kind:'calibration',input,sheet:' sheet λ ',noise:' explicit noise '});
  assert.deepEqual(w.calls.filter(c=>c.kind==='image').map(c=>c.id),['a.png','nested/b.jpg']);
  assert.equal(w.sent.filter(m=>m.type==='calibration-view').length,2);assert.equal(w.instances[0].freed,true);
  assert.deepEqual(JSON.parse(result.report),cameraReport());
});

test('camera worker refuses incomplete, duplicate and oversized selections before reading, and frees failed acquisition',async()=>{
  const input='{"views":[{"image":"a.png"},{"image":"b.png"}]}';
  for(const ids of [['a.png'],['a.png','a.png'],['a.png','other.png']]){
    const w=await photoWorker();let reads=0;
    const result=await w.send({op:'calibrate-images',input,sheet:'{}',noise:'',images:ids.map(id=>({id,file:{size:1,arrayBuffer:async()=>{reads++;return new ArrayBuffer(1);}}}))});
    assert.equal(result.type,'error');assert.equal(reads,0);assert.equal(w.instances[0].freed,true);assert.ok(!w.calls.some(c=>c.kind==='finish'));
  }
  const w=await photoWorker({failImage:true});
  const result=await w.send({op:'calibrate-images',input,sheet:'{}',noise:'',images:['a.png','b.png'].map(id=>({id,file:file('pixels',id)}))});
  assert.equal(result.type,'error');assert.equal(w.instances[0].freed,true);assert.equal(w.calls.filter(c=>c.kind==='image').length,1);
  assert.ok(!w.calls.some(c=>c.kind==='finish'));
  const oversized=await photoWorker();let read=false;
  assert.equal((await oversized.send({op:'calibrate-images',input:'{"views":[{"image":"a.png"}]}',sheet:'{}',noise:'',
    images:[{id:'a.png',file:{size:64*1024*1024+1,arrayBuffer:async()=>{read=true;}}}]})).type,'error');
  assert.equal(read,false);assert.equal(oversized.instances[0].freed,true);
});

test('coordinate camera calibration preserves the exact noise request and rejected diagnostics',async()=>{
  const report=cameraReport({status:'UNOBSERVABLE',solution:null,flags:['insufficient independent tilt']}),w=await photoWorker({calibrationReport:report});
  const input=' {"coordinates":"original λ"}\n',noise=' {"sigma_px":0.2}\n';
  const result=await w.send({request:3,op:'calibrate-coordinates',input,noise});
  assert.deepEqual(w.calls,[{kind:'coordinates',input,noise}]);assert.deepEqual(JSON.parse(result.report),report);assert.equal(w.instances.length,0);
});

test('browser calibration keeps exact source and noise downloads and can immediately calibrate the current photo',async()=>{
  const p=await photoPage();p.select(false);p.selectCamera();
  let imageReads=0;for(const f of p.e.cameraImages.files)f.arrayBuffer=async()=>{imageReads++;throw Error('page must not decode images');};
  const camera=await p.calibrate(),m=camera.messages[0];
  assert.equal(m.op,'calibrate-images');assert.equal(imageReads,0);assert.equal(m.images[0].file,p.e.cameraImages.files[0]);
  assert.equal(p.e.cameraResult.hidden,false);assert.equal(p.e.cameraUse.disabled,false);assert.equal(p.e.cameraChecks.children.length,1);
  for(const [name,expected] of [['camera-image-manifest.json',m.input],['camera-sheet.json',m.sheet],['camera-corner-noise.json',m.noise]]){
    const a=p.e.cameraDownloads.children.find(a=>a.download===name);assert.ok(a);assert.equal(await p.urls.get(a.href).text(),expected);
  }
  const retained=p.e.cameraDownloads.children.find(a=>a.download==='camera-calibration.json'),exact=await p.urls.get(retained.href).text();
  p.e.cameraUse.onclick();assert.match(p.e.cameraActive.textContent,/declared-camera/);assert.equal(p.e.lensId.value,'photo-lens');
  await p.analyze();assert.equal(p.current().messages[0].calibration,exact);
  assert.deepEqual(JSON.parse(p.current().messages[0].capture),{capture_lens_id:'photo-lens',rotation_cw_deg:0,independent_calibration_evidence:'Separate acquisition'});
  const photoCalibration=p.e.sourceDownloads.children.find(a=>a.download==='photo-calibration.json');
  assert.equal(await p.urls.get(photoCalibration.href).text(),exact);
  p.e.cameraNoise.handlers.change();assert.equal(p.e.preview.hidden,true);assert.equal(p.e.measure.disabled,true);assert.equal(p.e.cameraActive.textContent,'');assert.equal(p.urls.size,0);
});

test('rejected calibration or unavailable requested uncertainty cannot activate a previous generated result',async()=>{
  for(const report of [cameraReport({status:'REJECTED',solution:null,flags:['held-out residual']}),
    cameraReport({conditional_uncertainty:{status:'UNAVAILABLE',estimate:null,flags:['noise model unresolved']}})]){
    const p=await photoPage();p.select(false);p.selectCamera('coordinates');await p.calibrate();p.e.cameraUse.onclick();await p.analyze();
    await p.calibrate(report);assert.equal(p.e.cameraResult.hidden,false);assert.equal(p.e.cameraUse.disabled,true);
    assert.equal(p.e.cameraActive.textContent,'');assert.equal(p.e.preview.hidden,true);p.e.cameraUse.onclick();
    await p.analyze();assert.equal(p.current().messages[0].calibration,'');assert.equal(p.current().messages[0].capture,'{}');
    const a=p.e.cameraDownloads.children.find(a=>a.download==='camera-calibration.json');assert.deepEqual(JSON.parse(await p.urls.get(a.href).text()),report);
  }
});

test('all calibration source edits and cancellation reject stale replies without changing unrelated photo evidence',async()=>{
  for(const id of ['cameraMode','cameraInput','cameraSheet','cameraImages','cameraDirectory','cameraNoise','cameraCancel']){
    const p=await photoPage();p.select();await p.analyze();const photoLinks=p.e.sourceDownloads.children.length;p.selectCamera();
    await p.e.cameraRun.onclick();const worker=p.current(),request=worker.messages[0].request;
    assert.equal(p.e.analyze.disabled,true);assert.equal(p.e.measure.disabled,true);
    if(id==='cameraCancel')p.e.cameraCancel.onclick();else p.e[id].handlers.change();
    assert.equal(worker.dead,true,id);assert.equal(p.e.preview.hidden,false,id);assert.equal(p.e.measure.disabled,false,id);
    await worker.emit({type:'result',request,report:JSON.stringify(cameraReport())});
    assert.equal(p.e.cameraResult.hidden,true,id);assert.equal(p.e.cameraDownloads.children.length,0,id);assert.equal(p.urls.size,photoLinks,id);
  }
});

test('calibration source read races, exact directory identities and input budgets are bounded',async()=>{
  const p=await photoPage();p.selectCamera();let release;
  p.e.cameraInput.files=[{size:1,arrayBuffer:()=>new Promise(resolve=>{release=resolve;})}];
  const pending=p.e.cameraRun.onclick();p.e.cameraNoise.handlers.change();release(new TextEncoder().encode('{}').buffer);await pending;
  assert.equal(p.workers.length,0);assert.equal(p.e.cameraCancel.disabled,true);assert.equal(p.urls.size,0);
  const q=await photoPage();q.selectCamera();q.e.cameraImages.files=[];
  q.e.cameraDirectory.files=[{...file('pixels','a.png'),webkitRelativePath:'capture/nested/a.png'},
    {...file('{}','views.json'),webkitRelativePath:'capture/views.json'}];
  await q.e.cameraRun.onclick();assert.equal(q.current().messages[0].images.length,1);assert.equal(q.current().messages[0].images[0].id,'nested/a.png');
  for(const [id,size] of [['cameraInput',16*1024*1024+1],['cameraNoise',16*1024*1024+1],['cameraSheet',256*1024+1]]){
    const r=await photoPage();r.selectCamera();let read=false;r.e[id].files=[{size,arrayBuffer:async()=>{read=true;}}];
    await r.e.cameraRun.onclick();assert.equal(read,false);assert.equal(r.workers.length,0);assert.match(r.e.cameraError.textContent,/at most/);
  }
  const r=await photoPage();r.selectCamera();r.e.cameraImages.files=Array.from({length:33},(_,i)=>file('pixels',`${i}.png`));
  await r.e.cameraRun.onclick();assert.equal(r.workers.length,0);assert.match(r.e.cameraError.textContent,/1–32/);
  const invalid=await photoPage();invalid.selectCamera();invalid.e.cameraNoise.files=[{size:2,arrayBuffer:async()=>new Uint8Array([0xc3,0x28]).buffer}];
  await invalid.e.cameraRun.onclick();assert.equal(invalid.workers.length,0);assert.notEqual(invalid.e.cameraError.textContent,'');
});

async function compiledPhotoBindings(){
  const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(root,'web/pkg');
  const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
  ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});return ftx;
}

test('compiled WASM identifies an independently projected pinhole camera and rejects unsupported noise or altered held-out evidence',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(),hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    const controls=Array.from({length:20},(_,i)=>({id:`p${String(i).padStart(2,'0')}`,sheet_xz_m:[i%5*0.12-0.24,Math.floor(i/5)*0.12-0.18]}));
    // Independent analytic rotations/projective camera; no fitted WASM output generates input.
    const views=[[0.35,0.1],[-0.45,0.3],[0.2,-0.55],[-0.3,-0.25]].map(([a,b],i)=>({
      frame:{stream:'synthetic-lens',timestamp_us:(i+1)*200000},partition:i===3?'held_out':'fit',
      corners:controls.map(({id,sheet_xz_m:[x,z]},k)=>{
        const xx=Math.cos(b)*x+(i-1.5)*0.05, yy=-Math.cos(a)*z+Math.sin(a)*Math.sin(b)*x+0.015*i;
        const zz=-Math.sin(a)*z-Math.cos(a)*Math.sin(b)*x+1.2+0.08*i;
        return {id,pixel_px:[900*xx/zz+641,1100*yy/zz+463],partition:k===6||k===13?'held_out':'fit'};
      }),
    }));
    const input={schema:'ftx-camera-calibration-input/1',image_model:'pinhole_zero_skew',lens_id:'synthetic-pinhole',
      pinhole_evidence:'Independent analytic zero-skew projection without distortion',reference_evidence:'Synthetic 120 mm planar grid',
      image_size_px:[1280,960],rotation_cw_deg:0,controls,views};
    const source=' '+JSON.stringify(input)+'\n',noise=' '+JSON.stringify({schema:'ftx-calibration-corner-noise/1',sigma_px:0.2,
      evidence:'Explicit synthetic noise assumption for software checks',independence_evidence:'Independent synthetic fitting coordinates and views'})+'\n';
    const report=JSON.parse(ftx.camera_calibration(source,''));
    assert.equal(report.status,'ACCEPTED');assert.equal(report.input_sha256,hash(source));assert.equal(report.checks.length,4);
    assert.equal(report.conditional_uncertainty,undefined);assert.equal(report.corner_noise_input,undefined);
    assert.throws(()=>ftx.camera_calibration(source,noise),/Brown|brown5|uncertainty/i);
    for(const [key,expected] of [['focal_px',[900,1100]],['principal_px',[641,463]]]) {
      report.solution.intrinsics[key].forEach((v,i)=>assert.ok(Math.abs(v-expected[i])<1e-7));
    }
    const altered=structuredClone(input);altered.views[3].corners[6].pixel_px[0]+=20;
    const rejected=JSON.parse(ftx.camera_calibration(JSON.stringify(altered),''));
    assert.equal(rejected.status,'REJECTED');assert.equal(rejected.solution,null);assert.ok(rejected.flags.length>0);
  });

test('compiled WASM camera acquisition uses original ChArUco pixels and refuses image reuse',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(),sheet=fs.readFileSync(path.join(root,'web/calibration/calib-sheet-letter.json'),'utf8');
    const bytes=fs.readFileSync(path.join(root,'web/calibration/calib-sheet-letter.png'));
    const manifest={schema:'ftx-camera-calibration-images/1',image_model:'pinhole_zero_skew',lens_id:'synthetic-board',
      pinhole_evidence:'Synthetic rendered board for acquisition refusal checks',reference_evidence:'Synthetic 22 mm board',
      image_size_px:[2550,3300],rotation_cw_deg:0,held_out_corner_ids:[0,6,28,34],
      views:['a.png','b.png','c.png','check.png'].map((image,i)=>({image,frame:{stream:'synthetic',timestamp_us:i*100000},partition:i===3?'held_out':'fit'}))};
    const builder=new ftx.WebCameraCalibration(JSON.stringify(manifest),sheet,'');
    try{
      assert.deepEqual(JSON.parse(builder.image_ids()),manifest.views.map(v=>v.image));
      assert.throws(()=>builder.finish(),/complete|missing|acquir|views/i);
      const receipt=JSON.parse(builder.add_image('a.png',bytes));
      assert.equal(receipt.corners_detected,35);assert.equal(receipt.held_out_corners,4);
      assert.throws(()=>builder.add_image('b.png',bytes),/duplicate|reuse/i);
      assert.throws(()=>builder.finish(),/complete|missing|acquir|views/i);
    }finally{builder.free();}
  });

test('compiled WASM replays the complete native raster acquisition and accepted camera report',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_CAMERA_WEB_FIXTURES},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_CAMERA_WEB_FIXTURES;
    const source=name=>fs.readFileSync(path.join(dir,name),'utf8');
    const manifest=source('manifest.json'),sheet=source('sheet.json'),expected=JSON.parse(source('report.json'));
    const builder=new ftx.WebCameraCalibration(manifest,sheet,'');
    try{
      for(const id of JSON.parse(builder.image_ids()))builder.add_image(id,fs.readFileSync(path.join(dir,id)));
      const actual=JSON.parse(builder.finish());assert.equal(actual.status,'ACCEPTED');assert.deepEqual(actual,expected);
      assert.equal(actual.image_acquisition.views.length,4);assert.equal(actual.input.views.filter(v=>v.partition==='held_out').length,1);
      assert.ok(actual.checks.every(view=>view.corners.some(c=>c.partition==='held_out')));
      assert.deepEqual(JSON.parse(builder.finish()),actual,'Repeated finish preserves the exact source observations');
    }finally{builder.free();}
  });

test('compiled WASM detects local ChArUco pixels and enforces calibrated capture assertions',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(), bytes=fs.readFileSync(path.join(root,'web/calibration/calib-sheet-letter.png'));
    const sheet=fs.readFileSync(path.join(root,'web/calibration/calib-sheet-letter.json'),'utf8');
    const lens=JSON.stringify({schema:'ftx-photo-lens/1',lens_id:'synthetic-identity',image_size_px:[2550,3300],rotation_cw_deg:0,
      calibration_evidence:'Synthetic software fixture; not a real camera',intrinsics:{focal_px:[3000,3000],principal_px:[1275,1650]}});
    const capture='{"capture_lens_id":"synthetic-identity","rotation_cw_deg":0}';
    const photo=ftx.WebPhoto.from_reference(bytes,sheet,lens,capture);
    try{
      const report=JSON.parse(photo.report());assert.equal(report.ok,true);assert.equal(report.inliers,35);
      assert.equal(report.coordinate_source,'detected_charuco');assert.equal(report.lens_input.recomputed,false);
      assert.equal(report.image_input.charuco_detection.implementation.backend,'rust_charuco');
      const pixel=(x,y)=>[134.5+x*2079/176,93.5+y*1559/132];
      const result=JSON.parse(photo.measure(JSON.stringify({across_back:[pixel(44,44),pixel(132,44)]}),'{}'));
      assert.ok(Math.abs(result.records[0].value_mm-88)<0.2);assert.equal(result.records[0].provenance,'PHOTO_ESTIMATED');
      assert.deepEqual(result.photo.lens_input,report.lens_input);assert.ok(photo.preview().length>0);
    }finally{photo.free();}
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,lens,'{}'),/capture_lens_id/);
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,lens,capture.replace('identity','other')),/lens identifier/);
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,'',capture),/require a calibration/);
  });

test('compiled WASM retains native Brown calibration covariance and complete photo measurement results',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_PHOTO_WEB_FIXTURES},async()=>{
    const ftx=await compiledPhotoBindings(), dir=process.env.FTX_PHOTO_WEB_FIXTURES;
    const source=name=>fs.readFileSync(path.join(dir,`calibrated-${name}.json`),'utf8');
    const bytes=fs.readFileSync(path.join(dir,'calibrated-photo.png')),sheet=source('sheet'),lens=source('lens'),capture=source('capture');
    const saved=JSON.parse(lens),coordinates=structuredClone(saved.input),inline=coordinates.distortion_fit.corner_noise;
    delete coordinates.distortion_fit.corner_noise;
    const input=' '+JSON.stringify(coordinates)+'\n',noise=' '+JSON.stringify({schema:'ftx-calibration-corner-noise/1',
      sigma_px:inline.sigma_px,evidence:inline.evidence,independence_evidence:'Independent synthetic coordinates and views from the native Brown fixture'})+'\n';
    const identified=JSON.parse(ftx.camera_calibration(input,noise)),hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    assert.equal(identified.status,'ACCEPTED');assert.equal(identified.input_sha256,hash(input));assert.equal(identified.corner_noise_input.sha256,hash(noise));
    assert.deepEqual(identified.solution,saved.solution);
    const uncertainty=identified.conditional_uncertainty,native=saved.uncertainty;
    assert.equal(uncertainty.status,'AVAILABLE');assert.deepEqual(uncertainty.flags,[]);assert.deepEqual(uncertainty.input,JSON.parse(noise));
    // Inline and separate noise retain different envelopes around the same marginalized estimate.
    assert.deepEqual(uncertainty.estimate,{intrinsics:native.intrinsics,covariance:native.covariance,
      sigma:native.standard_deviations,parameters:native.parameters,fitting_views:native.nuisance_parameters/6,
      fitting_scalar_observations:2*native.fit_corners});
    const photo=ftx.WebPhoto.from_reference(bytes,sheet,lens,capture);
    try{
      assert.deepEqual(JSON.parse(photo.report()),JSON.parse(source('scale')));
      const result=JSON.parse(photo.measure(source('points'),'{}'));
      assert.deepEqual(result,JSON.parse(source('measurements')));
      assert.ok(result.records[0].sigma_terms_mm.intrinsics>0);assert.equal(result.photo.lens_input.recomputed,true);
    }finally{photo.free();}
    const identifiedPhoto=ftx.WebPhoto.from_reference(bytes,sheet,JSON.stringify(identified),capture);
    try{
      const result=JSON.parse(identifiedPhoto.measure(source('points'),'{}'));
      assert.deepEqual(result.records,JSON.parse(source('measurements')).records);
      assert.equal(result.photo.lens_input.recomputed,true);
    }finally{identifiedPhoto.free();}
    const missing=JSON.parse(capture);delete missing.independent_calibration_evidence;
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,lens,JSON.stringify(missing)),/independent-calibration-evidence/);
    const reused=JSON.parse(lens);reused.image_acquisition={views:[{sha256:require('node:crypto').createHash('sha256').update(bytes).digest('hex')}]};
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,JSON.stringify(reused),capture),/reuses a calibration image/);
  });

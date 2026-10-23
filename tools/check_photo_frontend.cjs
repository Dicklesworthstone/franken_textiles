// Focused checks of the actual photo page/worker with explicit DOM/WASM doubles.
// Numerical/image fidelity is covered by portable Rust tests and check_photo_web.cjs.
const test=require('node:test'), assert=require('node:assert/strict');
const fs=require('node:fs'), path=require('node:path'), vm=require('node:vm');
const root=path.resolve(__dirname,'..'), read=n=>fs.readFileSync(path.join(root,'web',n),'utf8');

async function photoWorker({failConstruction=false,failMeasure=false}={}) {
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
  const context=vm.createContext({Uint8Array,JSON,Error,URL,self:{postMessage:m=>sent.push(m)}});
  const imports=new vm.SyntheticModule(['default','WebPhoto'],function(){
    this.setExport('default',async()=>{}); this.setExport('WebPhoto',WebPhoto);
  },{context});
  await imports.link(()=>{}); await imports.evaluate();
  const module=new vm.SourceTextModule(read('photo-worker.js'),{
    context,initializeImportMeta:m=>{m.url='https://unit.invalid/photo-worker.js';},
    importModuleDynamically:()=>imports,
  });
  await module.link(()=>{}); await module.evaluate();
  return {calls,instances,send:async m=>{await context.self.onmessage({data:m});return sent.at(-1);}};
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
  set value(v){this._value=v;if(this.type==='file' && v==='')this.files=[];}
  addEventListener(event,fn){this.handlers[event]=fn;}
  append(...items){this.children.push(...items);}
  replaceChildren(...items){this.children=items;}
  getContext(){return {drawImage(){},beginPath(){},moveTo(){},lineTo(){},closePath(){},stroke(){},arc(){},fill(){}};}
}
const file=text=>{const bytes=new TextEncoder().encode(text);return {size:bytes.length,arrayBuffer:async()=>bytes.buffer};};
async function photoPage({decode}={}) {
  const e=Object.fromEntries([...read('photo.html').matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
  for(const id of ['photo','sheet','calibration'])e[id].type='file';
  for(const [id,value] of Object.entries({referenceMode:'plain',paper:'auto',points:'{}',distance:'2500',depth:'0',click:'3',level:'1.5',bootstrap:'200',name:'across_back'}))e[id].value=value;
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
  return {e,workers,urls,closed,current,select,analyze,measure,scaleResult,bitmap};
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

async function compiledPhotoBindings(){
  const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(root,'web/pkg');
  const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
  ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});return ftx;
}

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
    const photo=ftx.WebPhoto.from_reference(bytes,sheet,lens,capture);
    try{
      assert.deepEqual(JSON.parse(photo.report()),JSON.parse(source('scale')));
      const result=JSON.parse(photo.measure(source('points'),'{}'));
      assert.deepEqual(result,JSON.parse(source('measurements')));
      assert.ok(result.records[0].sigma_terms_mm.intrinsics>0);assert.equal(result.photo.lens_input.recomputed,true);
    }finally{photo.free();}
    const missing=JSON.parse(capture);delete missing.independent_calibration_evidence;
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,lens,JSON.stringify(missing)),/independent-calibration-evidence/);
    const reused=JSON.parse(lens);reused.image_acquisition={views:[{sha256:require('node:crypto').createHash('sha256').update(bytes).digest('hex')}]};
    assert.throws(()=>ftx.WebPhoto.from_reference(bytes,sheet,JSON.stringify(reused),capture),/reuses a calibration image/);
  });

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
async function photoWorker({failConstruction=false,failMeasure=false,failImage=false,failSection=false,failTriangulation=false,failBundle=false,failStill=false,failBodyFit=false,failBodyReview=false,failBodyStart=false,failBodyImage=false,failBodyImagePreview=false,bodyImageClass=null,bodyStartReview=null,bodyStarts=null,bodyObservations=null,calibrationReport=cameraReport()}={}) {
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
  class WebBodyFitImage {
    constructor(image,review){calls.push({kind:'body-frame-image',image:[...image],review});if(failBodyImage)throw Error('original capture image SHA-256 mismatch');instances.push(this);}
    preview(){if(failBodyImagePreview)throw Error('image preview failed');return new Uint8Array([8,5,3]);}
    report(){return ' {"verified":"original image"}\r\n';}
    free(){assert.equal(this.freed,undefined);this.freed=true;}
  }
  function camera_calibration(input,noise){calls.push({kind:'coordinates',input,noise});return JSON.stringify(calibrationReport);}
  function photo_section(width,depth,options){calls.push({kind:'section',width,depth,options});if(failSection)throw Error('section evidence refused');return JSON.stringify(sectionReport());}
  function triangulate(input,noise){calls.push({kind:'triangulate',input,noise});if(failTriangulation)throw Error('capture evidence refused');return JSON.stringify(triangulationReport());}
  function bundle_adjust(input,iterations){calls.push({kind:'bundle-adjust',input,iterations});if(failBundle)throw Error('metric controls refused');return JSON.stringify(bundleReport());}
  function still_window(input,source){calls.push({kind:'still-window',input,source});if(failStill)throw Error('track evidence refused');return JSON.stringify(stillReport());}
  function fit_body(model,input,iterations,maxEvaluations){calls.push({kind:'fit-body',model:[...model],input,iterations,maxEvaluations});if(failBodyFit)throw Error('model SHA-256 mismatch');return JSON.stringify(bodyFitReport());}
  function body_fit_starts(model,input,edits){calls.push({kind:'body-starts',model:[...model],input,edits});return JSON.stringify(bodyStartsReport({input}));}
  function body_fit_observations(model,input,edits){calls.push({kind:'body-observations',model:[...model],input,edits});return JSON.stringify(bodyPointsReport({input}));}
  function review_body_fit_start(model,input,startIndex,frameIndex){calls.push({kind:'body-start-review',model:[...model],input,startIndex,frameIndex});if(failBodyStart)throw Error('no declared initialization at that index');return JSON.stringify(bodyStartFrameReport({start_index:Number(startIndex),frame_index:Number(frameIndex),start_count:4,frame_count:8}));}
  function review_body_fit_frame(model,report,frameIndex){calls.push({kind:'body-frame-review',model:[...model],report,frameIndex});if(failBodyReview)throw Error('full mesh keypoint disagrees with the accepted 3-pixel fit gate');return JSON.stringify(bodyFrameReport());}
  const context=vm.createContext({Uint8Array,ArrayBuffer,TextEncoder,JSON,Error,URL,self:{postMessage:m=>sent.push(m)}});
  const imports=new vm.SyntheticModule(['default','WebPhoto','WebCameraCalibration','WebBodyFitImage','camera_calibration','photo_section','triangulate','bundle_adjust','still_window','fit_body','body_fit_starts','body_fit_observations','review_body_fit_start','review_body_fit_frame'],function(){
    this.setExport('default',async()=>{}); this.setExport('WebPhoto',WebPhoto);
    this.setExport('WebCameraCalibration',WebCameraCalibration);this.setExport('camera_calibration',camera_calibration);
    this.setExport('WebBodyFitImage',bodyImageClass ?? WebBodyFitImage);
    this.setExport('photo_section',photo_section);
    this.setExport('triangulate',triangulate);
    this.setExport('bundle_adjust',bundle_adjust);
    this.setExport('still_window',still_window);
    this.setExport('fit_body',fit_body);
    this.setExport('body_fit_starts',bodyStarts ?? body_fit_starts);
    this.setExport('body_fit_observations',bodyObservations ?? body_fit_observations);
    this.setExport('review_body_fit_start',bodyStartReview ?? review_body_fit_start);
    this.setExport('review_body_fit_frame',review_body_fit_frame);
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
  getContext(){const element=this,mark=(kind,args)=>(element.drawing ||= []).push([kind,...args]);return {
    drawImage(...args){mark('image',args);},beginPath(){},moveTo(){},lineTo(){},closePath(){},stroke(){},arc(){},fill(){},
    fillRect(...args){mark('fill',args);},fillText(...args){(element.labels ||= []).push(args);mark('label',args);}};}
}
const file=(text,name='source.json')=>{const bytes=new TextEncoder().encode(text);return {name,size:bytes.length,arrayBuffer:async()=>bytes.buffer};};
async function photoPage({decode,materialsToken=null}={}) {
  const e=Object.fromEntries([...read('photo.html').matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
  for(const id of ['photo','sheet','calibration','cameraInput','cameraSheet','cameraImages','cameraDirectory','cameraNoise'])e[id].type='file';
  for(const id of ['pointFile','optionsFile','draftFile','sectionWidthFile','sectionDepthFile','triangulationInput','triangulationNoise','bodyFitModel','bodyFitInput','bodyFitSaved','bodyFitReviewImage','bodyPointsImage'])if(e[id])e[id].type='file';e.points.type='textarea';
  for(const [id,value] of Object.entries({referenceMode:'plain',paper:'auto',points:'{}',distance:'2500',depth:'0',click:'3',level:'1.5',bootstrap:'200',name:'across_back',cameraMode:'images',geometryOperation:'triangulate',bundleIterations:'50'}))e[id].value=value;
  for(const [id,value] of Object.entries({sectionLevel:'waist',sectionMode:'girth',sectionClothing:'0',sectionRatio:'1',sectionRatioSigma:'0',sectionTapeSigma:'10'}))e[id].value=value;
  e.bodyFitIterations.value='50';e.bodyFitEvaluations.value='100000';
  e.bodyFitStartIndex.value='1';e.bodyFitStartFrame.value='1';
  const workers=[],urls=new Map(),closed=[],windowEvents={},popups=[]; let serial=0;
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
  const openerMessages=[],opener={closed:false,postMessage:(data,origin)=>openerMessages.push({data,origin})};
  const context=vm.createContext({Worker,URL:LocalURL,Blob,TextDecoder,JSON,Number,Set,Uint8Array,
    createImageBitmap:decode || (async()=>bitmap()),
    document:{getElementById:id=>{assert.ok(e[id],`missing photo control ${id}`);return e[id];},createElement:()=>new Element()},
    crypto:{getRandomValues:a=>a.fill(7)},
    window:{opener,location:{href:'https://unit.invalid/photo.html'+(materialsToken?'?materialsReturn='+materialsToken:'')},addEventListener:(name,fn)=>{windowEvents[name]=fn;},open:url=>{
      const messages=[],target={closed:false,postMessage:(data,origin)=>messages.push({data,origin})};
      popups.push({url,target,messages});return target;
    }},
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
  return {e,workers,urls,closed,current,select,analyze,measure,scaleResult,bitmap,selectCamera,calibrate,windowEvents,popups,opener,openerMessages};
}

test('photo records handoff transfers only the current exact measurement snapshot once to its own tab',async()=>{
  const p=await photoPage();assert.ok(p.e.openWorkbench,'Local photo-record handoff control');
  assert.equal(p.e.openWorkbench.disabled,true);p.select();await p.analyze();await p.measure();
  assert.equal(p.e.openWorkbench.disabled,false);
  const text=p.e.report.textContent;p.e.openWorkbench.onclick();p.e.openWorkbench.onclick();
  assert.equal(p.popups.length,1,'Only one pending tab');
  const popup=p.popups[0],token=new URL(popup.url).searchParams.get('photoImport');assert.match(token,/^[0-9a-f]{32}$/);
  const data={type:'ftx-photo-records-ready',token};
  for(const [origin,source,m] of [['https://foreign.invalid',popup.target,data],['https://unit.invalid',{},data],['https://unit.invalid',popup.target,{...data,token:'wrong'}]]) {
    p.windowEvents.message({origin,source,data:m});assert.equal(popup.messages.length,0);
  }
  const ready=()=>p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data});
  ready();assert.equal(popup.messages.length,1);assert.equal(popup.messages[0].data.type,'ftx-photo-records');
  assert.equal(await popup.messages[0].data.records.text(),text);assert.equal(popup.messages[0].origin,'https://unit.invalid');
  ready();assert.equal(popup.messages.length,1);
  p.e.openWorkbench.onclick();const stale=p.popups[1];p.e.points.handlers.input();assert.equal(p.e.openWorkbench.disabled,true);
  p.windowEvents.message({origin:'https://unit.invalid',source:stale.target,data:{type:'ftx-photo-records-ready',token:new URL(stale.url).searchParams.get('photoImport')}});
  assert.equal(stale.messages[0].data.type,'ftx-photo-records-cancelled');
});

test('photo material return offers only a current exact snapshot to its initiating session',async()=>{
  const token='ab'.repeat(16),p=await photoPage({materialsToken:token});assert.equal(p.e.returnMaterials.hidden,false);assert.equal(p.e.returnMaterials.disabled,true);
  p.select();await p.analyze();await p.measure();const raw=p.e.report.textContent;p.e.returnMaterials.onclick();
  assert.equal(p.openerMessages[0].data.type,'ftx-photo-records-offer');p.e.returnMaterials.onclick();assert.equal(p.openerMessages.length,1);
  for(const [origin,source] of [['https://foreign.invalid',p.opener],['https://unit.invalid',{}]]) {
    p.windowEvents.message({origin,source,data:{type:'ftx-photo-records-ready',token}});assert.equal(p.openerMessages.length,1);
  }
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-photo-records-ready',token}});
  assert.equal(await p.openerMessages[1].data.records.text(),raw);
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-materials-photo-received',token}});
  assert.equal(p.e.returnMaterials.disabled,true);assert.match(p.e.status.textContent,/received by Material Workbench/);
  p.e.returnMaterials.onclick();assert.equal(p.openerMessages.length,2);
});
test('photo material return withdraws changed snapshots and closes the one-use return on cancellation or exit',async()=>{
  const token='ab'.repeat(16),p=await photoPage({materialsToken:token});p.select();await p.analyze();await p.measure();
  p.e.returnMaterials.onclick();p.e.points.handlers.input();
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-photo-records-ready',token}});
  assert.equal(p.openerMessages[1].data.type,'ftx-photo-records-cancelled');
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-materials-photo-cancelled',token}});
  assert.equal(p.e.returnMaterials.disabled,true);assert.match(p.e.status.textContent,/cancelled this return/);
  const exited=await photoPage({materialsToken:token});exited.windowEvents.pagehide();assert.equal(exited.openerMessages[0].data.type,'ftx-photo-records-cancelled');
  const closed=await photoPage({materialsToken:token});closed.select();await closed.analyze();await closed.measure();closed.opener.closed=true;
  closed.e.returnMaterials.onclick();assert.equal(closed.openerMessages.length,0);assert.match(closed.e.status.textContent,/closed/);
  assert.equal((await photoPage()).e.returnMaterials.hidden,true);
});
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

function sectionReport() {
  return {schema:'fabric-measurements/1',level:'waist',girth_mm:484.2,sigma_mm:14.6,
    sigma_terms_mm:{width:6.99,depth:7.87,k:10.05},clothing_mm_per_side:0,notes:'UNCERTAIN section-shape assumption',
    records:[{name:'waist',value_mm:484.2,sigma_mm:14.6,provenance:'PHOTO_ESTIMATED'}]};
}
function selectSection(p) {
  p.e.sectionWidthFile.files=[file(' {"front":"original λ","seed":18446744073709551615}\r\n','front.json')];
  p.e.sectionDepthFile.files=[file(' {"side":"original","value":-0.0}\r\n','side.json')];
  p.e.sectionWidthFile.handlers.change();p.e.sectionDepthFile.handlers.change();
  p.e.sectionConsent.checked=true;p.e.sectionConsent.handlers.change();
}
async function sectionResult(p,report=sectionReport()) {
  await p.e.sectionRun.onclick();const w=p.current();
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(report)});return w;
}

test('section worker passes exact photo sources to the shared authority without constructing a photo',async()=>{
  const source={request:11,op:'section',width:' {"front":18446744073709551615}\r\n',depth:' {"side":-0.0}\n',options:' {"level":"waist"}\n'};
  const w=await photoWorker(),result=await w.send(source);
  assert.deepEqual(w.calls,[{kind:'section',width:source.width,depth:source.depth,options:source.options}]);
  assert.equal(w.instances.length,0);assert.equal(result.request,11);assert.deepEqual(JSON.parse(result.report),sectionReport());
  const refused=await photoWorker({failSection:true});assert.equal((await refused.send(source)).type,'error');
  assert.equal(refused.instances.length,0);assert.deepEqual(refused.calls.map(c=>c.kind),['section']);
});

test('retained front and side photo snapshots survive later photo edits and require fresh source consent',async()=>{
  const p=await photoPage();p.select(false);await p.analyze();await p.measure();
  const exact=p.e.report.textContent;p.e.sectionKeepWidth.onclick();
  assert.match(p.e.sectionWidthSource.textContent,/Measured snapshot/);assert.equal(p.e.sectionRun.disabled,true);
  p.e.photo.handlers.change();assert.match(p.e.sectionWidthSource.textContent,/Measured snapshot/);
  p.e.sectionDepthFile.files=[file(' {"side":"saved"}\r\n','side.json')];p.e.sectionDepthFile.handlers.change();
  assert.equal(p.e.sectionRun.disabled,true);p.e.sectionConsent.checked=true;p.e.sectionConsent.handlers.change();
  assert.equal(p.e.sectionRun.disabled,false);const w=await sectionResult(p);
  assert.equal(w.messages[0].width,exact);assert.equal(w.messages[0].depth,' {"side":"saved"}\r\n');
  assert.equal(p.e.sectionResult.hidden,false);assert.match(p.e.sectionSummary.textContent,/484\.2.*14\.6.*PHOTO_ESTIMATED/);
  assert.match(p.e.sectionUncertainty.textContent,/6\.99.*7\.87.*10\.05/);
  p.e.sectionWidthFile.files=[file('replacement','front.json')];p.e.sectionWidthFile.handlers.change();
  assert.equal(p.e.sectionConsent.checked,false);assert.equal(p.e.sectionRun.disabled,true);
  assert.equal(p.e.sectionResult.hidden,true);assert.equal(p.e.sectionDownloads.children.length,0);
  p.e.sectionClear.onclick();assert.equal(p.e.sectionClear.disabled,true);assert.equal(p.e.sectionWidthFile.files.length,0);
});

test('section download and one-use handoff retain exact source bytes and invalidate only the edited evidence',async()=>{
  const p=await photoPage();selectSection(p);const w=await sectionResult(p),m=w.messages[0];
  for(const [name,expected] of [['section-front-measurements.json',m.width],['section-side-measurements.json',m.depth],['section-options.json',m.options]]) {
    const a=p.e.sectionDownloads.children.find(a=>a.download===name);assert.ok(a);assert.equal(await p.urls.get(a.href).text(),expected);
  }
  const exact=p.e.sectionReport.textContent;p.e.sectionWorkbench.onclick();const popup=p.popups[0];
  p.e.points.handlers.input();assert.equal(p.e.sectionWorkbench.disabled,false,'retained section snapshots are independent of photo edits');
  const ready=target=>p.windowEvents.message({origin:'https://unit.invalid',source:target.target,data:{type:'ftx-photo-records-ready',token:new URL(target.url).searchParams.get('photoImport')}});
  ready(popup);assert.equal(await popup.messages[0].data.records.text(),exact);assert.equal(popup.messages[0].data.type,'ftx-photo-records');
  p.e.sectionWorkbench.onclick();const stale=p.popups[1];p.e.sectionLevel.value='hip';p.e.sectionLevel.handlers.change();ready(stale);
  assert.equal(stale.messages[0].data.type,'ftx-photo-records-cancelled');assert.equal(p.e.sectionWorkbench.disabled,true);
  p.select(false);await p.analyze();await p.measure();const photo=p.e.report.textContent;p.e.openWorkbench.onclick();
  p.e.sectionClothing.handlers.input();ready(p.popups[2]);assert.equal(await p.popups[2].messages[0].data.records.text(),photo);
});

test('section calibration keeps coefficients separate from measurement handoff and sends only active settings',async()=>{
  const p=await photoPage();selectSection(p);await sectionResult(p);
  p.e.sectionMode.value='calibrate';p.e.sectionMode.handlers.change();p.e.sectionTape.value='505';
  p.e.sectionK.value='9';p.e.sectionKSigma.value='9';p.e.sectionRatio.value='9';p.e.sectionRatioSigma.value='9';
  const report={level:'waist',k_child:1.1058,k_child_sigma:0.0197,k_ratio:1.0432,k_ratio_sigma:0.0186};
  const w=await sectionResult(p,report);
  assert.deepEqual(JSON.parse(w.messages[0].options),{level:'waist',same_wearer:true,clothing_mm:0,tape_girth_mm:505,sigma_tape_mm:10});
  assert.equal(p.e.sectionGirthControls.hidden,true);assert.equal(p.e.sectionTapeControls.hidden,false);
  assert.equal(p.e.sectionWorkbench.disabled,true);assert.equal(p.e.sectionMaterials.disabled,true);
  p.e.sectionWorkbench.onclick();assert.equal(p.popups.length,0);
  assert.ok(p.e.sectionDownloads.children.some(a=>a.download==='section-calibration.json'));
  assert.ok(!p.e.sectionDownloads.children.some(a=>a.download==='section-measurements.json'));
  assert.match(p.e.sectionNotes.textContent,/not another independent/);
  p.e.sectionTape.value='';p.e.sectionTape.handlers.input();const count=p.workers.length;await p.e.sectionRun.onclick();
  assert.equal(p.workers.length,count);assert.match(p.e.sectionError.textContent,/finite numbers/);
});

test('section source bounds, invalid UTF-8, combined documents and native refusals remain explicit',async()=>{
  const p=await photoPage();selectSection(p);let reads=0;
  p.e.sectionWidthFile.files=[{name:'large.json',size:16*1024*1024,arrayBuffer:async()=>{reads++;}}];p.e.sectionWidthFile.handlers.change();
  p.e.sectionConsent.checked=true;await p.e.sectionRun.onclick();assert.equal(reads,0);assert.equal(p.workers.length,0);
  assert.match(p.e.sectionError.textContent,/16 MiB/);
  selectSection(p);p.e.sectionWidthFile.files=[{name:'bad.json',size:2,arrayBuffer:async()=>new Uint8Array([0xc3,0x28]).buffer}];
  p.e.sectionWidthFile.handlers.change();p.e.sectionConsent.checked=true;await p.e.sectionRun.onclick();assert.equal(p.workers.length,0);
  assert.notEqual(p.e.sectionError.textContent,'');
  selectSection(p);p.e.sectionCombined.checked=true;p.e.sectionCombined.handlers.change();assert.equal(p.e.sectionConsent.checked,false);
  p.e.sectionConsent.checked=true;p.e.sectionConsent.handlers.change();await p.e.sectionRun.onclick();
  const w=p.current();assert.equal(w.messages[0].depth,'');assert.equal(p.e.sectionDepthControls.hidden,true);
  await w.emit({type:'error',request:w.messages[0].request,message:'conflicting photo dimensions require REMEASURE'});
  assert.equal(p.e.sectionResult.hidden,true);assert.equal(p.e.sectionDownloads.children.length,0);
  assert.match(p.e.sectionError.textContent,/REMEASURE/);assert.equal(p.e.sectionRun.disabled,false);
});

test('section cancellation and edits discard late reads or worker replies without stopping newer work',async()=>{
  for(const reject of [false,true])for(const action of ['cancel','source','level','consent','clear']) {
    const p=await photoPage();selectSection(p);let resolve,rejectRead;
    p.e.sectionWidthFile.files=[{name:'held.json',size:10,arrayBuffer:()=>new Promise((yes,no)=>{resolve=yes;rejectRead=no;})}];
    p.e.sectionWidthFile.handlers.change();p.e.sectionConsent.checked=true;
    const reading=p.e.sectionRun.onclick();assert.equal(p.e.sectionCancel.disabled,false);
    if(action==='cancel')p.e.sectionCancel.onclick();
    else if(action==='source')p.e.sectionDepthFile.handlers.change();
    else if(action==='level')p.e.sectionLevel.handlers.change();
    else if(action==='consent'){p.e.sectionConsent.checked=false;p.e.sectionConsent.handlers.change();}
    else p.e.sectionClear.onclick();
    selectSection(p);await p.e.sectionRun.onclick();const current=p.current();
    if(reject)rejectRead(Error('late read failed'));else resolve(new TextEncoder().encode('stale').buffer);
    await reading;assert.equal(current.dead,undefined);assert.equal(p.workers.length,1);assert.equal(p.e.sectionResult.hidden,true);
    p.e.sectionCancel.onclick();assert.equal(current.dead,true);
    await current.emit({type:'result',request:current.messages[0].request,report:JSON.stringify(sectionReport())});
    assert.equal(p.e.sectionResult.hidden,true);assert.equal(p.e.sectionDownloads.children.length,0);
  }
});

test('section evidence can return to its material session and page exit withdraws pending transfers',async()=>{
  const token='ab'.repeat(16),p=await photoPage({materialsToken:token});selectSection(p);await sectionResult(p);
  assert.equal(p.e.sectionMaterials.hidden,false);assert.equal(p.e.sectionMaterials.disabled,false);
  const exact=p.e.sectionReport.textContent;p.e.sectionMaterials.onclick();assert.equal(p.openerMessages[0].data.type,'ftx-photo-records-offer');
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-photo-records-ready',token}});
  assert.equal(await p.openerMessages[1].data.records.text(),exact);
  p.windowEvents.message({origin:'https://unit.invalid',source:p.opener,data:{type:'ftx-materials-photo-received',token}});
  assert.equal(p.e.sectionMaterials.disabled,true);
  p.e.sectionWorkbench.onclick();const popup=p.popups[0];p.windowEvents.pagehide();
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-photo-records-ready',token:new URL(popup.url).searchParams.get('photoImport')}});
  assert.equal(popup.messages[0].data.type,'ftx-photo-records-cancelled');assert.equal(p.e.sectionResult.hidden,true);
});

function triangulationReport(overrides={}) {
  return {schema:'ftx-triangulation/1',status:'ACCEPTED',point_m:[0,0,1.3],fit_views:2,held_out_views:1,
    flags:[],advisories:[],reprojection:[],no_claim:'Synthetic conditional geometry only',...overrides};
}

test('triangulation worker uses exact source strings and never falls back to photo analysis',async()=>{
  const source={request:17,op:'triangulate',input:' \r\n{"timestamp_us":18446744073709551615}\r\n',noise:' {"noise":"declared λ"}\r\n'};
  const w=await photoWorker();assert.equal((await w.send(source)).type,'result');
  assert.deepEqual(w.calls,[{kind:'triangulate',input:source.input,noise:source.noise}]);assert.equal(w.instances.length,0);
  const failed=await photoWorker({failTriangulation:true});const result=await failed.send(source);
  assert.equal(result.type,'error');assert.match(result.message,/capture evidence refused/);assert.equal(failed.calls.length,1);
});

test('triangulation page retains exact reports and sources, including full-width timestamps, without creating photo records',async()=>{
  const p=await photoPage();assert.equal(p.e.triangulationRun.disabled,true);
  const input=' \r\n{"timestamp_us":18446744073709551615}\r\n',noise=' {"sigma":"original λ"}\r\n';
  p.e.triangulationInput.files=[file(input)];p.e.triangulationNoise.files=[file(noise)];p.e.triangulationInput.handlers.change();
  assert.equal(p.e.triangulationRun.disabled,false);await p.e.triangulationRun.onclick();const w=p.current(),message=w.messages[0];
  assert.equal(message.input,input);assert.equal(message.noise,noise);assert.equal(message.op,'triangulate');
  assert.equal(p.e.triangulationRun.disabled,true);assert.equal(p.e.triangulationCancel.disabled,false);
  const raw=JSON.stringify(triangulationReport()).replace('"reprojection":[]','"reprojection":[{"frame":{"timestamp_us":18446744073709551615}}]');
  await w.emit({type:'result',request:message.request,report:raw});
  assert.equal(p.e.triangulationReport.textContent,raw);assert.equal(p.e.triangulationResult.hidden,false);
  assert.equal(p.e.triangulationRun.disabled,false);assert.equal(p.e.triangulationCancel.disabled,true);
  assert.match(p.e.triangulationPrecision.textContent,/No click uncertainty/);
  const downloads=p.e.triangulationDownloads.children;
  assert.deepEqual(await Promise.all(downloads.map(a=>p.urls.get(a.href).text())),[raw,input,noise]);
  assert.equal(p.e.openWorkbench.disabled,true);assert.equal(p.e.report.textContent,'');
  p.e.triangulationNoise.handlers.change();assert.equal(p.urls.size,0);assert.equal(p.e.triangulationResult.hidden,true);
  await w.emit({type:'result',request:message.request,report:raw});assert.equal(p.e.triangulationResult.hidden,true);
});

test('triangulation page shows unavailable precision separately and retains rejected diagnostics',async()=>{
  const p=await photoPage();p.e.triangulationInput.files=[file('{}')];p.e.triangulationInput.handlers.change();
  for(const report of [triangulationReport({conditional_click_uncertainty:{status:'UNAVAILABLE',sigma_m:null,flags:['lens boundary'],no_claim:'click only'}}),
    triangulationReport({status:'UNOBSERVABLE',point_m:null,flags:['weak parallax']})]) {
    await p.e.triangulationRun.onclick();const w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(report)});
    assert.equal(p.e.triangulationResult.hidden,false);assert.equal(p.e.triangulationDownloads.children.length,2);
    if(report.point_m) {assert.match(p.e.triangulationStatus.textContent,/requested click uncertainty is unavailable/);assert.match(p.e.triangulationPrecision.textContent,/No covariance estimate/);}
    else {assert.match(p.e.triangulationStatus.textContent,/not accepted/);assert.match(p.e.triangulationPoint.textContent,/No usable point/);}
    assert.equal(p.e.openWorkbench.disabled,true);
  }
});

test('triangulation cancels late reads and stale worker events without withdrawing independent photo evidence',async()=>{
  const p=await photoPage();p.select();await p.analyze();await p.measure();const photo=p.e.report.textContent;
  p.e.triangulationInput.files=[file('{}')];p.e.triangulationInput.handlers.change();await p.e.triangulationRun.onclick();
  const old=p.current(),request=old.messages[0].request;
  await old.emit({type:'result',request:request+1,report:JSON.stringify(triangulationReport())});assert.equal(p.e.triangulationResult.hidden,true);
  p.e.triangulationCancel.onclick();assert.equal(old.dead,true);
  await old.emit({type:'result',request,report:JSON.stringify(triangulationReport())});assert.equal(p.e.triangulationResult.hidden,true);
  assert.equal(p.e.report.textContent,photo);assert.equal(p.e.openWorkbench.disabled,false);
  let finish;const count=p.workers.length;
  p.e.triangulationInput.files=[{size:2,arrayBuffer:()=>new Promise(resolve=>{finish=resolve;})}];
  const pending=p.e.triangulationRun.onclick();p.e.triangulationInput.handlers.change();finish(new TextEncoder().encode('{}').buffer);await pending;
  assert.equal(p.workers.length,count);assert.equal(p.e.triangulationResult.hidden,true);
  p.e.triangulationInput.files=[file('{}')];await p.e.triangulationRun.onclick();const live=p.current();
  p.windowEvents.pagehide();await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(triangulationReport())});
  assert.equal(p.e.triangulationResult.hidden,true);assert.equal(p.urls.size,0);
});

test('triangulation preflights both byte budgets, rejects empty/invalid noise files and clears failed outputs',async()=>{
  for(const oversized of ['triangulationInput','triangulationNoise']) {
    const p=await photoPage();let reads=0;p.e.triangulationInput.files=[{size:2,arrayBuffer:async()=>{reads++;return new TextEncoder().encode('{}').buffer;}}];
    p.e[oversized].files=[{size:256*1024+1,arrayBuffer:async()=>{reads++;throw Error('must not read');}}];
    await p.e.triangulationRun.onclick();assert.equal(reads,0);assert.equal(p.workers.length,0);assert.match(p.e.triangulationError.textContent,/256 KiB/);
  }
  const p=await photoPage();p.e.triangulationInput.files=[file('{}')];p.e.triangulationNoise.files=[file('')];
  await p.e.triangulationRun.onclick();assert.equal(p.workers.length,0);assert.match(p.e.triangulationError.textContent,/file is empty/);
  p.e.triangulationClearNoise.onclick();assert.equal(p.e.triangulationNoise.files.length,0);
  p.e.triangulationNoise.files=[{size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer}];await p.e.triangulationRun.onclick();assert.equal(p.workers.length,0);
  p.e.triangulationClearNoise.onclick();await p.e.triangulationRun.onclick();const w=p.current();
  await w.emit({type:'error',request:w.messages[0].request,message:'native capture refusal'});
  assert.equal(p.e.triangulationResult.hidden,true);assert.equal(p.urls.size,0);assert.match(p.e.triangulationError.textContent,/native capture refusal/);
  await p.e.triangulationRun.onclick();p.current().onerror({message:'worker crash'});assert.equal(p.e.triangulationResult.hidden,true);assert.equal(p.urls.size,0);
});

function bundleReport(overrides={}) {
  return {schema:'ftx-bundle/1',status:'ACCEPTED',solution:{views:[],landmarks:[{id:'p',position_m:[0,-0.4,1.2]}]},
    input:{views:[{},{},{}],landmarks:[{id:'p'}]},iterations:7,objective_history:[100,0],checks:[],flags:[],advisories:[],no_claim:'Synthetic local anchored geometry only',...overrides};
}

test('bundle worker forwards exact raw sources and textual limits without invoking another estimator',async()=>{
  const source={request:21,op:'bundle-adjust',input:' \r\n{"timestamp_us":18446744073709551615}\r\n',iterations:'50.5',noise:'must not use'};
  const w=await photoWorker();assert.equal((await w.send(source)).type,'result');
  assert.deepEqual(w.calls,[{kind:'bundle-adjust',input:source.input,iterations:'50.5'}]);assert.equal(w.instances.length,0);
  const failed=await photoWorker({failBundle:true});assert.equal((await failed.send(source)).type,'error');
  assert.deepEqual(failed.calls.map(call=>call.kind),['bundle-adjust']);
});

test('bundle mode clears separate noise and retains exact input, limit and native report downloads',async()=>{
  const p=await photoPage(),input=' \r\n{"bundle":"original λ","timestamp_us":18446744073709551615}\r\n';
  p.e.triangulationInput.files=[file(input)];p.e.triangulationNoise.files=[file('old triangulation noise')];
  p.e.geometryOperation.value='bundle-adjust';p.e.geometryOperation.handlers.change();
  assert.equal(p.e.triangulationNoise.files.length,0);assert.equal(p.e.triangulationNoise.disabled,true);
  assert.equal(p.e.geometryNoiseControls.hidden,true);assert.equal(p.e.geometryBundleControls.hidden,false);
  assert.equal(p.e.geometryInputKind.textContent,'ftx-bundle-input/1');p.e.bundleIterations.value='050';
  await p.e.triangulationRun.onclick();const w=p.current(),message=w.messages[0];
  assert.equal(message.op,'bundle-adjust');assert.equal(message.input,input);assert.equal(message.iterations,'050');assert.equal(message.noise,'');
  const raw=JSON.stringify(bundleReport());await w.emit({type:'result',request:message.request,report:raw});
  assert.match(p.e.triangulationPoint.textContent,/Refined landmarks/);assert.match(p.e.triangulationSummary.textContent,/limit 050/);
  assert.match(p.e.triangulationSummary.textContent,/normalized residual sum of squares: 100 → 0/);
  const links=p.e.triangulationDownloads.children;
  assert.deepEqual(links.map(a=>a.download),['bundle.json','bundle-input.json','bundle-iterations.txt']);
  assert.deepEqual(await Promise.all(links.map(a=>p.urls.get(a.href).text())),[raw,input,'050']);
  assert.equal(p.e.openWorkbench.disabled,true);assert.match(p.e.triangulationPrecision.textContent,/no camera.*covariance/);
  p.e.geometryOperation.value='triangulate';p.e.geometryOperation.handlers.change();
  assert.equal(p.urls.size,0);assert.equal(p.e.triangulationResult.hidden,true);assert.equal(p.e.bundleIterations.disabled,true);
  assert.equal(p.e.geometryInputKind.textContent,'ftx-triangulation-input/1');
  await w.emit({type:'result',request:message.request,report:raw});assert.equal(p.e.triangulationResult.hidden,true);
});

test('bundle budget edits cancel reads/workers and failed solves expose diagnostics without initial guesses as results',async()=>{
  const p=await photoPage();p.e.geometryOperation.value='bundle-adjust';p.e.geometryOperation.handlers.change();p.e.triangulationInput.files=[file('{}')];
  for(const status of ['BUDGET_STOP','UNOBSERVABLE','REJECTED']) {
    await p.e.triangulationRun.onclick();const w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bundleReport({status,solution:null}))});
    assert.equal(p.e.triangulationResult.hidden,false);assert.match(p.e.triangulationPoint.textContent,/No optimized geometry/);
    assert.match(p.e.triangulationPoint.textContent,/initial guesses/);assert.equal(p.e.triangulationDownloads.children.length,3);
    assert.equal(p.e.openWorkbench.disabled,true);
  }
  await p.e.triangulationRun.onclick();const old=p.current();p.e.bundleIterations.value='1';p.e.bundleIterations.handlers.input();
  assert.equal(old.dead,true);await old.emit({type:'result',request:old.messages[0].request,report:JSON.stringify(bundleReport())});
  assert.equal(p.urls.size,0);assert.equal(p.e.triangulationResult.hidden,true);
  let finish;const count=p.workers.length;p.e.triangulationInput.files=[{size:2,arrayBuffer:()=>new Promise(resolve=>{finish=resolve;})}];
  const pending=p.e.triangulationRun.onclick();p.e.geometryOperation.value='triangulate';p.e.geometryOperation.handlers.change();
  finish(new TextEncoder().encode('{}').buffer);await pending;assert.equal(p.workers.length,count);
  p.e.geometryOperation.value='unsupported';await p.e.triangulationRun.onclick();assert.equal(p.workers.length,count);
  assert.match(p.e.triangulationError.textContent,/supported geometry operation/);
});

async function compiledPhotoBindings(){
  const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(root,'web/pkg');
  const ftx=await import(require('node:url').pathToFileURL(path.join(pkg,'ftx_wasm.js')).href);
  ftx.initSync({module:fs.readFileSync(path.join(pkg,'ftx_wasm_bg.wasm'))});return ftx;
}

function stillReport(overrides={}) {
  return {schema:'ftx-still-window/1',status:'STILL_CANDIDATE',input:{frames:[{},{},{},{}],max_motion_mm:5},
    fit_landmarks:3,held_out_landmarks:1,checks:[{name:'held',partition:'held_out',motion_mm:0,passed:true,flags:[]}],
    flags:[],advisories:[],no_claim:'Human review required',...overrides};
}

function bodyFitReport(overrides={}) {
  return {schema:'ftx-body-fit/1',status:'ACCEPTED',input:{frames:[{frame:{stream:'synthetic',timestamp_us:1}}]},
    model:{measurements:['stature']},model_evaluations:42,flags:[],advisories:['Conditional on supplied cameras'],
    attempts:[{status:'CONVERGED',model_evaluations:42,cost_history:[8,0.01],flags:[],checks:[
      {frame:{stream:'synthetic',timestamp_us:1},name:'shoulder',partition:'held_out',unit:'px',residual:0.5,limit:3,passed:true},
      {frame:null,name:'stature',partition:'fit',unit:'m',residual:0.001,limit:0.006,passed:true}]}],
    solution:{shape:[0.6],poses:[],predicted_measurements_m:[1.3]},no_claim:'Not measured anatomy or physical fit.',...overrides};
}
function bodyFrameReport(overrides={}) {
  return {schema:'ftx-body-frame-review/1',frame_index:0,frame:{stream:'synthetic',timestamp_us:1},
    frame_label:'synthetic @ 18446744073709551615 us',image_size_px:[640,480],rotation_cw_deg:90,
    camera:{source:'native'},mesh_obj:'# exact native MODEL_FIT\nv 0.12345678912345678 -0 2\nf 1 2 3\n',
    triangles:[[0,1,2]],projected_vertices_px:[[20,30],[70,30],[20,80],null],unprojectable_vertices:1,
    projection_kind:'straight_edges',keypoints:[
      {name:'shoulder',partition:'held_out',observed_px:[20,30],predicted_px:[20.5,30],sigma_px:1,residual_px:0.5},
      {name:'waist',partition:'fit',observed_px:[30,40],predicted_px:[30,40],sigma_px:1,residual_px:0}],
    silhouette:{lines:[{observation:{axis:'horizontal',at_px:70,partition:'held_out',intervals_px:[[10,40]]},
      comparison:{predicted_intervals_px:[[11,41]],passed:true}}]},no_claim:'Not full-mask, occlusion or real-wearer fit.',...overrides};
}
function bodyStartFrameReport(overrides={}) {
  return bodyFrameReport({schema:'ftx-body-start-frame-review/1',status:'INITIALIZATION_REVIEW',body_fitted:false,
    start_index:0,start_count:2,frame_count:2,pose_evaluations:1,
    shape_parameters:[{name:'height',lower:0,upper:1,unit:'coefficient'}],shape_values:[0.6],
    pose_parameters:[{name:'root_tx',lower:-3,upper:3,unit:'m'}],pose_values:[0.2],diagnostics:[],
    mesh_obj:'# declared INITIALIZATION; no fitted body\nv 0.12345678912345678 -0 2\nf 1 2 3\n',
    no_claim:'Declared starting pose only. No optimization or acceptance was performed.',...overrides});
}
function selectBodyFit(p,input=' {"timestamp_us":18446744073709551615,"evidence":"λ"}\r\n') {
  p.e.bodyFitModel.files=[Object.assign(new Blob(['original model']),{name:'model.ftxbody'})];p.e.bodyFitInput.files=[file(input)];
  p.e.bodyFitInput.handlers.change();return input;
}
function selectSavedBodyFit(p,report=JSON.stringify(bodyFitReport())) {
  p.e.bodyFitModel.files=[Object.assign(new Blob(['original model']),{name:'model.ftxbody'})];
  p.e.bodyFitSaved.files=[file(report,'accepted-fit.json')];p.e.bodyFitSaved.handlers.change();return report;
}
async function openCaptureBodyReview(p,overrides={}) {
  selectSavedBodyFit(p);await p.e.bodyFitOpen.onclick();
  const source_image={frame:{stream:'synthetic',timestamp_us:1},image_name:'capture & <original>.png',
    image_sha256:'ab'.repeat(32),image_size_px:[640,480],rotation_cw_deg:90};
  const review=' \r\n'+JSON.stringify(bodyFrameReport({source_image,...overrides}))
    .replaceAll('"timestamp_us":1','"timestamp_us":18446744073709551615')+'\r\n';
  const w=p.current();await w.emit({type:'result',request:w.messages[0].request,report:review});return review;
}
function selectBodyReviewImage(p,image=file('original capture bytes','capture.png')) {
  p.e.bodyFitReviewImage.files=[image];p.e.bodyFitReviewImage.handlers.change();
}
function bodyStartsReport(overrides={}) {
  return {schema:'ftx-body-starts-review/1',input:'{}',input_sha256:'a'.repeat(64),original_input_sha256:'a'.repeat(64),model_sha256:'c'.repeat(64),
    shape:[{name:'height <λ>',lower:0,upper:1,unit:'dimensionless'}],pose:[{name:'root_x_m',lower:-3,upper:3,unit:'m'}],
    frames:[{frame_index:0,label:'Frame 1: late <λ> @ 18446744073709551615 us',identity:{stream:'late <λ>',timestamp_us:'18446744073709551615'}},
      {frame_index:1,label:'Frame 2: early @ 9007199254740993 us',identity:{stream:'early',timestamp_us:'9007199254740993'}}],
    starts:[{shape:[0.4],poses:[{frame_index:0,values:[0.1]},{frame_index:1,values:[0.2]}]},
      {shape:[0.6],poses:[{frame_index:0,values:[0.15]},{frame_index:1,values:[0.25]}]}],...overrides};
}
function bodyStartField(p,id) {
  const descend=node=>[node,...node.children.flatMap(descend)];
  const input=descend(p.e.bodyStartsFields).find(node=>node.id===id);assert.ok(input,`missing start field ${id}`);return input;
}
function editBodyStart(p,id,value) {const input=bodyStartField(p,id);input.value=value;input.handlers.input();}
async function loadBodyStarts(p,overrides={}) {
  await p.e.bodyStartsLoad.onclick();const w=p.current(),message=w.messages[0];
  assert.equal(message.op,'body-fit-starts');assert.equal(message.edits,'');
  const view=bodyStartsReport({input:message.input,...overrides});
  await w.emit({type:'result',request:message.request,report:JSON.stringify(view)});return view;
}

test('body-start editor worker preserves source and edit bytes and refuses transport limits without another estimator',async()=>{
  const source={request:91,op:'body-fit-starts',model:new Uint8Array([8,5,3]).buffer,
    input:' \r\n{"timestamp_us":18446744073709551615,"evidence":"λ"}\r\n',edits:' {"starts":[]}\r\n'};
  const w=await photoWorker(),result=await w.send(source);
  assert.equal(result.type,'result');assert.equal(result.request,91);
  assert.deepEqual(w.calls,[{kind:'body-starts',model:[8,5,3],input:source.input,edits:source.edits}]);
  assert.equal(JSON.parse(result.report).input,source.input);assert.equal(w.instances.length,0);
  const failed=await photoWorker({bodyStarts:()=>{throw Error('duplicate starts refused');}});
  assert.match((await failed.send(source)).message,/duplicate starts refused/);
  for(const bad of [{input:null},{input:' '.repeat(16*1024*1024+1)},{edits:null},{edits:' '.repeat(64*1024+1)},
    {model:[]},{model:new ArrayBuffer(0)},{model:new ArrayBuffer(512*1024*1024+1)}]) {
    const invalid=await photoWorker();assert.equal((await invalid.send({...source,...bad})).type,'error');assert.equal(invalid.calls.length,0);
  }
});

test('body-start editor applies every named pose natively and uses exact edited input for preview fit and download',async()=>{
  const p=await photoPage(),original=selectBodyFit(p),view=await loadBodyStarts(p);
  assert.equal(p.e.bodyStartsEditor.hidden,false);assert.equal(p.e.bodyStartsFields.children[1].children[0].textContent,view.frames[0].label);
  assert.match(p.e.bodyStartsFields.children[0].children[1].children[0].textContent,/height <λ>/);
  editBodyStart(p,'bodyStart-shape-0','0.45');editBodyStart(p,'bodyStart-pose-1-0','-0');
  p.e.bodyStartsSelect.value='1';p.e.bodyStartsSelect.handlers.change();editBodyStart(p,'bodyStart-shape-0','0.65');
  p.e.bodyStartsSelect.value='0';p.e.bodyStartsSelect.handlers.change();assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.45');
  await p.e.bodyStartsApply.onclick();const apply=p.current(),message=apply.messages[0],edits=JSON.parse(message.edits);
  assert.equal(message.input,original);assert.equal(new TextDecoder().decode(message.model),'original model');
  assert.equal(edits.input_sha256,view.original_input_sha256);assert.equal(edits.model_sha256,view.model_sha256);
  assert.deepEqual(edits.starts.map(start=>start.shape),[[0.45],[0.65]]);assert.ok(Object.is(edits.starts[0].poses[1].values[0],-0));
  assert.deepEqual(edits.starts[0].poses.map(pose=>pose.frame_index),[0,1]);assert.equal(p.e.bodyFitRun.disabled,true);
  const native=' {"timestamp_us":18446744073709551615,"other_frame":9007199254740993,"edited":"native λ"}\r\n';
  await apply.emit({type:'result',request:message.request,report:JSON.stringify({...view,input:native,input_sha256:'b'.repeat(64),starts:edits.starts})});
  assert.equal(p.e.bodyStartsError.textContent,'');assert.equal(p.e.bodyFitRun.disabled,false);assert.equal(p.e.bodyStartsApply.disabled,true);
  assert.equal(await p.urls.get(p.e.bodyStartsDownloads.children[0].href).text(),native);
  assert.equal(await new Blob([await p.e.bodyFitInput.files[0].arrayBuffer()]).text(),original);
  await p.e.bodyFitStartRun.onclick();const preview=p.current();assert.equal(preview.messages[0].input,native);
  await p.e.bodyFitRun.onclick();const fit=p.current();assert.equal(preview.dead,true);assert.equal(fit.messages[0].input,native);
  await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport())});
  assert.equal(await p.urls.get(p.e.bodyFitDownloads.children.find(a=>a.download==='body-fit-input.json').href).text(),native);
  p.e.bodyStartsReset.onclick();assert.equal(p.e.bodyStartsEditor.hidden,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.urls.size,0);
  await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,original);
});

test('body-start draft edits withdraw accepted handoffs and invalid applies cannot fit or restore stale output',async()=>{
  const p=await photoPage();selectBodyFit(p);await loadBodyStarts(p);await p.e.bodyFitRun.onclick();let w=p.current();
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFitReport())});
  p.e.bodyFitWorkbench.onclick();const popup=p.popups[0];
  editBodyStart(p,'bodyStart-shape-0','1');
  assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.urls.size,0);
  assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.e.bodyFitStartRun.disabled,true);
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token:new URL(popup.url).searchParams.get('bodyFitImport')}});
  assert.equal(popup.messages[0].data.type,'ftx-body-fit-sources-cancelled');
  let count=p.workers.length;await p.e.bodyStartsApply.onclick();assert.equal(p.workers.length,count);assert.match(p.e.bodyStartsError.textContent,/strictly between/);
  await p.e.bodyFitRun.onclick();await p.e.bodyFitStartRun.onclick();assert.equal(p.workers.length,count);
  editBodyStart(p,'bodyStart-shape-0','0.49');await p.e.bodyStartsApply.onclick();w=p.current();
  await w.emit({type:'error',request:w.messages[0].request,message:'native evidence refused'});
  assert.match(p.e.bodyStartsError.textContent,/native evidence refused/);assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
  p.e.bodyStartsCancel.onclick();assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.4');
  assert.equal(p.e.bodyFitRun.disabled,false);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitWorkbench.disabled,true);
});

test('body-start editor creates empty starts explicitly and separates one-start preview from fitting',async()=>{
  const p=await photoPage();selectBodyFit(p);const view=await loadBodyStarts(p,{starts:[]});
  assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.e.bodyFitStartRun.disabled,true);
  p.e.bodyStartsAdd.onclick();assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'');
  assert.equal(bodyStartField(p,'bodyStart-pose-0-0').value,'');await p.e.bodyStartsApply.onclick();assert.equal(p.workers.length,1);
  assert.match(p.e.bodyStartsError.textContent,/finite/);p.e.bodyStartsMidpoint.onclick();
  assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.5');assert.equal(bodyStartField(p,'bodyStart-pose-1-0').value,'0');
  await p.e.bodyStartsApply.onclick();let w=p.current(),edits=JSON.parse(w.messages[0].edits);
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify({...view,input:'{"one_start":"native"}',input_sha256:'b'.repeat(64),starts:edits.starts})});
  assert.equal(p.e.bodyFitStartRun.disabled,false);assert.equal(p.e.bodyFitRun.disabled,true);
  const count=p.workers.length;await p.e.bodyFitRun.onclick();assert.equal(p.workers.length,count);
  p.e.bodyStartsCopy.onclick();editBodyStart(p,'bodyStart-shape-0','0.7');await p.e.bodyStartsApply.onclick();
  w=p.current();edits=JSON.parse(w.messages[0].edits);assert.equal(edits.starts.length,2);
  assert.equal(w.messages[0].input,'{"one_start":"native"}');assert.equal(edits.input_sha256,'b'.repeat(64));
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify({...view,input:'{"two_starts":"native"}',input_sha256:'d'.repeat(64),original_input_sha256:'b'.repeat(64),starts:edits.starts})});
  assert.equal(p.e.bodyFitRun.disabled,false);p.e.bodyStartsCopy.onclick();p.e.bodyStartsCopy.onclick();
  assert.equal(p.e.bodyStartsAdd.disabled,true);assert.equal(p.e.bodyStartsCopy.disabled,true);
  p.e.bodyStartsRemove.onclick();assert.equal(p.e.bodyStartsAdd.disabled,false);
  p.e.bodyStartsCancel.onclick();assert.equal(p.e.bodyStartsSelect.children.length,2);assert.equal(p.e.bodyFitRun.disabled,false);
});

test('body-start editor cancellation and source changes ignore old workers and native refusals',async()=>{
  for(const invalidate of [p=>p.e.bodyStartsCancel.onclick(),p=>p.e.bodyStartsReset.onclick(),p=>p.e.bodyFitInput.handlers.change(),
    p=>p.e.bodyFitModel.handlers.click(),p=>p.e.bodyFitInput.handlers.cancel(),p=>p.windowEvents.pagehide()]) {
    for(const late of ['result','error','crash']) {
      const p=await photoPage();selectBodyFit(p);const view=await loadBodyStarts(p);
      editBodyStart(p,'bodyStart-shape-0','0.45');await p.e.bodyStartsApply.onclick();const old=p.current(),request=old.messages[0].request;
      invalidate(p);const message=p.e.bodyStartsStatus.textContent,downloadCount=p.urls.size;
      if(late==='crash')old.onerror({message:'old crash'});
      else await old.emit(late==='error'?{type:'error',request,message:'old refusal'}:
        {type:'result',request,report:JSON.stringify({...view,input:'{"stale":true}',input_sha256:'b'.repeat(64)})});
      assert.equal(old.dead,true);assert.equal(p.e.bodyStartsStatus.textContent,message);assert.equal(p.e.bodyStartsError.textContent,'');
      assert.equal(p.urls.size,downloadCount);assert.equal(p.e.bodyFitResult.hidden,true);
    }
  }
});

test('body-start source read races and inconsistent native replies never replace the checked editor',async()=>{
  for(const which of ['bodyFitInput','bodyFitModel'])for(const rejected of [false,true]) {
    const p=await photoPage();selectBodyFit(p);let finish,fail,entered;
    const reading=new Promise(resolve=>{entered=resolve;});
    p.e[which].files=[{size:2,arrayBuffer:()=>{entered();return new Promise((yes,no)=>{finish=yes;fail=no;});}}];
    const pending=p.e.bodyStartsLoad.onclick();await reading;p.e.bodyStartsCancel.onclick();
    selectBodyFit(p);await loadBodyStarts(p);const message=p.e.bodyStartsStatus.textContent;
    if(rejected)fail(Error('stale read failed'));else finish(new TextEncoder().encode('{}').buffer);
    await pending;assert.equal(p.workers.length,1);assert.equal(p.e.bodyStartsStatus.textContent,message);
    assert.equal(p.e.bodyStartsError.textContent,'');assert.equal(p.e.bodyStartsEditor.hidden,false);
  }
  for(const override of [{input:'{"wrong":true}'},{model_sha256:'bad'},{frames:[]},{starts:[{shape:[1],poses:[]}]},
    {frames:[{frame_index:0,label:'rounded',identity:{stream:'x',timestamp_us:18446744073709551615}}]}]) {
    const p=await photoPage();selectBodyFit(p);await loadBodyStarts(p,override);
    assert.equal(p.e.bodyStartsEditor.hidden,true);assert.ok(p.e.bodyStartsError.textContent);assert.equal(p.urls.size,0);
  }
  const p=await photoPage();selectBodyFit(p);const view=await loadBodyStarts(p);
  editBodyStart(p,'bodyStart-shape-0','0.45');await p.e.bodyStartsApply.onclick();const w=p.current();
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify({...view,input:'{"changed":true}',input_sha256:'b'.repeat(64),original_input_sha256:'d'.repeat(64)})});
  assert.match(p.e.bodyStartsError.textContent,/source-bound/);assert.equal(p.e.bodyFitRun.disabled,true);
  p.e.bodyStartsCancel.onclick();assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.4');
});

test('body-start editor retains applied and pending values through source chooser cancellation until explicit recovery',async()=>{
  for(const source of ['bodyFitInput','bodyFitModel'])for(const pending of [false,true]) {
    const p=await photoPage(),original=selectBodyFit(p),view=await loadBodyStarts(p);
    editBodyStart(p,'bodyStart-shape-0','0.45');await p.e.bodyStartsApply.onclick();
    const applied=p.current(),request=applied.messages[0],native='{"applied":"exact native text"}';
    const starts=JSON.parse(request.edits).starts;
    await applied.emit({type:'result',request:request.request,report:JSON.stringify({...view,input:native,input_sha256:'b'.repeat(64),starts})});
    assert.equal(p.e.bodyStartsLoad.disabled,true);
    let count=p.workers.length;await p.e.bodyStartsLoad.onclick();assert.equal(p.workers.length,count);
    assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.45');
    if(pending)editBodyStart(p,'bodyStart-shape-0','0.49');
    for(const event of ['click','cancel']) {
      p.e[source].handlers[event]();assert.equal(p.e.bodyStartsEditor.hidden,false);
      assert.equal(bodyStartField(p,'bodyStart-shape-0').value,pending?'0.49':'0.45');
      assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.e.bodyFitStartRun.disabled,true);assert.equal(p.urls.size,0);
      count=p.workers.length;await p.e.bodyStartsLoad.onclick();assert.equal(p.workers.length,count);
    }
    p.e.bodyStartsCancel.onclick();assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.45');
    assert.equal(await p.urls.get(p.e.bodyStartsDownloads.children[0].href).text(),native);
    await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,native);
    p.e[source].handlers.change();assert.equal(p.e.bodyStartsEditor.hidden,true);assert.equal(p.urls.size,0);
    await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,original);
  }
});

test('body-start input events retire in-flight application workers and reads before newer values can be overwritten',async()=>{
  for(const late of ['result','error','crash']) {
    const p=await photoPage();selectBodyFit(p);const view=await loadBodyStarts(p);
    editBodyStart(p,'bodyStart-shape-0','0.45');await p.e.bodyStartsApply.onclick();
    const old=p.current(),request=old.messages[0].request;
    editBodyStart(p,'bodyStart-shape-0','0.47');const status=p.e.bodyStartsStatus.textContent;
    assert.equal(old.dead,true);
    if(late==='crash')old.onerror({message:'obsolete application crashed'});
    else await old.emit(late==='error'?{type:'error',request,message:'obsolete refusal'}:
      {type:'result',request,report:JSON.stringify({...view,input:'{"obsolete":true}',input_sha256:'b'.repeat(64)})});
    assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.47');assert.equal(p.e.bodyStartsStatus.textContent,status);
    assert.equal(p.e.bodyStartsError.textContent,'');assert.equal(p.urls.size,0);assert.equal(p.e.bodyFitRun.disabled,true);
    await p.e.bodyStartsApply.onclick();const current=p.current(),edits=JSON.parse(current.messages[0].edits);
    assert.equal(edits.starts[0].shape[0],0.47);assert.equal(edits.input_sha256,view.original_input_sha256);
    assert.equal(edits.model_sha256,view.model_sha256);
  }
  for(const rejected of [false,true]) {
    const p=await photoPage();selectBodyFit(p);await loadBodyStarts(p);let resolve,reject,entered;
    const reading=new Promise(done=>{entered=done;});
    p.e.bodyFitModel.files[0].arrayBuffer=()=>{entered();return new Promise((yes,no)=>{resolve=yes;reject=no;});};
    editBodyStart(p,'bodyStart-shape-0','0.45');const pending=p.e.bodyStartsApply.onclick();await reading;
    editBodyStart(p,'bodyStart-shape-0','0.48');const status=p.e.bodyStartsStatus.textContent;
    if(rejected)reject(Error('obsolete model read'));else resolve(new ArrayBuffer(4));
    await pending;assert.equal(p.workers.length,1);assert.equal(bodyStartField(p,'bodyStart-shape-0').value,'0.48');
    assert.equal(p.e.bodyStartsStatus.textContent,status);assert.equal(p.e.bodyStartsError.textContent,'');assert.equal(p.urls.size,0);
  }
});

function bodyPointsReport(overrides={}) {
  const image_review=' {"schema":"ftx-body-observation-frame-review/1","status":"OBSERVATION_REVIEW","body_fitted":false,"frame":{"stream":"synthetic","timestamp_us":18446744073709551615},"image_size_px":[640,480],"rotation_cw_deg":90,"source_image":{"image_name":"original.png"}}\r\n';
  return {schema:'ftx-body-observations-review/1',status:'OBSERVATION_REVIEW',body_fitted:false,input:'{}',input_sha256:'a'.repeat(64),original_input_sha256:'a'.repeat(64),model_sha256:'c'.repeat(64),start_count:2,
    targets:[{name:'p0 <λ>',definition:{kind:'vertex',index:0}},{name:'p1',definition:{kind:'vertex',index:1}}],
    frames:[{frame_index:0,label:'synthetic @ 18446744073709551615 us',image_size_px:[640,480],rotation_cw_deg:90,image_name:'original.png',image_review,
      silhouette:null,silhouette_editable:true,silhouette_reason:'Rectified manual silhouettes may be authored.',
      keypoints:[{name:'p0 <λ>',pixel_px:[10,20],sigma_px:1,partition:'held_out'}]},
      {frame_index:1,label:'second @ 9007199254740993 us',image_size_px:[640,480],rotation_cw_deg:0,image_name:null,image_review:null,keypoints:[],
        silhouette:null,silhouette_editable:true,silhouette_reason:'Rectified manual silhouettes may be authored.'}],...overrides};
}
async function loadBodyPoints(p,overrides={}) {
  await p.e.bodyPointsLoad.onclick();const w=p.current(),message=w.messages[0];
  assert.equal(message.op,'body-fit-observations');assert.equal(message.edits,'');
  const view=bodyPointsReport({input:message.input,...overrides});
  await w.emit({type:'result',request:message.request,report:JSON.stringify(view)});return view;
}
function chooseBodyPoint(p,name,frame=0) {
  p.e.bodyPointsFrame.value=String(frame);p.e.bodyPointsFrame.handlers.change();
  p.e.bodyPointsTarget.value=name;p.e.bodyPointsTarget.handlers.change();
}
function pointField(p,id,value) {p.e[id].value=value;p.e[id].handlers[id==='bodyPointsPartition'?'change':'input']();}
async function applyBodyPoints(p,view,input,digest) {
  await p.e.bodyPointsApply.onclick();const w=p.current(),message=w.messages[0],edits=JSON.parse(message.edits);
  const result={...view,input,input_sha256:digest,original_input_sha256:edits.input_sha256,
    frames:view.frames.map(frame=>{
      const edit=edits.frames.find(f=>f.frame_index===frame.frame_index);
      return {...frame,keypoints:edit.keypoints,silhouette:edit.silhouette_edit?.action==='replace'?edit.silhouette_edit.silhouette:
        edit.silhouette_edit?.action==='remove'?null:frame.silhouette};
    })};
  await w.emit({type:'result',request:message.request,report:JSON.stringify(result)});
  assert.equal(p.e.bodyPointsError.textContent,'');return {result,message,edits};
}
const observationImageResult=request=>({type:'result',request,preview:new Uint8Array([8,5,3]),
  report:JSON.stringify({schema:'ftx-body-frame-image/1',review_kind:'OBSERVATION_REVIEW',orientation:'already_applied_stored_pixels'})});
function silhouetteField(p,id,value) {
  if(id==='bodySilhouetteBackground')p.e[id].checked=value;else p.e[id].value=value;
  p.e[id].handlers[['bodySilhouetteAxis','bodySilhouettePartition','bodySilhouetteBackground'].includes(id)?'change':'input']();
}
function addSilhouette(p,overrides={}) {
  p.e.bodySilhouetteAdd.onclick();
  for(const [id,value] of Object.entries({bodySilhouetteAxis:'horizontal',bodySilhouetteAt:'100.25',bodySilhouetteSigma:'1.5',
    bodySilhouettePartition:'held_out',bodySilhouetteIntervals:'30.5, 90.75\n120, 200',bodySilhouetteEvidence:'Synthetic rectified outline; no clothing or occlusion.',...overrides})) silhouetteField(p,id,value);
}

test('body-silhouette authoring composes source-bound frame drafts with landmarks and hands the exact native input to fitting',async()=>{
  const p=await photoPage(),original=selectBodyFit(p),view=await loadBodyPoints(p);addSilhouette(p);
  assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.e.bodyPointsDownloads.children.length,0);
  chooseBodyPoint(p,'p1',1);addSilhouette(p,{bodySilhouetteAxis:'vertical',bodySilhouetteAt:'50',bodySilhouettePartition:'fit',bodySilhouetteIntervals:'10, 150'});
  chooseBodyPoint(p,'p0 <λ>');assert.equal(p.e.bodySilhouetteAt.value,'100.25');pointField(p,'bodyPointsX','12.25');
  const input=' {"native":"landmarks and silhouettes","timestamp_us":18446744073709551615}\r\n';
  const applied=await applyBodyPoints(p,view,input,'b'.repeat(64));
  assert.equal(applied.message.input,original);assert.equal(applied.edits.input_sha256,view.input_sha256);
  assert.equal(applied.edits.model_sha256,view.model_sha256);
  assert.deepEqual(applied.edits.frames[0].silhouette_edit,{action:'replace',silhouette:{evidence:'Synthetic rectified outline; no clothing or occlusion.',
    lines:[{axis:'horizontal',at_px:100.25,intervals_px:[[30.5,90.75],[120,200]],sigma_px:1.5,partition:'held_out'}]}});
  assert.equal(applied.edits.frames[1].silhouette_edit.silhouette.lines[0].axis,'vertical');
  assert.deepEqual(applied.edits.frames[0].keypoints[0].pixel_px,[12.25,20]);
  assert.equal(p.e.bodySilhouetteSigma.disabled,true);assert.equal(p.e.bodySilhouettePartition.disabled,true);
  assert.equal(p.e.bodySilhouetteAxis.disabled,true);assert.equal(p.e.bodySilhouetteAt.disabled,true);
  assert.equal(await p.urls.get(p.e.bodyPointsDownloads.children[0].href).text(),input);
  await p.e.bodyFitStartRun.onclick();assert.equal(p.current().messages[0].input,input);
  pointField(p,'bodyPointsY','20.125');const second=await applyBodyPoints(p,applied.result,'{"native":"point correction"}','d'.repeat(64));
  assert.equal(Object.hasOwn(second.edits.frames[0],'silhouette_edit'),false,'point-only edits preserve the original silhouette field');
  assert.equal(Object.hasOwn(second.edits.frames[1],'silhouette_edit'),false);
  p.e.bodySilhouetteClear.onclick();assert.equal(p.e.bodyFitRun.disabled,true);
  p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodySilhouetteAt.value,'100.25');
  p.e.bodySilhouetteClear.onclick();const removed=await applyBodyPoints(p,second.result,'{"native":"silhouette removed"}','e'.repeat(64));
  assert.deepEqual(removed.edits.frames[0].silhouette_edit,{action:'remove'});
  assert.equal(removed.result.frames[1].silhouette.lines.length,1);
  await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,'{"native":"silhouette removed"}');
});

test('body-silhouette authoring rejects missing evidence, duplicate slices, unsafe intervals and incomplete backgrounds before dispatch',async()=>{
  for(const [field,value,pattern] of [
    ['bodySilhouetteEvidence','',/evidence/],['bodySilhouetteEvidence','decoded_mask_sha256: invented',/source marker/],
    ['bodySilhouetteSigma','0',/silhouette sigma/],['bodySilhouettePartition','',/FIT or CHECK/],
    ['bodySilhouetteAt','480',/fixed coordinate/],['bodySilhouetteIntervals','30, 90\n90, 120',/disjoint/],
    ['bodySilhouetteIntervals','90, 30',/strictly before/],['bodySilhouetteIntervals','-1, 30',/endpoint/],
    ['bodySilhouetteIntervals','30, 90, 120',/pair per/],['bodySilhouetteBackground',true,/foreground on at least one/]]) {
    const p=await photoPage();selectBodyFit(p);await loadBodyPoints(p);addSilhouette(p);silhouetteField(p,field,value);
    const count=p.workers.length;await p.e.bodyPointsApply.onclick();assert.equal(p.workers.length,count);
    assert.match(p.e.bodyPointsError.textContent,pattern);assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
  }
  const p=await photoPage();selectBodyFit(p);const view=await loadBodyPoints(p);addSilhouette(p);addSilhouette(p);
  await p.e.bodyPointsApply.onclick();assert.match(p.e.bodyPointsError.textContent,/distinct/);
  silhouetteField(p,'bodySilhouetteAt','200');silhouetteField(p,'bodySilhouetteBackground',true);
  const applied=await applyBodyPoints(p,view,'{"native":"foreground and background"}','b'.repeat(64));
  assert.deepEqual(applied.edits.frames[0].silhouette_edit.silhouette.lines[1].intervals_px,[]);
  p.e.bodySilhouetteLine.value='0';p.e.bodySilhouetteLine.handlers.change();p.e.bodySilhouetteRemove.onclick();
  await p.e.bodyPointsApply.onclick();assert.match(p.e.bodyPointsError.textContent,/foreground on at least one/);
  p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodySilhouetteLine.children.length,2);
});

test('body-silhouette imported masks and distorted cameras remain read-only while keypoint-only edits preserve them',async()=>{
  for(const masked of [false,true]) {
    const p=await photoPage();selectBodyFit(p);const initial=bodyPointsReport();
    Object.assign(initial.frames[0],{silhouette_editable:false,silhouette_reason:masked?'Imported source mask remains read-only.':'Rectify the camera and all observations together.'});
    if(masked)initial.frames[0].silhouette={evidence:'decoded_mask_sha256: '+'f'.repeat(64),
      lines:[{axis:'vertical',at_px:40,intervals_px:[[20,150]],sigma_px:2,partition:'fit'}]};
    const view=await loadBodyPoints(p,{frames:initial.frames});assert.equal(p.e.bodyPointsError.textContent,'');
    assert.equal(p.e.bodySilhouetteAdd.disabled,true);assert.equal(p.e.bodySilhouetteClear.disabled,true);
    p.e.bodySilhouetteAdd.onclick();p.e.bodySilhouetteClear.onclick();assert.equal(p.e.bodyPointsApply.disabled,true);
    chooseBodyPoint(p,'p0 <λ>');pointField(p,'bodyPointsX','15');
    const applied=await applyBodyPoints(p,view,'{"native":"keypoints only"}','b'.repeat(64));
    assert.equal(Object.hasOwn(applied.edits.frames[0],'silhouette_edit'),false);
    assert.deepEqual(applied.result.frames[0].silhouette,initial.frames[0].silhouette);
  }
});

test('body-silhouette photo picking uses verified stored pixels and explicitly owns canvas clicks',async()=>{
  const p=await photoPage({decode:async()=>({width:640,height:480,close(){}})});selectBodyFit(p);const view=await loadBodyPoints(p);
  chooseBodyPoint(p,'p0 <λ>');addSilhouette(p,{bodySilhouetteIntervals:''});
  p.e.bodySilhouettePick.onclick();assert.equal(p.e.bodySilhouettePickCancel.disabled,true,'unverified pixels cannot be picked');
  p.e.bodyPointsImage.files=[file('original pixels','original.png')];await p.e.bodyPointsImageRun.onclick();let w=p.current();
  await w.emit(observationImageResult(w.messages[0].request));
  p.e.bodyPointsCanvas.getBoundingClientRect=()=>({left:10,top:20,width:320,height:240});
  const click=(x,y)=>p.e.bodyPointsCanvas.handlers.click({clientX:10+(x+.5)/2,clientY:20+(y+.5)/2});
  p.e.bodySilhouettePick.onclick();click(30.25,130);
  const count=p.workers.length;await p.e.bodyPointsApply.onclick();assert.equal(p.workers.length,count);assert.match(p.e.bodyPointsError.textContent,/Finish or cancel/);
  click(20,130);assert.match(p.e.bodyPointsError.textContent,/never reordered or clamped/);
  click(90.75,130);assert.equal(p.e.bodySilhouetteIntervals.value,'30.25, 90.75');
  assert.equal(p.e.bodyPointsX.value,'10','slice clicks do not move the selected anatomical target');
  assert.equal(p.e.bodySilhouetteAt.value,'100.25','transverse clicks never move the authored scanline');
  const applied=await applyBodyPoints(p,view,'{"native":"picked intervals"}','b'.repeat(64));
  assert.deepEqual(applied.edits.frames[0].silhouette_edit.silhouette.lines[0].intervals_px,[[30.25,90.75]]);
  p.e.bodySilhouettePick.onclick();click(100,10);p.e.bodySilhouettePickCancel.onclick();
  assert.equal(p.e.bodySilhouetteIntervals.value,'30.25, 90.75');
  p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodyFitRun.disabled,false);
});

test('body-silhouette newer drafts cancel stale worker results and reject changed or ignored silhouette replies',async()=>{
  const p=await photoPage();selectBodyFit(p);const view=await loadBodyPoints(p);addSilhouette(p);
  await p.e.bodyPointsApply.onclick();let old=p.current();const staleRequest=old.messages[0].request;
  silhouetteField(p,'bodySilhouetteIntervals','40, 100');assert.equal(old.dead,true);
  await old.emit({type:'result',request:staleRequest,report:JSON.stringify({...view,input:'{"stale":true}',input_sha256:'b'.repeat(64)})});
  assert.equal(p.e.bodySilhouetteIntervals.value,'40, 100');assert.equal(p.e.bodyPointsError.textContent,'');assert.equal(p.urls.size,0);
  await p.e.bodyPointsApply.onclick();old=p.current();
  await old.emit({type:'result',request:old.messages[0].request,report:JSON.stringify({...view,input:'{"ignored":true}',input_sha256:'b'.repeat(64)})});
  assert.match(p.e.bodyPointsError.textContent,/requested silhouette/);assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
  p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodySilhouetteLine.children.length,0);assert.equal(p.e.bodyFitRun.disabled,false);
  addSilhouette(p);p.e.bodyFitModel.handlers.change();assert.equal(p.e.bodyPointsEditor.hidden,true);assert.equal(p.urls.size,0);
});

test('body-silhouette remove and re-add retains original roles at the same location, including coordinate zero',async()=>{
  const p=await photoPage();selectBodyFit(p);const initial=bodyPointsReport();
  initial.frames[0].silhouette={evidence:'Synthetic reviewed original silhouette.',
    lines:[{axis:'horizontal',at_px:0,intervals_px:[[20,100]],sigma_px:3,partition:'held_out'}]};
  const view=await loadBodyPoints(p,{frames:initial.frames});p.e.bodySilhouetteClear.onclick();p.e.bodySilhouetteAdd.onclick();
  silhouetteField(p,'bodySilhouetteAxis','horizontal');assert.equal(p.e.bodySilhouetteAt.disabled,false,'a blank coordinate is not silently treated as zero');
  silhouetteField(p,'bodySilhouetteAt','0');assert.equal(p.e.bodySilhouetteAt.disabled,true);
  assert.equal(p.e.bodySilhouetteSigma.value,'3');assert.equal(p.e.bodySilhouettePartition.value,'held_out');
  silhouetteField(p,'bodySilhouetteSigma','9');silhouetteField(p,'bodySilhouettePartition','fit');
  silhouetteField(p,'bodySilhouetteIntervals','25, 105');silhouetteField(p,'bodySilhouetteEvidence','Explicitly reviewed interval correction.');
  const applied=await applyBodyPoints(p,view,'{"native":"same sampled location"}','b'.repeat(64));
  const line=applied.edits.frames[0].silhouette_edit.silhouette.lines[0];
  assert.equal(line.at_px,0);assert.equal(line.partition,'held_out');assert.equal(line.sigma_px,3);
});

test('body-silhouette changing frames cancels partial image picks without writing to either frame',async()=>{
  const p=await photoPage({decode:async()=>({width:640,height:480,close(){}})});selectBodyFit(p);await loadBodyPoints(p);
  addSilhouette(p,{bodySilhouetteIntervals:''});p.e.bodyPointsImage.files=[file('original pixels','original.png')];await p.e.bodyPointsImageRun.onclick();
  const worker=p.current();await worker.emit(observationImageResult(worker.messages[0].request));
  p.e.bodyPointsCanvas.getBoundingClientRect=()=>({left:0,top:0,width:640,height:480});
  p.e.bodySilhouettePick.onclick();p.e.bodyPointsCanvas.handlers.click({clientX:30.5,clientY:100.75});
  assert.equal(p.e.bodySilhouettePickCancel.disabled,false);chooseBodyPoint(p,'p1',1);
  assert.equal(p.e.bodySilhouettePickCancel.disabled,true);assert.equal(p.e.bodyPointsCanvas.hidden,true);
  p.e.bodyPointsCanvas.handlers.click({clientX:90.5,clientY:100.75});assert.equal(p.e.bodyPointsX.value,'');
  chooseBodyPoint(p,'p0 <λ>');assert.equal(p.e.bodySilhouetteIntervals.value,'');assert.equal(p.e.bodyPointsX.value,'10');
});

test('body-observation worker delegates exact bound drafts and transport refusals to the native authority',async()=>{
  const source={request:93,op:'body-fit-observations',model:new Uint8Array([1,5]).buffer,input:' {"timestamp_us":18446744073709551615}\r\n',edits:' {"frames":[]}\n'};
  const worker=await photoWorker(),result=await worker.send(source);
  assert.equal(result.type,'result');assert.deepEqual(worker.calls,[{kind:'body-observations',model:[1,5],input:source.input,edits:source.edits}]);
  assert.equal(worker.instances.length,0);assert.equal(JSON.parse(result.report).input,source.input);
  const refused=await photoWorker({bodyObservations:()=>{throw Error('fixed CHECK role refused');}});
  assert.match((await refused.send(source)).message,/fixed CHECK role/);
  for(const bad of [{edits:' '.repeat(256*1024+1)},{edits:null},{input:null},{model:new ArrayBuffer(0)}]) {
    const w=await photoWorker();assert.equal((await w.send({...source,...bad})).type,'error');assert.equal(w.calls.length,0);
  }
});

test('body-observation drafts compose both edit directions and send the current exact snapshot to preview fitting and downloads',async()=>{
  const p=await photoPage(),original=selectBodyFit(p);await p.e.bodyFitRun.onclick();let live=p.current();
  await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(bodyFitReport())});
  const view=await loadBodyPoints(p);assert.equal(p.e.bodyFitResult.hidden,false,'opening observations does not change the fit');
  p.e.bodyFitWorkbench.onclick();const popup=p.popups[0];chooseBodyPoint(p,'p0 <λ>');
  assert.equal(p.e.bodyPointsSigma.disabled,true);assert.equal(p.e.bodyPointsPartition.disabled,true);
  pointField(p,'bodyPointsX','100.25');assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitRun.disabled,true);
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token:new URL(popup.url).searchParams.get('bodyFitImport')}});
  assert.equal(popup.messages[0].data.type,'ftx-body-fit-sources-cancelled');
  chooseBodyPoint(p,'p1',1);assert.equal(p.e.bodyPointsSigma.disabled,false);
  pointField(p,'bodyPointsX','30.5');pointField(p,'bodyPointsY','40.75');pointField(p,'bodyPointsSigma','2');pointField(p,'bodyPointsPartition','fit');
  chooseBodyPoint(p,'p0 <λ>');assert.equal(p.e.bodyPointsX.value,'100.25');
  const first=' {"native":"points","timestamp_us":18446744073709551615}\r\n';
  const points=await applyBodyPoints(p,view,first,'b'.repeat(64));
  assert.equal(points.message.input,original);assert.equal(points.edits.input_sha256,'a'.repeat(64));
  assert.deepEqual(points.edits.frames[0].keypoints[0],{name:'p0 <λ>',pixel_px:[100.25,20],sigma_px:1,partition:'held_out'});
  assert.equal(await p.urls.get(p.e.bodyPointsDownloads.children[0].href).text(),first);
  const starts=await loadBodyStarts(p,{input_sha256:'b'.repeat(64),original_input_sha256:'b'.repeat(64)});
  assert.equal(p.current().messages[0].input,first);assert.equal(p.e.bodyPointsEditor.hidden,true);
  editBodyStart(p,'bodyStart-shape-0','0.48');await p.e.bodyStartsApply.onclick();live=p.current();
  const startEdits=JSON.parse(live.messages[0].edits),second=' {"native":"points and starts","timestamp_us":18446744073709551615}\r\n';
  assert.equal(startEdits.input_sha256,'b'.repeat(64));assert.equal(live.messages[0].input,first);
  await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify({...starts,input:second,input_sha256:'d'.repeat(64),original_input_sha256:'b'.repeat(64),starts:startEdits.starts})});
  const resumed=await loadBodyPoints(p,{...points.result,input:second,input_sha256:'d'.repeat(64),original_input_sha256:'d'.repeat(64)});
  assert.equal(p.current().messages[0].input,second);assert.equal(p.e.bodyStartsEditor.hidden,true);
  chooseBodyPoint(p,'p1',1);assert.equal(p.e.bodyPointsY.value,'40.75');assert.equal(p.e.bodyPointsPartition.disabled,true);
  pointField(p,'bodyPointsY','35.75');const final=' {"native":"both edits retained","timestamp_us":18446744073709551615}\r\n';
  const applied=await applyBodyPoints(p,resumed,final,'e'.repeat(64));
  assert.equal(applied.message.input,second);assert.equal(applied.edits.input_sha256,'d'.repeat(64));
  await p.e.bodyFitStartRun.onclick();assert.equal(p.current().messages[0].input,final);
  await p.e.bodyFitRun.onclick();live=p.current();assert.equal(live.messages[0].input,final);
  await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(bodyFitReport())});
  assert.equal(await p.urls.get(p.e.bodyFitDownloads.children.find(a=>a.download==='body-fit-input.json').href).text(),final);
  assert.equal(await new Blob([await p.e.bodyFitInput.files[0].arrayBuffer()]).text(),original);
  p.e.bodyPointsReset.onclick();assert.equal(p.e.bodyPointsEditor.hidden,true);assert.equal(p.urls.size,0);
  await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,original);
});

test('body-observation clicks require verified pixels and preserve CSS-scaled stored coordinates with recorded rotation',async()=>{
  const options=[],closed=[];const p=await photoPage({decode:async(_blob,opts)=>{options.push(opts);return {width:640,height:480,close(){closed.push(true);}};}});
  selectBodyFit(p);const view=await loadBodyPoints(p);chooseBodyPoint(p,'p0 <λ>');
  p.e.bodyPointsCanvas.getBoundingClientRect=()=>({left:10,top:20,width:320,height:240});
  const click={clientX:10+(100.25+.5)/2,clientY:20+(50.5+.5)/2};
  p.e.bodyPointsCanvas.handlers.click(click);assert.equal(p.e.bodyPointsX.value,'10');
  p.e.bodyPointsImage.files=[file('original pixels','original.png')];await p.e.bodyPointsImageRun.onclick();let w=p.current();
  assert.equal(w.messages[0].review,view.frames[0].image_review);assert.match(w.messages[0].review,/18446744073709551615/);
  await w.emit(observationImageResult(w.messages[0].request));
  assert.equal(options[0].imageOrientation,'none');assert.match(p.e.bodyPointsImageStatus.textContent,/90° rotation already applied/);
  assert.equal(p.e.bodyPointsCanvas.hidden,false);p.e.bodyPointsCanvas.handlers.click(click);
  assert.equal(p.e.bodyPointsX.value,'100.25');assert.equal(p.e.bodyPointsY.value,'50.5');
  p.e.bodyPointsCanvas.handlers.click({clientX:10,clientY:20});assert.match(p.e.bodyPointsError.textContent,/never clamped/);
  const applied=await applyBodyPoints(p,view,'{"edited":"native coordinates"}','b'.repeat(64));
  assert.deepEqual(applied.edits.frames[0].keypoints[0].pixel_px,[100.25,50.5]);
  assert.equal(p.e.bodyPointsCanvas.hidden,false,'verified original pixels remain valid after point-only edits');
  chooseBodyPoint(p,'p1',1);assert.equal(p.e.bodyPointsCanvas.hidden,true);assert.equal(closed.length,1);
  assert.equal(p.e.bodyPointsImageRun.disabled,true);assert.match(p.e.bodyPointsImageSource.textContent,/no original-image binding/);
});

test('body-observation pending values require explicit uncertainty and roles and refuse invalid native replies',async()=>{
  const p=await photoPage();selectBodyFit(p);const view=await loadBodyPoints(p);chooseBodyPoint(p,'p1',1);
  pointField(p,'bodyPointsX','32');pointField(p,'bodyPointsY','45');
  for(const [id,value,pattern] of [['bodyPointsPartition','',/Choose FIT or CHECK/],['bodyPointsPartition','fit',/finite sigma/],
    ['bodyPointsSigma','0',/finite sigma/],['bodyPointsSigma','2',null],['bodyPointsX','640',/finite x pixel/]]) {
    pointField(p,id,value);if(!pattern)continue;
    const count=p.workers.length;await p.e.bodyPointsApply.onclick();assert.equal(p.workers.length,count);
    assert.match(p.e.bodyPointsError.textContent,pattern);assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
  }
  pointField(p,'bodyPointsX','32');await p.e.bodyPointsApply.onclick();let w=p.current();
  await w.emit({type:'error',request:w.messages[0].request,message:'native camera domain refusal'});
  assert.match(p.e.bodyPointsError.textContent,/camera domain/);assert.equal(p.e.bodyPointsX.value,'32');
  for(const change of [v=>{v.original_input_sha256='f'.repeat(64);},v=>{v.frames[0].rotation_cw_deg=0;}]) {
    await p.e.bodyPointsApply.onclick();w=p.current();const candidate=structuredClone(view);candidate.input='{"edited":"native"}';
    candidate.input_sha256='b'.repeat(64);change(candidate);
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(candidate)});
    assert.ok(p.e.bodyPointsError.textContent);assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
  }
  p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodyPointsX.value,'');assert.equal(p.e.bodyFitRun.disabled,false);
  chooseBodyPoint(p,'p0 <λ>');p.e.bodyPointsRemove.onclick();assert.equal(p.e.bodyPointsRows.children.length,0);
  const applied=await applyBodyPoints(p,view,'{"removed":"native"}','b'.repeat(64));
  assert.deepEqual(applied.edits.frames[0].keypoints,[]);assert.deepEqual(applied.edits.frames[1].keypoints,[]);
});

test('body-observation source chooser cancellation retains applied and pending work while actual source changes withdraw it',async()=>{
  for(const source of ['bodyFitInput','bodyFitModel']) {
    const p=await photoPage();selectBodyFit(p);const view=await loadBodyPoints(p);chooseBodyPoint(p,'p0 <λ>');
    pointField(p,'bodyPointsX','33.5');const native=' {"applied":"observations","timestamp_us":18446744073709551615}\r\n';
    await applyBodyPoints(p,view,native,'b'.repeat(64));assert.equal(p.e.bodyPointsLoad.disabled,true);
    const count=p.workers.length;await p.e.bodyPointsLoad.onclick();assert.equal(p.workers.length,count);
    pointField(p,'bodyPointsX','34.5');
    for(const event of ['click','cancel']) {
      p.e[source].handlers[event]();assert.equal(p.e.bodyPointsEditor.hidden,false);assert.equal(p.e.bodyPointsX.value,'34.5');
      assert.equal(p.e.bodyFitRun.disabled,true);assert.equal(p.urls.size,0);
    }
    p.e.bodyPointsCancel.onclick();assert.equal(p.e.bodyPointsX.value,'33.5');
    assert.equal(await p.urls.get(p.e.bodyPointsDownloads.children[0].href).text(),native);
    await p.e.bodyFitStartRun.onclick();assert.equal(p.current().messages[0].input,native);
    p.e[source].handlers.change();assert.equal(p.e.bodyPointsEditor.hidden,true);assert.equal(p.urls.size,0);
  }
});

test('body-observation edits cancel in-flight applications and delayed reads without overwriting newer drafts',async()=>{
  for(const kind of ['result','error','crash']) {
    const p=await photoPage();selectBodyFit(p);const view=await loadBodyPoints(p);chooseBodyPoint(p,'p0 <λ>');
    pointField(p,'bodyPointsX','30');await p.e.bodyPointsApply.onclick();const old=p.current(),request=old.messages[0].request;
    pointField(p,'bodyPointsX','31');const status=p.e.bodyPointsStatus.textContent;
    if(kind==='crash')old.onerror({message:'old worker crashed'});
    else await old.emit({type:kind,request,message:'old refusal',report:JSON.stringify({...view,input:'{"old":"reply"}',input_sha256:'b'.repeat(64)})});
    assert.equal(old.dead,true);assert.equal(p.e.bodyPointsX.value,'31');assert.equal(p.e.bodyPointsStatus.textContent,status);
    assert.equal(p.e.bodyPointsError.textContent,'');assert.equal(p.urls.size,0);assert.equal(p.e.bodyFitRun.disabled,true);
    await p.e.bodyPointsApply.onclick();assert.equal(JSON.parse(p.current().messages[0].edits).frames[0].keypoints[0].pixel_px[0],31);
  }
  for(const field of ['bodyFitInput','bodyFitModel']) {
    const p=await photoPage();selectBodyFit(p);let resolve,started;const reading=new Promise(done=>{started=done;});
    p.e[field].files=[{size:2,arrayBuffer:()=>{started();return new Promise(done=>{resolve=done;});}}];
    const pending=p.e.bodyPointsLoad.onclick();await reading;p.e.bodyPointsCancel.onclick();
    selectBodyFit(p);await loadBodyPoints(p);const status=p.e.bodyPointsStatus.textContent;
    resolve(new TextEncoder().encode('{}').buffer);await pending;
    assert.equal(p.workers.length,1);assert.equal(p.e.bodyPointsStatus.textContent,status);assert.equal(p.e.bodyPointsError.textContent,'');
  }
});

test('body-observation photograph cancellation closes late decoded pixels and native refusals leave manual observations usable',async()=>{
  let finish;const closed=[];const p=await photoPage({decode:()=>new Promise(done=>{finish=done;})});
  selectBodyFit(p);await loadBodyPoints(p);chooseBodyPoint(p,'p0 <λ>');p.e.bodyPointsImage.files=[file('original pixels')];
  await p.e.bodyPointsImageRun.onclick();let w=p.current();const decoding=w.emit(observationImageResult(w.messages[0].request));
  chooseBodyPoint(p,'p1',1);finish({width:640,height:480,close(){closed.push(true);}});await decoding;
  assert.equal(closed.length,1);assert.equal(p.e.bodyPointsCanvas.hidden,true);
  chooseBodyPoint(p,'p0 <λ>');p.e.bodyPointsImage.files=[file('wrong pixels')];await p.e.bodyPointsImageRun.onclick();w=p.current();
  await w.emit({type:'error',request:w.messages[0].request,message:'original encoded SHA-256 mismatch'});
  assert.match(p.e.bodyPointsImageStatus.textContent,/SHA-256 mismatch/);assert.equal(p.e.bodyPointsCanvas.hidden,true);
  pointField(p,'bodyPointsX','29');assert.equal(p.e.bodyPointsX.value,'29');assert.equal(p.e.bodyPointsApply.disabled,false);
  p.windowEvents.pagehide();assert.equal(p.e.bodyPointsEditor.hidden,true);assert.equal(p.urls.size,0);
});

test('saved body-fit worker validates the exact report and first pose without dispatching fitting',async()=>{
  const source={request:71,op:'open-body-fit',model:new Uint8Array([1,3,7]).buffer,
    report:' \r\n{"status":"ACCEPTED","timestamp_us":18446744073709551615}\r\n',iterations:'inactive',frameIndex:7};
  const w=await photoWorker();assert.equal((await w.send(source)).type,'result');
  assert.deepEqual(w.calls,[{kind:'body-frame-review',model:[1,3,7],report:source.report,frameIndex:'0'}]);
  assert.equal(w.instances.length,0);
  const refused=await photoWorker({failBodyReview:true});assert.equal((await refused.send(source)).type,'error');
  assert.equal(refused.calls.length,1);assert.equal(refused.calls[0].kind,'body-frame-review');
  for(const bad of [{model:new ArrayBuffer(0)},{model:[1,3,7]},{report:null},{report:' '.repeat(16*1024*1024+1)}]) {
    const invalid=await photoWorker();assert.equal((await invalid.send({...source,...bad})).type,'error');
    assert.equal(invalid.calls.length,0);
  }
});

test('saved body-fit loading preserves original evidence and model snapshots for review and garment reuse',async()=>{
  const p=await photoPage();assert.equal(p.e.bodyFitOpen.disabled,true);
  const raw=' \r\n'+JSON.stringify(bodyFitReport()).replaceAll('"timestamp_us":1','"timestamp_us":18446744073709551615')
    .replace('"shape":[0.6]','"shape":[-0.0]')+'\r\n';
  selectSavedBodyFit(p,raw);let reads=0;
  class ReadOnceModel extends Blob {
    async arrayBuffer(){assert.equal(++reads,1,'Saved-model source is snapshotted once');return super.arrayBuffer();}
  }
  p.e.bodyFitModel.files=[new ReadOnceModel(['original model'])];
  p.e.bodyFitIterations.value='not used';p.e.bodyFitEvaluations.value='not used';
  assert.equal(p.e.bodyFitOpen.disabled,false);assert.equal(p.e.bodyFitRun.disabled,true);
  await p.e.bodyFitOpen.onclick({type:'click'});const w=p.current(),m=w.messages[0];
  assert.equal(m.op,'open-body-fit');assert.equal(m.report,raw);assert.equal(m.input,undefined);
  assert.equal(m.iterations,undefined);assert.equal(m.maxEvaluations,undefined);
  assert.equal(new TextDecoder().decode(m.model),'original model');new Uint8Array(m.model).fill(0);
  assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitReviewRun.disabled,true);
  const review=JSON.stringify(bodyFrameReport()).replace('"timestamp_us":1','"timestamp_us":18446744073709551615');
  await w.emit({type:'result',request:m.request,report:review});
  assert.equal(w.dead,true);assert.equal(p.e.bodyFitReport.textContent,raw);
  assert.match(p.e.bodyFitSummary.textContent,/^Saved report: ACCEPTED/);
  assert.match(p.e.bodyFitStatus.textContent,/Saved accepted fit loaded; no fitting was run/);
  assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.match(p.e.bodyFitReviewStatus.textContent,/18446744073709551615 us/);
  assert.deepEqual(p.e.bodyFitDownloads.children.map(a=>a.download),['body-fit-report.json']);
  assert.equal(await p.urls.get(p.e.bodyFitDownloads.children[0].href).text(),raw);
  assert.equal(await p.urls.get(p.e.bodyFitReviewDownloads.children[0].href).text(),review);
  assert.equal(p.urls.size,3,'Report plus frame JSON/OBJ; no reconstructed input or work-options files');
  assert.equal(p.e.openWorkbench.disabled,true,'Loaded MODEL_FIT never becomes measurement evidence');
  p.e.bodyFitWorkbench.onclick();const popup=p.popups[0],token=new URL(popup.url).searchParams.get('bodyFitImport');
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token}});
  assert.equal(await popup.messages[0].data.fit.text(),raw);assert.equal(await popup.messages[0].data.model.text(),'original model');
  assert.equal(reads,1);
  selectBodyFit(p,' {"fresh":"original observations"}\n');p.e.bodyFitIterations.value='50';p.e.bodyFitEvaluations.value='100000';
  await p.e.bodyFitRun.onclick({type:'click'});const fresh=p.current();
  assert.equal(fresh.messages[0].op,'fit-body','Click events cannot select saved-fit mode');
  await fresh.emit({type:'result',request:fresh.messages[0].request,report:JSON.stringify(bodyFitReport())});
  assert.match(p.e.bodyFitSummary.textContent,/^ACCEPTED:/);
  assert.deepEqual(p.e.bodyFitDownloads.children.map(a=>a.download),['body-fit-report.json','body-fit-input.json','body-fit-options.json']);
});

test('saved body-fit loading withdraws old bodies and rejects cancelled reads, source changes and stale workers',async()=>{
  for(const invalidate of [p=>p.e.bodyFitCancel.onclick(),p=>p.e.bodyFitSaved.handlers.change(),
    p=>p.e.bodyFitSaved.handlers.click(),p=>p.e.bodyFitSaved.handlers.cancel(),p=>p.e.bodyFitModel.handlers.change(),
    p=>p.e.bodyFitInput.handlers.click(),p=>p.e.bodyFitIterations.handlers.input(),p=>p.windowEvents.pagehide()]) {
    const p=await photoPage();selectSavedBodyFit(p);await p.e.bodyFitOpen.onclick();let w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFrameReport())});
    assert.equal(p.urls.size,3);await p.e.bodyFitOpen.onclick();w=p.current();
    assert.equal(p.urls.size,0);assert.equal(p.e.bodyFitWorkbench.disabled,true);
    invalidate(p);await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFrameReport())});
    assert.equal(w.dead,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
    assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.urls.size,0);
  }
  for(const which of ['bodyFitSaved','bodyFitModel'])for(const reject of [false,true]) {
    const p=await photoPage();selectSavedBodyFit(p);let resolve,fail,entered;
    const reading=new Promise(yes=>{entered=yes;});
    p.e[which].files=[{size:2,arrayBuffer:()=>{entered();return new Promise((yes,no)=>{resolve=yes;fail=no;});}}];
    const pending=p.e.bodyFitOpen.onclick();await reading;p.e.bodyFitCancel.onclick();
    selectSavedBodyFit(p);await p.e.bodyFitOpen.onclick();const current=p.current();
    const message=p.e.bodyFitStatus.textContent;
    if(reject)fail(Error('late read failed'));else resolve(new TextEncoder().encode('{}').buffer);
    await pending;assert.equal(p.workers.length,1);assert.equal(current.dead,undefined);
    assert.equal(p.e.bodyFitStatus.textContent,message);
    await current.emit({type:'result',request:current.messages[0].request,report:JSON.stringify(bodyFrameReport())});
    assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,3);
  }
});

test('saved body-fit loading refuses invalid sources and incomplete validation without publishing reusable geometry',async()=>{
  for(const change of [p=>p.e.bodyFitSaved.files=[],p=>p.e.bodyFitModel.files=[],
    p=>p.e.bodyFitSaved.files=[{size:16*1024*1024+1,arrayBuffer:()=>{throw Error('oversized source read');}}],
    p=>p.e.bodyFitModel.files=[{size:512*1024*1024+1,arrayBuffer:()=>{throw Error('oversized model read');}}],
    p=>p.e.bodyFitSaved.files=[{size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer}]]) {
    const p=await photoPage();selectSavedBodyFit(p);change(p);await p.e.bodyFitOpen.onclick();
    assert.equal(p.workers.length,0);assert.ok(p.e.bodyFitError.textContent);assert.equal(p.urls.size,0);
    assert.equal(p.e.bodyFitWorkbench.disabled,true);
  }
  for(const outcome of ['refused','crash','malformed-review','wrong-frame','nonaccepted-report','invalid-report']) {
    const p=await photoPage();selectSavedBodyFit(p,outcome==='nonaccepted-report'?JSON.stringify(bodyFitReport({status:'REJECTED',solution:null})):
      outcome==='invalid-report'?'{':JSON.stringify(bodyFitReport()));
    await p.e.bodyFitOpen.onclick();const w=p.current(),request=w.messages[0].request;
    if(outcome==='refused')await w.emit({type:'error',request,message:'model SHA-256 mismatch'});
    else if(outcome==='crash')w.onerror({message:'load interrupted'});
    else await w.emit({type:'result',request,report:outcome==='malformed-review'?'{}':JSON.stringify(bodyFrameReport(outcome==='wrong-frame'?{frame_index:1}:{}))});
    assert.equal(w.dead,true);assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitReviewRun.disabled,true);
    assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
    assert.ok(p.e.bodyFitError.textContent);assert.equal(p.urls.size,0);
  }
});

test('body-fit worker preserves the model and exact observations and refuses invalid work limits',async()=>{
  const source={request:51,op:'fit-body',model:new Uint8Array([1,3,7]).buffer,
    input:' {"timestamp_us":18446744073709551615,"evidence":"λ"}\r\n',iterations:50,maxEvaluations:100000};
  const w=await photoWorker();assert.equal((await w.send(source)).type,'result');
  assert.deepEqual(w.calls,[{kind:'fit-body',model:[1,3,7],input:source.input,iterations:'50',maxEvaluations:'100000'}]);
  assert.equal(w.instances.length,0);
  for(const change of [{iterations:0},{iterations:1.5},{iterations:4294967346},{maxEvaluations:Infinity},
    {maxEvaluations:1000001},{model:new ArrayBuffer(0)},{model:[1,3,7]},{input:' '.repeat(16*1024*1024+1)}]) {
    const refused=await photoWorker();assert.equal((await refused.send({...source,...change})).type,'error');
    assert.equal(refused.calls.length,0);
  }
  const failed=await photoWorker({failBodyFit:true});assert.match((await failed.send(source)).message,/SHA-256 mismatch/);
  assert.equal(failed.calls.length,1,'No substitute fit is attempted');
});
test('body-fit page shows every native outcome and retains original reports without promoting predictions',async()=>{
  const p=await photoPage(),input=selectBodyFit(p);
  for(const status of ['ACCEPTED','REJECTED','AMBIGUOUS','UNOBSERVABLE','NOT_CONVERGED','BUDGET_STOP']) {
    await p.e.bodyFitRun.onclick();const w=p.current(),m=w.messages[0];
    assert.equal(m.op,'fit-body');assert.equal(m.input,input);assert.equal(new TextDecoder().decode(m.model),'original model');
    assert.equal(m.iterations,50);assert.equal(m.maxEvaluations,100000);
    const value=bodyFitReport({status,solution:status==='ACCEPTED'?bodyFitReport().solution:null});
    if(status!=='ACCEPTED') {value.attempts[0].status=status;value.attempts[0].flags=['held-out observation failed'];
      value.attempts[0].checks[0].residual=null;value.attempts[0].checks[0].passed=false;}
    const raw=JSON.stringify(value).replace('"timestamp_us":1','"timestamp_us":18446744073709551615');
    await w.emit({type:'result',request:m.request,report:raw});
    assert.equal(w.dead,true);assert.equal(p.e.bodyFitResult.hidden,false);assert.match(p.e.bodyFitSummary.textContent,new RegExp(status));
    assert.equal(p.e.bodyFitAttempts.children.length,1);assert.equal(p.e.bodyFitChecks.children.length,2);
    const links=p.e.bodyFitDownloads.children;assert.deepEqual(links.map(a=>a.download),['body-fit-report.json','body-fit-input.json','body-fit-options.json']);
    assert.equal(await p.urls.get(links[0].href).text(),raw);assert.equal(await p.urls.get(links[1].href).text(),input);
    assert.deepEqual(JSON.parse(await p.urls.get(links[2].href).text()),{iterations:50,max_model_evaluations:100000});
    if(status==='ACCEPTED') {assert.match(p.e.bodyFitPredictions.textContent,/MODEL_FIT.*\n.*1300/);assert.match(p.e.bodyFitUse.textContent,/confirm the wearer/);}
    else {assert.equal(p.e.bodyFitPredictions.textContent,'');assert.match(p.e.bodyFitUse.textContent,/No accepted shape/);
      assert.match(p.e.bodyFitChecks.children[0].children[4].textContent,/Unavailable/);assert.match(p.e.bodyFitFlags.textContent,/held-out observation failed/);}
    assert.equal(p.e.openWorkbench.disabled,true);assert.equal(p.e.returnMaterials.disabled,true);
    assert.equal(p.e.downloads.children.length,0,'Predictions do not enter measurement evidence');
  }
});

test('body-frame review worker preserves exact accepted sources and dispatches no fitting call',async()=>{
  const source={request:61,op:'review-body-fit-frame',model:new Uint8Array([1,3,7]).buffer,
    report:' \r\n{"timestamp_us":18446744073709551615,"status":"ACCEPTED"}\r\n',frameIndex:1};
  const w=await photoWorker();assert.equal((await w.send(source)).type,'result');
  assert.deepEqual(w.calls,[{kind:'body-frame-review',model:[1,3,7],report:source.report,frameIndex:'1'}]);
  assert.equal(w.instances.length,0);
  for(const frameIndex of [-1,8,1.5,4294967296,NaN,'0']) {
    const rejected=await photoWorker();assert.equal((await rejected.send({...source,frameIndex})).type,'error');
    assert.equal(rejected.calls.length,0);
  }
  for(const bad of [{model:new ArrayBuffer(0)},{report:' '.repeat(16*1024*1024+1)},{report:null}]) {
    const rejected=await photoWorker();assert.equal((await rejected.send({...source,...bad})).type,'error');
    assert.equal(rejected.calls.length,0);
  }
  const refused=await photoWorker({failBodyReview:true});assert.equal((await refused.send(source)).type,'error');
  assert.equal(refused.calls.length,1);assert.match(refused.sent.at(-1).message,/3-pixel/);
});

test('body-frame review draws native projections and downloads exact frame identities and full geometry',async()=>{
  const p=await photoPage();selectBodyFit(p);p.e.bodyFitReviewMesh.checked=true;
  assert.equal(p.e.bodyFitReviewRun.disabled,true);await p.e.bodyFitReviewRun.onclick();assert.equal(p.workers.length,0);
  await p.e.bodyFitRun.onclick();const fit=p.current();
  const raw=' \r\n'+JSON.stringify(bodyFitReport()).replace('"timestamp_us":1','"timestamp_us":18446744073709551615')+'\r\n';
  await fit.emit({type:'result',request:fit.messages[0].request,report:raw});
  assert.equal(p.e.bodyFitReviewRun.disabled,false);assert.equal(p.e.bodyFitReview.hidden,false);
  assert.equal(p.e.bodyFitReviewFrame.children[0].textContent,'Frame 1: synthetic');
  await p.e.bodyFitReviewRun.onclick();const review=p.current(),request=review.messages[0];
  assert.equal(request.op,'review-body-fit-frame');assert.equal(request.report,raw);assert.equal(request.frameIndex,0);
  assert.equal(new TextDecoder().decode(request.model),'original model');
  const reviewText=JSON.stringify(bodyFrameReport()).replace('"timestamp_us":1','"timestamp_us":18446744073709551615');
  await review.emit({type:'result',request:request.request,report:reviewText});
  assert.equal(review.dead,true);assert.equal(p.e.bodyFitReviewCanvas.hidden,false);
  assert.equal(p.e.bodyFitReviewCanvas.width,640);assert.equal(p.e.bodyFitReviewCanvas.height,480);
  assert.ok(p.e.bodyFitReviewCanvas.labels.some(([label])=>label.includes('shoulder (CHECK')));
  assert.ok(p.e.bodyFitReviewCanvas.labels.some(([label])=>label.includes('waist (FIT')));
  assert.match(p.e.bodyFitReviewStatus.textContent,/18446744073709551615 us/);
  assert.match(p.e.bodyFitReviewSummary.textContent,/1 vertices cannot be projected/);
  const [json,obj]=p.e.bodyFitReviewDownloads.children;
  assert.equal(await p.urls.get(json.href).text(),reviewText);assert.equal(await p.urls.get(obj.href).text(),bodyFrameReport().mesh_obj);
  assert.equal(p.e.bodyFitReport.textContent,raw);assert.equal(p.e.bodyFitWorkbench.disabled,false);
  assert.equal(p.e.openWorkbench.disabled,true,'Frame review does not create measurement evidence');
  p.e.bodyFitReviewMesh.checked=false;p.e.bodyFitReviewMesh.handlers.change();
  assert.equal(p.workers.length,2,'Display toggle never refits or reevaluates');assert.equal(p.urls.size,5);
  p.e.bodyFitReviewFrame.value='1';p.e.bodyFitReviewFrame.handlers.change();
  assert.equal(p.e.bodyFitReviewCanvas.hidden,true);assert.equal(p.urls.size,3);
  assert.equal(p.e.bodyFitReport.textContent,raw);assert.equal(p.e.bodyFitWorkbench.disabled,false);
});

test('body-frame review cancellation, source edits and frame changes discard stale workers and downloads',async()=>{
  for(const [invalidate,keepFit] of [
    [p=>p.e.bodyFitReviewCancel.onclick(),true],
    [p=>{p.e.bodyFitReviewFrame.value='1';p.e.bodyFitReviewFrame.handlers.change();},true],
    [p=>p.e.bodyFitInput.handlers.change(),false],[p=>p.e.bodyFitModel.handlers.click(),false],
    [p=>p.e.bodyFitIterations.handlers.input(),false],[p=>p.windowEvents.pagehide(),false]]) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const fit=p.current();
    await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport())});
    await p.e.bodyFitReviewRun.onclick();let review=p.current();
    await review.emit({type:'result',request:review.messages[0].request,report:JSON.stringify(bodyFrameReport())});
    assert.equal(p.urls.size,5);await p.e.bodyFitReviewRun.onclick();review=p.current();
    assert.equal(p.urls.size,3,'Starting a new review revokes its previous geometry');
    invalidate(p);
    await review.emit({type:'result',request:review.messages[0].request,report:JSON.stringify(bodyFrameReport())});
    assert.equal(review.dead,true);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
    assert.equal(p.e.bodyFitReviewDownloads.children.length,0);assert.equal(p.urls.size,keepFit?3:0);
    assert.equal(p.e.bodyFitWorkbench.disabled,!keepFit);
  }
});

test('body-frame review cancels delayed snapshot reads and retains accepted reports after review refusal',async()=>{
  for(const part of ['fit','model']) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const fit=p.current();
    await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport())});
    p.e.bodyFitWorkbench.onclick();const popup=p.popups[0],token=new URL(popup.url).searchParams.get('bodyFitImport');
    p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token}});
    const snapshot=popup.messages[0].data[part],method=part==='fit'?'text':'arrayBuffer';
    const original=snapshot[method].bind(snapshot);let release,entered;
    const reading=new Promise(resolve=>{entered=resolve;});
    snapshot[method]=()=>new Promise(resolve=>{release=async()=>resolve(await original());entered();});
    const pending=p.e.bodyFitReviewRun.onclick();await reading;
    p.e.bodyFitReviewCancel.onclick();await release();await pending;
    assert.equal(p.workers.length,1);assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,3);
  }
  for(const outcome of ['refusal','wrong-frame','crash','distorted']) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const fit=p.current();
    await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport())});
    await p.e.bodyFitReviewRun.onclick();const review=p.current(),request=review.messages[0].request;
    if(outcome==='refusal')await review.emit({type:'error',request,message:'full-mesh projection refused'});
    else if(outcome==='crash')review.onerror({message:'review interrupted'});
    else await review.emit({type:'result',request,report:JSON.stringify(bodyFrameReport(outcome==='distorted'?
      {projection_kind:'distorted_vertices'}:{frame_index:1}))});
    assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.e.bodyFitResult.hidden,false);
    assert.equal(p.e.bodyFitReviewCanvas.hidden,outcome!=='distorted');
    if(outcome==='distorted')assert.match(p.e.bodyFitReviewSummary.textContent,/Distorted camera: only projected vertices/);
    else {assert.ok(p.e.bodyFitReviewError.textContent);assert.equal(p.urls.size,3);}
  }
  for(const status of ['REJECTED','AMBIGUOUS','UNOBSERVABLE','NOT_CONVERGED','BUDGET_STOP']) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const fit=p.current();
    await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport({status,solution:null}))});
    assert.equal(p.e.bodyFitReviewRun.disabled,true);await p.e.bodyFitReviewRun.onclick();assert.equal(p.workers.length,1);
  }
});

test('body-start preview worker passes exact sources and native ordinals without fitting or acceptance',async()=>{
  const source={request:84,op:'review-body-fit-start',model:new Uint8Array([1,3,7]).buffer,
    input:' \r\n{"timestamp_us":18446744073709551615,"starts":"original λ"}\r\n',startIndex:1,frameIndex:3};
  const w=await photoWorker(),result=await w.send(source);
  assert.equal(result.type,'result');assert.equal(JSON.parse(result.report).status,'INITIALIZATION_REVIEW');
  assert.deepEqual(w.calls,[{kind:'body-start-review',model:[1,3,7],input:source.input,startIndex:'1',frameIndex:'3'}]);
  for(const change of [{startIndex:-1},{startIndex:4},{startIndex:1.5},{startIndex:'0'},
    {frameIndex:-1},{frameIndex:8},{frameIndex:1.5},{frameIndex:'0'},{frameIndex:4294967296},
    {model:new ArrayBuffer(0)},{model:[1,3,7]},{input:null},{input:' '.repeat(16*1024*1024+1)}]) {
    const invalid=await photoWorker();assert.equal((await invalid.send({...source,...change})).type,'error');
    assert.equal(invalid.calls.length,0);
  }
  const refused=await photoWorker({failBodyStart:true});
  assert.match((await refused.send(source)).message,/no declared initialization/);
  assert.equal(refused.calls.length,1);assert.equal(refused.calls[0].kind,'body-start-review');
});

test('body-start preview displays a single declared start and exact sources without enabling garment reuse',async()=>{
  const p=await photoPage();assert.equal(p.e.bodyFitStartRun.disabled,true);
  const input=selectBodyFit(p);p.e.bodyFitStartFrame.value='2';
  p.e.bodyFitIterations.value='unused';p.e.bodyFitEvaluations.value='unused';
  assert.equal(p.e.bodyFitStartRun.disabled,false);await p.e.bodyFitStartRun.onclick();
  const w=p.current(),m=w.messages[0];
  assert.equal(m.op,'review-body-fit-start');assert.equal(m.input,input);assert.equal(m.startIndex,0);assert.equal(m.frameIndex,1);
  assert.equal(new TextDecoder().decode(m.model),'original model');assert.equal(m.iterations,undefined);assert.equal(m.maxEvaluations,undefined);
  assert.equal(p.e.bodyFitReviewCancel.disabled,false);assert.equal(p.e.bodyFitWorkbench.disabled,true);
  const value=bodyStartFrameReport({start_count:1,frame_index:1});
  const raw=' \r\n'+JSON.stringify(value).replaceAll('"timestamp_us":1','"timestamp_us":18446744073709551615')+'\r\n';
  await w.emit({type:'result',request:m.request,report:raw});
  assert.equal(w.dead,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.e.bodyFitReview.hidden,false);
  assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.e.bodyFitStartDetails.hidden,false);
  assert.match(p.e.bodyFitReviewHeading.textContent,/Declared initialization/);
  assert.match(p.e.bodyFitReviewStatus.textContent,/INITIALIZATION_REVIEW: start 1 of 1, frame 2 of 2/);
  assert.match(p.e.bodyFitReviewParameters.textContent,/height: 0.6 coefficient \(0 < value < 1\)/);
  assert.match(p.e.bodyFitReviewParameters.textContent,/root_tx: 0.2 m \(-3 < value < 3\)/);
  assert.equal(p.e.bodyFitReviewChecks.children.length,3);assert.equal(p.e.bodyFitAcceptedReviewControls.hidden,true);
  const [json,obj]=p.e.bodyFitReviewDownloads.children;
  assert.equal(json.download,'body-fit-start-0000-frame-0001.json');assert.equal(obj.download,'body-fit-start-0000-frame-0001.obj');
  assert.equal(await p.urls.get(json.href).text(),raw);assert.equal(await p.urls.get(obj.href).text(),value.mesh_obj);
  assert.equal(p.e.bodyFitReport.textContent,'');assert.equal(p.e.bodyFitDownloads.children.length,0);
  assert.equal(p.e.bodyFitWorkbench.disabled,true);p.e.bodyFitWorkbench.onclick();assert.equal(p.popups.length,0);
  assert.equal(p.e.openWorkbench.disabled,true);assert.equal(p.e.bodyFitReviewRun.disabled,true);
});

test('body-start preview retains failed projections, large residuals and every unavailable silhouette comparison',async()=>{
  const p=await photoPage();selectBodyFit(p);await p.e.bodyFitStartRun.onclick();const w=p.current();
  const value=bodyStartFrameReport({mesh_obj:null,triangles:[],projected_vertices_px:[],unprojectable_vertices:0,
    keypoints:[
      {name:'behind-camera',partition:'fit',observed_px:[20,30],predicted_px:null,residual_px:null,projection_refusal:{code:'InvalidGeometry',message:'point behind camera'}},
      {name:'missing-model-point',partition:'held_out',observed_px:[30,40],projection_refusal:{message:'model evaluation failed'}},
      {name:'distant-guess',partition:'fit',observed_px:[40,50],predicted_px:[10000,12000],residual_px:12345.678901}],
    silhouette:{lines:[{observation:{axis:'horizontal',at_px:70,partition:'held_out',intervals_px:[[10,40]]},comparison:null}]},
    diagnostics:[{stage:'model_surface',refusal:{message:'no usable surface'}},{stage:'silhouette_projection',refusal:{message:'coverage unavailable'}}]});
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(value)});
  assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.e.bodyFitReviewError.textContent,'');
  const labels=p.e.bodyFitReviewCanvas.labels.map(([text])=>text);
  assert.ok(labels.some(text=>text.includes('behind-camera (FIT, projection unavailable)')));
  assert.ok(labels.some(text=>text.includes('missing-model-point (CHECK, projection unavailable)')));
  assert.ok(labels.some(text=>text.includes('distant-guess (FIT, 12345.679 px)')));
  const rows=p.e.bodyFitReviewChecks.children.map(row=>row.children.map(cell=>cell.textContent));
  assert.equal(rows.length,4);assert.equal(rows[0][2],'Unavailable');assert.match(rows[0][3],/point behind camera/);
  assert.equal(rows[1][1],'CHECK');assert.equal(rows[2][2],'12345.678901 px');assert.equal(rows[3][2],'Unavailable');
  assert.match(p.e.bodyFitReviewDiagnostics.textContent,/no usable surface/);assert.match(p.e.bodyFitReviewDiagnostics.textContent,/coverage unavailable/);
  assert.match(p.e.bodyFitReviewSummary.textContent,/No usable mesh was produced/);
  assert.deepEqual(p.e.bodyFitReviewDownloads.children.map(a=>a.download),['body-fit-start-0000-frame-0000.json']);
  assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitResult.hidden,true);
});

test('body-start preview refuses malformed files, undeclared choices and inconsistent engine output without an accepted body',async()=>{
  for(const change of [p=>p.e.bodyFitModel.files=[],p=>p.e.bodyFitInput.files=[],
    p=>p.e.bodyFitModel.files=[{size:512*1024*1024+1}],p=>p.e.bodyFitInput.files=[{size:16*1024*1024+1}],
    p=>p.e.bodyFitInput.files=[{size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer}],
    p=>p.e.bodyFitStartIndex.value='',p=>p.e.bodyFitStartIndex.value='0',p=>p.e.bodyFitStartIndex.value='5',
    p=>p.e.bodyFitStartFrame.value='9',p=>p.e.bodyFitStartFrame.value='1.5']) {
    const p=await photoPage();selectBodyFit(p);change(p);await p.e.bodyFitStartRun.onclick();
    assert.equal(p.workers.length,0);assert.ok(p.e.bodyFitReviewError.textContent);assert.equal(p.urls.size,0);
    assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
  }
  for(const outcome of ['refused','crash',{schema:'ftx-body-frame-review/1'},{status:'ACCEPTED'},{body_fitted:true},
    {start_index:1},{frame_index:1},{start_count:0},{frame_count:1},{pose_values:[]},{diagnostics:null}]) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitStartRun.onclick();const w=p.current(),request=w.messages[0].request;
    if(outcome==='refused')await w.emit({type:'error',request,message:'no declared initialization at that index'});
    else if(outcome==='crash')w.onerror({message:'preview worker stopped'});
    else await w.emit({type:'result',request,report:JSON.stringify(bodyStartFrameReport(outcome))});
    assert.equal(w.dead,true);assert.ok(p.e.bodyFitReviewError.textContent);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
    assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.urls.size,0);
    assert.match(p.e.bodyFitReviewStatus.textContent,/no fitting was run/);
  }
});

test('body-start selection, cancellation and source changes withdraw prior reviews and ignore stale workers',async()=>{
  for(const invalidate of [p=>p.e.bodyFitReviewCancel.onclick(),p=>p.e.bodyFitStartIndex.handlers.input(),
    p=>p.e.bodyFitStartFrame.handlers.input(),p=>p.e.bodyFitInput.handlers.change(),p=>p.e.bodyFitModel.handlers.click(),
    p=>p.e.bodyFitInput.handlers.cancel(),p=>p.e.bodyFitIterations.handlers.input(),p=>p.windowEvents.pagehide()]) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitStartRun.onclick();let w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyStartFrameReport())});assert.equal(p.urls.size,2);
    await p.e.bodyFitStartRun.onclick();w=p.current();assert.equal(p.urls.size,0);invalidate(p);
    const message=p.e.bodyFitReviewStatus.textContent;
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyStartFrameReport())});
    assert.equal(w.dead,true);assert.equal(p.e.bodyFitReviewStatus.textContent,message);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
    assert.equal(p.e.bodyFitStartDetails.hidden,true);assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.urls.size,0);
  }
  const p=await photoPage();selectBodyFit(p);await p.e.bodyFitStartRun.onclick();const old=p.current();
  await p.e.bodyFitRun.onclick();const fit=p.current();
  await old.emit({type:'result',request:old.messages[0].request,report:JSON.stringify(bodyStartFrameReport())});
  assert.equal(old.dead,true);assert.equal(fit.dead,undefined);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);
  await fit.emit({type:'result',request:fit.messages[0].request,report:JSON.stringify(bodyFitReport())});
  assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.e.bodyFitStartDetails.hidden,true);
});

test('body-start delayed source reads cannot create a worker or erase a newer reviewed initialization',async()=>{
  for(const which of ['bodyFitInput','bodyFitModel'])for(const rejected of [false,true]) {
    const p=await photoPage();selectBodyFit(p);let release,fail,entered;
    const reading=new Promise(resolve=>{entered=resolve;});
    p.e[which].files=[{size:2,arrayBuffer:()=>{entered();return new Promise((yes,no)=>{release=yes;fail=no;});}}];
    const pending=p.e.bodyFitStartRun.onclick();await reading;p.e.bodyFitReviewCancel.onclick();
    selectBodyFit(p);await p.e.bodyFitStartRun.onclick();const current=p.current();
    await current.emit({type:'result',request:current.messages[0].request,report:JSON.stringify(bodyStartFrameReport())});
    const message=p.e.bodyFitReviewStatus.textContent;
    if(rejected)fail(Error('old read failed'));else release(new TextEncoder().encode('{}').buffer);
    await pending;assert.equal(p.workers.length,1);assert.equal(p.e.bodyFitReviewStatus.textContent,message);
    assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.e.bodyFitReviewError.textContent,'');
    assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.urls.size,2);
  }
});

test('body-start preview shares verified photographs without accepting a body and clears them on a new selection',async()=>{
  let closed=0;const p=await photoPage({decode:async()=>({width:640,height:480,close(){closed++;}})});
  selectBodyFit(p);await p.e.bodyFitStartRun.onclick();const w=p.current();
  const source_image={frame:{stream:'synthetic',timestamp_us:1},image_name:'source.png',image_sha256:'ab'.repeat(32),image_size_px:[640,480],rotation_cw_deg:90};
  const review=' \r\n'+JSON.stringify(bodyStartFrameReport({source_image})).replaceAll('"timestamp_us":1','"timestamp_us":18446744073709551615')+'\r\n';
  await w.emit({type:'result',request:w.messages[0].request,report:review});
  assert.equal(p.e.bodyFitReviewImage.disabled,false);selectBodyReviewImage(p);await p.e.bodyFitReviewImageRun.onclick();
  let image=p.current();assert.equal(image.messages[0].review,review);
  await image.emit({type:'error',request:image.messages[0].request,message:'wrong source image'});
  assert.match(p.e.bodyFitReviewImageStatus.textContent,/declared start and fit inputs are unchanged/);
  await p.e.bodyFitReviewImageRun.onclick();image=p.current();
  await image.emit({type:'result',request:image.messages[0].request,preview:new Uint8Array([8,5,3])});
  assert.match(p.e.bodyFitReviewImageStatus.textContent,/does not fit or accept a body/);
  assert.equal(p.e.bodyFitWorkbench.disabled,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.urls.size,2);
  p.e.bodyFitStartIndex.value='2';p.e.bodyFitStartIndex.handlers.input();
  assert.equal(closed,1);assert.equal(p.e.bodyFitReviewCanvas.hidden,true);assert.equal(p.e.bodyFitReviewImage.disabled,true);assert.equal(p.urls.size,0);
});

test('body-start review leaves an existing accepted body separate and can return to accepted-frame inspection',async()=>{
  const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const fit=p.current(),raw=JSON.stringify(bodyFitReport());
  await fit.emit({type:'result',request:fit.messages[0].request,report:raw});
  await p.e.bodyFitStartRun.onclick();const initial=p.current();
  await initial.emit({type:'result',request:initial.messages[0].request,report:JSON.stringify(bodyStartFrameReport())});
  assert.equal(p.e.bodyFitReport.textContent,raw);assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,5);
  p.e.bodyFitWorkbench.onclick();const popup=p.popups[0],token=new URL(popup.url).searchParams.get('bodyFitImport');
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token}});
  assert.equal(await popup.messages[0].data.fit.text(),raw,'Only the original accepted report is reusable');
  await p.e.bodyFitReviewRun.onclick();const accepted=p.current();assert.equal(accepted.messages[0].report,raw);
  await accepted.emit({type:'result',request:accepted.messages[0].request,report:JSON.stringify(bodyFrameReport())});
  assert.match(p.e.bodyFitReviewHeading.textContent,/accepted frame/);assert.equal(p.e.bodyFitStartDetails.hidden,true);
  assert.equal(p.e.bodyFitReviewChecks.children.length,0);assert.equal(p.e.bodyFitReviewDownloads.children[0].download,'body-fit-frame-0000.json');
  assert.equal(p.e.bodyFitReport.textContent,raw);assert.equal(p.urls.size,5);
});

test('body-frame photograph worker verifies exact original bytes and frees previews without refitting',async()=>{
  const source={request:81,op:'review-body-fit-image',image:new Uint8Array([255,216,0,7]).buffer,
    review:' \r\n{"timestamp_us":18446744073709551615,"source_image":{"original":"λ"}}\r\n'};
  const w=await photoWorker(),result=await w.send(source);
  assert.equal(result.type,'result');assert.deepEqual([...result.preview],[8,5,3]);
  assert.equal(result.report,' {"verified":"original image"}\r\n');
  assert.deepEqual(w.calls,[{kind:'body-frame-image',image:[255,216,0,7],review:source.review}]);
  assert.equal(w.instances[0].freed,true);
  for(const bad of [{review:null},{review:' '.repeat(16*1024*1024+1)},{image:[255,216]},
    {image:new ArrayBuffer(0)},{image:new ArrayBuffer(128*1024*1024+1)}]) {
    const refused=await photoWorker();assert.equal((await refused.send({...source,...bad})).type,'error');
    assert.equal(refused.calls.length,0);
  }
  const refused=await photoWorker({failBodyImage:true});
  assert.match((await refused.send(source)).message,/SHA-256 mismatch/);assert.equal(refused.calls.length,1);
  assert.equal(refused.instances.length,0,'No alternate decoder or unverified fallback');
  const failed=await photoWorker({failBodyImagePreview:true});
  assert.match((await failed.send(source)).message,/preview failed/);assert.equal(failed.instances[0].freed,true);
});

test('body-frame photograph import requires a retained capture source identity',async()=>{
  for(const source_image of [undefined,null]) {
    const p=await photoPage();assert.equal(p.e.bodyFitReviewImage.disabled,true);
    await openCaptureBodyReview(p,{source_image});
    assert.equal(p.e.bodyFitReviewImage.disabled,true);assert.match(p.e.bodyFitReviewImageSource.textContent,/no retained original-image identity/);
    selectBodyReviewImage(p);assert.equal(p.e.bodyFitReviewImageRun.disabled,true);
    await p.e.bodyFitReviewImageRun.onclick();assert.equal(p.workers.length,1);
    assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.urls.size,3);
    assert.ok(!p.e.bodyFitReviewCanvas.drawing.some(([kind])=>kind==='image'));
  }
});

test('body-frame photograph preview sits behind projections in stored orientation and leaves exact reports unchanged',async()=>{
  let closed=0;const decoded={width:640,height:480,close(){closed++;}},decodes=[];
  const p=await photoPage({decode:async(blob,options)=>{decodes.push({bytes:[...new Uint8Array(await blob.arrayBuffer())],options});return decoded;}});
  const review=await openCaptureBodyReview(p),fit=p.e.bodyFitReport.textContent;
  assert.equal(p.e.bodyFitReviewImage.disabled,false);assert.match(p.e.bodyFitReviewImageSource.textContent,/capture & <original>\.png/);
  const source='original \0 JPEG source λ';selectBodyReviewImage(p,file(source));await p.e.bodyFitReviewImageRun.onclick();
  const w=p.current(),message=w.messages[0];
  assert.equal(message.op,'review-body-fit-image');assert.equal(message.review,review);
  assert.equal(new TextDecoder().decode(message.image),source);assert.equal(p.e.bodyFitReviewImageClear.disabled,false);
  assert.equal(p.e.bodyFitReviewCancel.disabled,false);assert.equal(p.e.bodyFitReviewImageRun.disabled,true);
  p.e.bodyFitReviewCanvas.drawing=[];
  await w.emit({type:'result',request:message.request,preview:new Uint8Array([8,5,3]),report:'original verified receipt'});
  assert.equal(w.dead,true);assert.equal(decodes.length,1);assert.deepEqual(decodes[0].bytes,[8,5,3],'Only native-verified PNG bytes reach the browser decoder');
  assert.equal(decodes[0].options.imageOrientation,'none');
  const drawing=p.e.bodyFitReviewCanvas.drawing;
  assert.deepEqual(drawing[0],['fill',0,0,640,480]);assert.deepEqual(drawing[1],['image',decoded,0,0,640,480]);
  assert.ok(drawing.findIndex(([kind])=>kind==='label')>1,'Observations and labels are painted over the photograph');
  assert.match(p.e.bodyFitReviewImageStatus.textContent,/640 × 480 stored pixels; 90° clockwise rotation already applied/);
  assert.match(p.e.bodyFitReviewImageStatus.textContent,new RegExp('ab'.repeat(32)));
  assert.equal(p.e.bodyFitReviewCanvas.width,640);assert.equal(p.e.bodyFitReviewCanvas.height,480,'Recorded quarter-turn is not applied twice');
  assert.equal(p.e.bodyFitReport.textContent,fit);assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,3);
  assert.equal(await p.urls.get(p.e.bodyFitReviewDownloads.children[0].href).text(),review);
  p.e.bodyFitReviewMesh.handlers.change();assert.equal(p.workers.length,2);assert.equal(closed,0);
  p.e.bodyFitReviewCanvas.drawing=[];p.e.bodyFitReviewImageClear.onclick();
  assert.equal(closed,1);assert.equal(p.e.bodyFitReviewImage.files.length,0);assert.equal(p.e.bodyFitReviewCanvas.hidden,false);
  assert.ok(!p.e.bodyFitReviewCanvas.drawing.some(([kind])=>kind==='image'));assert.equal(p.urls.size,3);
  assert.equal(p.e.bodyFitReviewImageRun.disabled,true);assert.equal(p.e.bodyFitReviewImageClear.disabled,true);
});

test('body-frame photograph refuses invalid sources and previews while retaining the accepted fit and geometry',async()=>{
  for(const source of [null,{size:0},{size:128*1024*1024+1}]) {
    const p=await photoPage();await openCaptureBodyReview(p);
    if(source)selectBodyReviewImage(p,source);await p.e.bodyFitReviewImageRun.onclick();
    assert.equal(p.workers.length,1);assert.match(p.e.bodyFitReviewImageError.textContent,/128 MiB/);
    assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,3);
  }
  const large=await photoPage();await openCaptureBodyReview(large,{image_size_px:[10000,4001]});
  selectBodyReviewImage(large);await large.e.bodyFitReviewImageRun.onclick();
  assert.equal(large.workers.length,1);assert.match(large.e.bodyFitReviewImageError.textContent,/40 million pixels/);
  for(const outcome of ['refused','crash','missing-preview','wrong-dimensions','decode-error']) {
    let decodes=0,closed=0;
    const p=await photoPage({decode:async()=>{decodes++;if(outcome==='decode-error')throw Error('PNG decode failed');
      return {width:480,height:640,close(){closed++;}};}});
    await openCaptureBodyReview(p);selectBodyReviewImage(p);await p.e.bodyFitReviewImageRun.onclick();
    const w=p.current(),request=w.messages[0].request;
    if(outcome==='refused')await w.emit({type:'error',request,message:'original capture image SHA-256 mismatch'});
    else if(outcome==='crash')w.onerror({message:'image worker stopped'});
    else await w.emit({type:'result',request,preview:outcome==='missing-preview'?new Uint8Array():new Uint8Array([8,5,3])});
    assert.equal(w.dead,true);assert.ok(p.e.bodyFitReviewImageError.textContent);
    assert.match(p.e.bodyFitReviewImageStatus.textContent,/frame review and accepted fit are unchanged/);
    assert.equal(decodes,['wrong-dimensions','decode-error'].includes(outcome)?1:0);assert.equal(closed,outcome==='wrong-dimensions'?1:0);
    assert.equal(p.e.bodyFitReviewCanvas.hidden,false);assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.urls.size,3);
    assert.ok(!p.e.bodyFitReviewCanvas.drawing.some(([kind])=>kind==='image'));assert.equal(p.e.bodyFitReviewImageRun.disabled,false);
  }
});

test('body-frame photograph cancellation and source changes discard old workers and close displayed bitmaps',async()=>{
  for(const completed of [false,true])for(const [invalidate,keepReview,keepFit] of [
    [p=>p.e.bodyFitReviewImageClear.onclick(),true,true],
    [p=>selectBodyReviewImage(p,file('replacement image')),true,true],
    [p=>p.e.bodyFitReviewCancel.onclick(),false,true],
    [p=>p.e.bodyFitReviewFrame.handlers.change(),false,true],
    [p=>p.e.bodyFitModel.handlers.change(),false,false],
    [p=>p.windowEvents.pagehide(),false,false]]) {
    let decodes=0,closed=0;const p=await photoPage({decode:async()=>{decodes++;return {width:640,height:480,close(){closed++;}};}});
    await openCaptureBodyReview(p);selectBodyReviewImage(p);await p.e.bodyFitReviewImageRun.onclick();
    const w=p.current(),result={type:'result',request:w.messages[0].request,preview:new Uint8Array([8,5,3])};
    if(completed)await w.emit(result);
    invalidate(p);const message=p.e.bodyFitReviewImageStatus.textContent;await w.emit(result);
    assert.equal(w.dead,true);assert.equal(decodes,completed?1:0);assert.equal(closed,completed?1:0);
    assert.equal(p.e.bodyFitReviewImageStatus.textContent,message);assert.equal(p.e.bodyFitReviewCanvas.hidden,!keepReview);
    assert.equal(p.e.bodyFitWorkbench.disabled,!keepFit);assert.equal(p.urls.size,keepReview?3:keepFit?1:0);
  }
});

test('body-frame photograph delayed reads and decodes cannot replace or erase a newer verified image',async()=>{
  for(const phase of ['read','decode'])for(const rejected of [false,true]) {
    let release,fail,entered,decodes=0,lateClosed=0;
    const reading=new Promise(resolve=>{entered=resolve;}),late=()=>new Promise((yes,no)=>{release=yes;fail=no;entered();});
    const currentBitmap={width:640,height:480,close(){}},lateBitmap={width:640,height:480,close(){lateClosed++;}};
    const p=await photoPage({decode:async()=>{decodes++;return phase==='decode' && decodes===1?late():currentBitmap;}});
    await openCaptureBodyReview(p);selectBodyReviewImage(p,phase==='read'?{size:4,arrayBuffer:late}:file('old image'));
    let pending=p.e.bodyFitReviewImageRun.onclick();
    if(phase==='decode') {await pending;const old=p.current();pending=old.emit({type:'result',request:old.messages[0].request,preview:new Uint8Array([8,5,3])});}
    await reading;p.e.bodyFitReviewImageClear.onclick();selectBodyReviewImage(p,file('new image'));
    await p.e.bodyFitReviewImageRun.onclick();const w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,preview:new Uint8Array([8,5,3])});
    const message=p.e.bodyFitReviewImageStatus.textContent,count=p.workers.length;
    if(rejected)fail(Error('old source failed'));else release(phase==='read'?new Uint8Array([1,2]).buffer:lateBitmap);
    await pending;assert.equal(p.workers.length,count);assert.equal(p.e.bodyFitReviewImageStatus.textContent,message);
    assert.match(message,/Verified original capture photograph/);assert.equal(p.e.bodyFitReviewImageError.textContent,'');
    assert.equal(lateClosed,phase==='decode' && !rejected?1:0);assert.equal(p.urls.size,3);
    assert.ok(!p.e.bodyFitReviewCanvas.drawing.some(([kind,image])=>kind==='image' && image===lateBitmap));
    assert.equal(p.e.bodyFitWorkbench.disabled,false);assert.equal(p.e.bodyFitReviewCanvas.hidden,false);
  }
});

test('accepted body handoff sends the original report and selected model once to the expected local tab',async()=>{
  const p=await photoPage();selectBodyFit(p);
  assert.equal(p.e.bodyFitWorkbench.disabled,true);
  await p.e.bodyFitRun.onclick();const w=p.current();
  const raw=JSON.stringify(bodyFitReport()).replace('"timestamp_us":1','"timestamp_us":18446744073709551615');
  await w.emit({type:'result',request:w.messages[0].request,report:raw});
  assert.equal(p.e.bodyFitWorkbench.disabled,false);
  p.e.bodyFitWorkbench.onclick();p.e.bodyFitWorkbench.onclick();assert.equal(p.popups.length,1);
  const popup=p.popups[0],token=new URL(popup.url).searchParams.get('bodyFitImport');
  assert.equal(new URL(popup.url).pathname,'/index.html');assert.match(token,/^[0-9a-f]{32}$/);
  const ready={type:'ftx-body-fit-sources-ready',token};
  for(const [origin,source,data] of [['https://foreign.invalid',popup.target,ready],
    ['https://unit.invalid',{},ready],['https://unit.invalid',popup.target,{...ready,token:'wrong'}]]) {
    p.windowEvents.message({origin,source,data});
  }
  assert.equal(popup.messages.length,0);
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:ready});
  const packet=popup.messages[0];
  assert.equal(packet.origin,'https://unit.invalid');assert.equal(packet.data.type,'ftx-body-fit-sources');
  assert.equal(await packet.data.fit.text(),raw);assert.equal(await packet.data.model.text(),'original model');
  assert.equal(p.e.openWorkbench.disabled,true,'Accepted predictions never enter the measurement handoff');
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:ready});
  assert.equal(popup.messages.length,1);
  p.e.bodyFitInput.handlers.change();assert.equal(p.e.bodyFitWorkbench.disabled,true);
  assert.equal(await packet.data.fit.text(),raw,'An already received snapshot is immutable');
});
test('accepted body handoff retains evaluated model bytes independently of later file reads and worker buffers',async()=>{
  const p=await photoPage();selectBodyFit(p);let reads=0;
  class ReadOnceModel extends Blob {
    async arrayBuffer() {
      assert.equal(++reads,1,'The evaluated model must not be read from the selected file again');
      return super.arrayBuffer();
    }
  }
  const source=new ReadOnceModel(['original model']);p.e.bodyFitModel.files=[source];
  await p.e.bodyFitRun.onclick();const w=p.current();
  assert.equal(new TextDecoder().decode(w.messages[0].model),'original model');
  // The worker owns the transferred buffer; later mutations must not alter the retained snapshot.
  new Uint8Array(w.messages[0].model).fill(0);
  const raw=' \r\n'+JSON.stringify(bodyFitReport())
    .replace('"timestamp_us":1','"timestamp_us":18446744073709551615')
    .replace('"shape":[0.6]','"shape":[-0.0]')+'\r\n';
  await w.emit({type:'result',request:w.messages[0].request,report:raw});
  p.e.bodyFitWorkbench.onclick();const popup=p.popups[0],token=new URL(popup.url).searchParams.get('bodyFitImport');
  p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token}});
  const packet=popup.messages[0].data;
  assert.notEqual(packet.model,source,'The handoff owns a byte snapshot, not the selected file');
  assert.equal(new TextDecoder().decode(await packet.model.arrayBuffer()),'original model');
  assert.equal(await packet.fit.text(),raw);assert.equal(reads,1);
});

test('pending accepted-body transfers are cancelled by source edits, work-limit changes and page exit',async()=>{
  for(const invalidate of [p=>p.e.bodyFitInput.handlers.change(),p=>p.e.bodyFitModel.handlers.click(),
    p=>p.e.bodyFitInput.handlers.cancel(),p=>p.e.bodyFitIterations.handlers.input(),
    p=>p.e.bodyFitEvaluations.handlers.input(),p=>p.windowEvents.pagehide()]) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFitReport())});
    p.e.bodyFitWorkbench.onclick();const popup=p.popups[0];
    const token=new URL(popup.url).searchParams.get('bodyFitImport');invalidate(p);
    p.windowEvents.message({origin:'https://unit.invalid',source:popup.target,data:{type:'ftx-body-fit-sources-ready',token}});
    assert.equal(popup.messages[0].data.type,'ftx-body-fit-sources-cancelled');
    assert.equal(popup.messages[0].data.fit,undefined);assert.equal(popup.messages[0].data.model,undefined);
    assert.equal(p.e.bodyFitWorkbench.disabled,true);
  }
  for(const status of ['REJECTED','AMBIGUOUS','UNOBSERVABLE','NOT_CONVERGED','BUDGET_STOP']) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const w=p.current();
    await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFitReport({status,solution:null}))});
    assert.equal(p.e.bodyFitWorkbench.disabled,true);p.e.bodyFitWorkbench.onclick();assert.equal(p.popups.length,0);
  }
});

test('body-fit source edits, cancellation and page exit withdraw results and discard delayed reads and workers',async()=>{
  for(const invalidate of [p=>p.e.bodyFitCancel.onclick(),p=>p.e.bodyFitInput.handlers.change(),p=>p.e.bodyFitModel.handlers.click(),
    p=>p.e.bodyFitInput.handlers.cancel(),p=>p.e.bodyFitIterations.handlers.input(),p=>p.e.bodyFitEvaluations.handlers.input(),p=>p.windowEvents.pagehide()]) {
    const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const old=p.current();invalidate(p);
    await old.emit({type:'result',request:old.messages[0].request,report:JSON.stringify(bodyFitReport())});
    assert.equal(old.dead,true);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.urls.size,0);
    selectBodyFit(p);await p.e.bodyFitRun.onclick();const next=p.current();await next.emit({type:'result',request:next.messages[0].request,report:JSON.stringify(bodyFitReport())});
    assert.equal(p.urls.size,3);invalidate(p);assert.equal(p.urls.size,0);assert.equal(p.e.bodyFitResult.hidden,true);
  }
  for(const which of ['bodyFitInput','bodyFitModel']) {
    const p=await photoPage();selectBodyFit(p);let release,entered;
    const reading=new Promise(resolve=>{entered=resolve;});
    p.e[which].files=[{size:2,arrayBuffer:()=>new Promise(resolve=>{release=resolve;entered();})}];
    const pending=p.e.bodyFitRun.onclick();await reading;
    p.e.bodyFitCancel.onclick();release(new TextEncoder().encode('{}').buffer);await pending;
    assert.equal(p.workers.length,0);assert.equal(p.e.bodyFitRun.disabled,false);
  }
});
test('body-fit page refuses malformed sources and budgets before dispatch and clears malformed engine output',async()=>{
  for(const change of [p=>p.e.bodyFitModel.files=[],p=>p.e.bodyFitModel.files=[{size:512*1024*1024+1}],
    p=>p.e.bodyFitInput.files=[{size:16*1024*1024+1}],p=>p.e.bodyFitInput.files=[{size:1,arrayBuffer:async()=>new Uint8Array([255]).buffer}],
    p=>p.e.bodyFitIterations.value='',p=>p.e.bodyFitIterations.value='101',p=>p.e.bodyFitEvaluations.value='1.5']) {
    const p=await photoPage();selectBodyFit(p);change(p);await p.e.bodyFitRun.onclick();
    assert.equal(p.workers.length,0);assert.ok(p.e.bodyFitError.textContent);assert.equal(p.urls.size,0);
  }
  const p=await photoPage();selectBodyFit(p);await p.e.bodyFitRun.onclick();const w=p.current();
  await w.emit({type:'result',request:w.messages[0].request,report:JSON.stringify(bodyFitReport({solution:null}))});
  assert.match(p.e.bodyFitError.textContent,/consistent body-fit/);assert.equal(p.e.bodyFitResult.hidden,true);assert.equal(p.urls.size,0);
});

test('still-window worker calls only the original assessor with exact source text',async()=>{
  const input=' \r\n{"timestamp_us":18446744073709551615,"evidence":"λ"}\r\n';
  const source='original λ.json';
  const w=await photoWorker();assert.equal((await w.send({request:22,op:'still-window',input,source,noise:'inactive',iterations:'1.5'})).type,'result');
  assert.deepEqual(w.calls,[{kind:'still-window',input,source}]);assert.equal(w.instances.length,0);
  const failed=await photoWorker({failStill:true});assert.equal((await failed.send({request:22,op:'still-window',input,source})).type,'error');
  assert.deepEqual(failed.calls,[{kind:'still-window',input,source}]);
});

test('still-window mode preserves raw reports and source, reports motion and never promotes a candidate',async()=>{
  const p=await photoPage(),input=' \r\n{"evidence":"λ","timestamp_us":18446744073709551615}\r\n';
  p.e.triangulationInput.files=[file(input,'original λ.json')];p.e.triangulationNoise.files=[file('old noise')];p.e.bundleIterations.value='1.5';
  p.e.geometryOperation.value='still-window';p.e.geometryOperation.handlers.change();
  assert.equal(p.e.triangulationNoise.files.length,0);assert.equal(p.e.triangulationNoise.disabled,true);
  assert.equal(p.e.geometryNoiseControls.hidden,true);assert.equal(p.e.geometryBundleControls.hidden,true);
  assert.equal(p.e.geometryStillControls.hidden,false);assert.equal(p.e.bundleIterations.disabled,true);
  assert.equal(p.e.geometryInputKind.textContent,'ftx-still-window-input/1');
  for(const report of [stillReport(),stillReport({status:'REJECTED',checks:[{name:'moving',partition:'fit',motion_mm:10,passed:false,flags:['exceeds 5 mm']}]}),
    stillReport({status:'UNOBSERVABLE',checks:[{name:'weak',partition:'held_out',motion_mm:null,passed:false,flags:['below 3 deg']}]})]) {
    await p.e.triangulationRun.onclick();const w=p.current(),message=w.messages[0];
    assert.deepEqual({op:message.op,input:message.input,noise:message.noise,iterations:message.iterations},{op:'still-window',input,noise:'',iterations:''});
    assert.equal(message.source,'original λ.json');
    const raw=JSON.stringify(report);await w.emit({type:'result',request:message.request,report:raw});
    const links=p.e.triangulationDownloads.children;
    assert.deepEqual(links.map(a=>a.download),['still-window.json','still-window-input.json']);
    assert.deepEqual(await Promise.all(links.map(a=>p.urls.get(a.href).text())),[raw,input]);
    assert.match(p.e.triangulationSummary.textContent,/3 FIT landmarks, 1 held-out.*5 mm/);
    assert.match(p.e.triangulationPrecision.textContent,/Candidate only: human review/);
    assert.match(p.e.triangulationPoint.textContent,new RegExp(report.checks[0].name));
    if(report.status==='UNOBSERVABLE')assert.match(p.e.triangulationPoint.textContent,/unavailable.*below 3 deg/);
    if(report.status==='REJECTED')assert.match(p.e.triangulationPoint.textContent,/10 mm.*exceeds 5 mm/);
    assert.equal(p.e.openWorkbench.disabled,true);assert.equal(p.e.returnMaterials.disabled,true);
    assert.equal(p.e.downloads.children.length,0);
  }
});

test('still-window cancellation and mode/source edits discard late workers, reads and old downloads',async()=>{
  const p=await photoPage();p.e.geometryOperation.value='still-window';p.e.geometryOperation.handlers.change();p.e.triangulationInput.files=[file('{}')];
  for(const cancel of [()=>p.e.triangulationCancel.onclick(),()=>p.e.triangulationInput.handlers.change(),()=>{
    p.e.geometryOperation.value='bundle-adjust';p.e.geometryOperation.handlers.change();}]) {
    p.e.geometryOperation.value='still-window';p.e.geometryOperation.handlers.change();
    await p.e.triangulationRun.onclick();const old=p.current();cancel();assert.equal(old.dead,true);
    await old.emit({type:'result',request:old.messages[0].request,report:JSON.stringify(stillReport())});
    assert.equal(p.e.triangulationResult.hidden,true);assert.equal(p.urls.size,0);
  }
  p.e.geometryOperation.value='still-window';p.e.geometryOperation.handlers.change();
  let finish;const count=p.workers.length;p.e.triangulationInput.files=[{size:2,arrayBuffer:()=>new Promise(resolve=>{finish=resolve;})}];
  const pending=p.e.triangulationRun.onclick();p.e.geometryOperation.value='triangulate';p.e.geometryOperation.handlers.change();
  finish(new TextEncoder().encode('{}').buffer);await pending;assert.equal(p.workers.length,count);
});

test('compiled body fitting reproduces complete native multistart reports and preserves exact source identities',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_BODY_FIT_PORTABLE_DIR},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_BODY_FIT_PORTABLE_DIR;
    const model=fs.readFileSync(path.join(dir,'model.ftxbody'));
    const parse=text=>JSON.parse(text,(key,value,meta)=>key==='timestamp_us'?meta.source:value);
    const hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    for(const item of JSON.parse(fs.readFileSync(path.join(dir,'cases.json'),'utf8')).cases) {
      const input=fs.readFileSync(path.join(dir,item.input),'utf8');
      const declared=parse(input),initialText=ftx.review_body_fit_start(model,input,'0','0'),initial=parse(initialText);
      assert.equal(initial.schema,'ftx-body-start-frame-review/1');assert.equal(initial.status,'INITIALIZATION_REVIEW');
      assert.equal(initial.body_fitted,false);assert.equal(initial.pose_evaluations,1);assert.equal(initial.input_sha256,hash(input));
      assert.deepEqual(initial.frame,declared.fit.frames[0].frame,item.name+' original input frame ordinal');
      assert.deepEqual(initial.shape_values,declared.fit.starts[0].shape);
      const declaredPose=declared.fit.starts[0].poses.find(p=>p.frame.stream===initial.frame.stream && p.frame.timestamp_us===initial.frame.timestamp_us);
      assert.deepEqual(initial.pose_values,declaredPose.values,item.name+' exact authored pose');
      const startWorker=await photoWorker({bodyStartReview:ftx.review_body_fit_start});
      const startResult=await startWorker.send({request:85,op:'review-body-fit-start',model:Uint8Array.from(model).buffer,input,startIndex:0,frameIndex:0});
      assert.equal(startResult.type,'result',startResult.message);assert.deepEqual(parse(startResult.report),initial);
      const actual=ftx.fit_body(model,input,item.iterations,item.max_evaluations);
      assert.deepEqual(parse(actual),parse(fs.readFileSync(path.join(dir,item.report),'utf8')),item.name+' complete original native body-fit report');
      assert.equal(JSON.parse(actual).status,item.status);
      const savedReport=fs.readFileSync(path.join(dir,item.report),'utf8');
      if(item.status==='ACCEPTED') {
        const review=ftx.review_body_fit_frame(model,savedReport,'0');
        assert.ok(item.review,'Native fixture must retain its frame review');
        const savedReview=fs.readFileSync(path.join(dir,item.review),'utf8');
        assert.deepEqual(parse(review),parse(savedReview),item.name+' complete native frame review');
        if(item.image) {
          assert.ok(item.image_report && item.image_preview,'Native capture fixture must retain its image receipt and preview');
          const image=fs.readFileSync(path.join(dir,item.image)),expectedReceipt=parse(fs.readFileSync(path.join(dir,item.image_report),'utf8'));
          const expectedPreview=fs.readFileSync(path.join(dir,item.image_preview));
          // The receipt hashes the exact saved review bytes, including its native pretty printing.
          const photo=new ftx.WebBodyFitImage(image,savedReview);
          try {
            assert.deepEqual(parse(photo.report()),expectedReceipt,item.name+' complete native image receipt');
            assert.deepEqual(Buffer.from(photo.preview()),expectedPreview,item.name+' exact native PNG preview');
          } finally {photo.free();}
          const changed=Buffer.from(image);changed[changed.length-1]^=1;
          assert.throws(()=>new ftx.WebBodyFitImage(changed,savedReview),/SHA-256 mismatch/);
          const worker=await photoWorker({bodyImageClass:ftx.WebBodyFitImage});
          const result=await worker.send({request:82,op:'review-body-fit-image',review:savedReview,image:Uint8Array.from(image).buffer});
          assert.equal(result.type,'result',result.message);assert.deepEqual(parse(result.report),expectedReceipt);
          assert.deepEqual(Buffer.from(result.preview),expectedPreview,item.name+' compiled image through the actual worker');
          const refusedWorker=await photoWorker({bodyImageClass:ftx.WebBodyFitImage});
          const refused=await refusedWorker.send({request:83,op:'review-body-fit-image',review:savedReview,image:Uint8Array.from(changed).buffer});
          assert.equal(refused.type,'error');assert.match(refused.message,/SHA-256 mismatch/);
        }
      } else assert.throws(()=>ftx.review_body_fit_frame(model,savedReport,'0'),/ACCEPTED/);
    }
    const acceptedReport=fs.readFileSync(path.join(dir,'accepted-report.json'),'utf8');
    for(const index of ['','-1','2','8','1.5','4294967296','9007199254740993']) {
      assert.throws(()=>ftx.review_body_fit_frame(model,acceptedReport,index),/index|counts/);
    }
    const input=fs.readFileSync(path.join(dir,'accepted-input.json'),'utf8');
    for(const value of ['','0','101','1.5','4294967346','9007199254740993','NaN']) assert.throws(()=>ftx.fit_body(model,input,value,'100000'),/iterations/);
    for(const value of ['','0','1000001','1.5','4295067296','9007199254740993','NaN']) assert.throws(()=>ftx.fit_body(model,input,'50',value),/evaluations/);
    assert.throws(()=>ftx.fit_body(model,'\ufeff'+input,'50','100000'));
    assert.throws(()=>ftx.fit_body(model,' '.repeat(16*1024*1024+1),'50','100000'),/16 MiB/);
    const changed=Buffer.from(model);changed[changed.length-1]^=1;
    assert.throws(()=>ftx.fit_body(changed,input,'50','100000'),/SHA-256 mismatch/);
    assert.throws(()=>ftx.review_body_fit_frame(changed,acceptedReport,'0'),/SHA-256 mismatch/);
  });

test('compiled body-start editing preserves unchanged evidence and feeds actual preview fitting and worker paths',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_BODY_FIT_PORTABLE_DIR},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_BODY_FIT_PORTABLE_DIR;
    const model=fs.readFileSync(path.join(dir,'model.ftxbody')),input=fs.readFileSync(path.join(dir,'accepted-input.json'),'utf8');
    const parse=text=>JSON.parse(text,(key,value,meta)=>key==='timestamp_us' && typeof value==='number'?meta.source:value);
    const hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    const original=parse(input),prepared=JSON.parse(ftx.body_fit_starts(model,input,''));
    assert.equal(prepared.input,input);assert.equal(prepared.input_sha256,hash(input));assert.equal(prepared.original_input_sha256,hash(input));
    assert.equal(prepared.model_sha256,hash(model));assert.equal(prepared.starts.length,original.fit.starts.length);
    prepared.frames.forEach((frame,index)=>{
      assert.equal(frame.frame_index,index);assert.deepEqual(frame.identity,original.fit.frames[index].frame);
      prepared.starts.forEach((start,startIndex)=>{
        const expected=original.fit.starts[startIndex].poses.find(p=>p.frame.stream===frame.identity.stream && p.frame.timestamp_us===frame.identity.timestamp_us);
        assert.deepEqual(start.poses.find(p=>p.frame_index===index).values,expected.values);
      });
    });
    const edits={input_sha256:prepared.original_input_sha256,model_sha256:prepared.model_sha256,starts:structuredClone(prepared.starts)},parameter=prepared.shape[0];
    const replacement=[0.25,0.5,0.75,0.125,0.875].map(t=>parameter.lower*(1-t)+parameter.upper*t)
      .find(value=>value>parameter.lower && value<parameter.upper && edits.starts.every(start=>start.shape[0]!==value));
    assert.ok(Number.isFinite(replacement),'an independent interior start value exists');edits.starts[0].shape[0]=replacement;
    const edited=JSON.parse(ftx.body_fit_starts(model,input,JSON.stringify(edits))),request=parse(edited.input);
    assert.equal(edited.original_input_sha256,hash(input));assert.equal(edited.input_sha256,hash(edited.input));
    assert.deepEqual({...request,fit:{...request.fit,starts:[]}},{...original,fit:{...original.fit,starts:[]}},'all cameras, observations, evidence, roles, source identities and bounds unchanged');
    assert.deepEqual(request.fit.starts[0].shape,edits.starts[0].shape);
    const preview=parse(ftx.review_body_fit_start(model,edited.input,'0','0'));
    assert.deepEqual(preview.shape_values,edits.starts[0].shape);assert.deepEqual(preview.frame,original.fit.frames[0].frame);
    const fit=parse(ftx.fit_body(model,edited.input,'50','100000'));
    assert.equal(fit.schema,'ftx-body-fit/1');assert.equal(fit.status,'ACCEPTED');
    const worker=await photoWorker({bodyStarts:ftx.body_fit_starts});
    const result=await worker.send({request:92,op:'body-fit-starts',model:Uint8Array.from(model).buffer,input,edits:JSON.stringify(edits)});
    assert.equal(result.type,'result',result.message);assert.deepEqual(JSON.parse(result.report),edited);
    const p=await photoPage();p.e.bodyFitInput.files=[file(input)];p.e.bodyFitModel.files=[new Blob([model])];p.e.bodyFitInput.handlers.change();
    await p.e.bodyStartsLoad.onclick();let live=p.current();
    await live.emit({type:'result',request:live.messages[0].request,report:ftx.body_fit_starts(model,input,'')});
    assert.equal(p.e.bodyStartsError.textContent,'');editBodyStart(p,'bodyStart-shape-0',String(replacement));
    await p.e.bodyStartsApply.onclick();live=p.current();
    await live.emit({type:'result',request:live.messages[0].request,report:ftx.body_fit_starts(model,live.messages[0].input,live.messages[0].edits)});
    assert.equal(p.e.bodyStartsError.textContent,'');assert.equal(await p.urls.get(p.e.bodyStartsDownloads.children[0].href).text(),edited.input);
    await p.e.bodyFitRun.onclick();assert.equal(p.current().messages[0].input,edited.input);
    const one=JSON.parse(ftx.body_fit_starts(model,input,JSON.stringify({starts:[edits.starts[0]]})));
    assert.equal(JSON.parse(ftx.review_body_fit_start(model,one.input,'0','0')).body_fitted,false);
    assert.throws(()=>ftx.fit_body(model,one.input,'50','100000'),/start/);
    assert.throws(()=>ftx.body_fit_starts(model,input,JSON.stringify({starts:[edits.starts[0],edits.starts[0]]})),/distinct|duplicate/);
    const boundary=structuredClone(edits);boundary.starts[0].shape[0]=parameter.lower;
    assert.throws(()=>ftx.body_fit_starts(model,input,JSON.stringify(boundary)),/bound|inside|interior/);
    const duplicateFrame=structuredClone(edits);duplicateFrame.starts[0].poses[1].frame_index=duplicateFrame.starts[0].poses[0].frame_index;
    assert.throws(()=>ftx.body_fit_starts(model,input,JSON.stringify(duplicateFrame)),/duplicate/);
    assert.throws(()=>ftx.body_fit_starts(model,input+'\n',JSON.stringify(edits)),/different input or model bytes/);
    assert.throws(()=>ftx.body_fit_starts(model,input,JSON.stringify({...edits,model_sha256:'0'.repeat(64)})),/different input or model bytes/);
    assert.throws(()=>ftx.body_fit_starts(model,input,JSON.stringify({input_sha256:edits.input_sha256,starts:edits.starts})),/both input_sha256 and model_sha256/);
    const changed=Buffer.from(model);changed[changed.length-1]^=1;
    assert.throws(()=>ftx.body_fit_starts(changed,input,''),/SHA-256 mismatch/);
  });

test('compiled body-observation authoring verifies original pixels and composes native starts preview and fitting',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_BODY_FIT_PORTABLE_DIR},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_BODY_FIT_PORTABLE_DIR;
    const item=JSON.parse(fs.readFileSync(path.join(dir,'cases.json'),'utf8')).cases.find(c=>c.name==='capture-image');
    assert.ok(item?.image);const model=fs.readFileSync(path.join(dir,'model.ftxbody')),input=fs.readFileSync(path.join(dir,item.input),'utf8');
    const parse=text=>JSON.parse(text,(key,value,meta)=>key==='timestamp_us' && typeof value==='number'?meta.source:value);
    const hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    const view=JSON.parse(ftx.body_fit_observations(model,input,'')),original=parse(input);
    assert.equal(view.input,input);assert.equal(view.input_sha256,hash(input));assert.equal(view.model_sha256,hash(model));
    assert.equal(view.start_count,original.fit.starts.length);assert.equal(view.body_fitted,false);
    view.frames.forEach((frame,i)=>{assert.equal(frame.frame_index,i);assert.deepEqual(frame.keypoints,original.fit.frames[i].keypoints);
      assert.deepEqual(parse(frame.image_review).frame,original.fit.frames[i].frame);});
    const pixels=fs.readFileSync(path.join(dir,item.image)),image=new ftx.WebBodyFitImage(pixels,view.frames[0].image_review);
    try {
      const receipt=parse(image.report());assert.equal(receipt.review_kind,'OBSERVATION_REVIEW');
      assert.equal(receipt.orientation,'already_applied_stored_pixels');assert.equal(view.frames[0].rotation_cw_deg,90);
      assert.deepEqual(Buffer.from(image.preview()),fs.readFileSync(path.join(dir,item.image_preview)));
    } finally {image.free();}
    const imageWorker=await photoWorker({bodyImageClass:ftx.WebBodyFitImage});
    const checkedImage=await imageWorker.send({request:96,op:'review-body-fit-image',review:view.frames[0].image_review,image:Uint8Array.from(pixels).buffer});
    assert.equal(checkedImage.type,'result',checkedImage.message);assert.equal(JSON.parse(checkedImage.report).review_kind,'OBSERVATION_REVIEW');
    const p=await photoPage();p.e.bodyFitInput.files=[file(input)];p.e.bodyFitModel.files=[new Blob([model])];p.e.bodyFitInput.handlers.change();
    await p.e.bodyPointsLoad.onclick();let live=p.current();
    await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(view)});
    assert.equal(p.e.bodyPointsError.textContent,'');const point=view.frames[0].keypoints[0];
    chooseBodyPoint(p,point.name);pointField(p,'bodyPointsX',String(point.pixel_px[0]+0.125));
    await p.e.bodyPointsApply.onclick();live=p.current();const request=live.messages[0];
    const worker=await photoWorker({bodyObservations:ftx.body_fit_observations}),result=await worker.send({...request,model:Uint8Array.from(model).buffer});
    assert.equal(result.type,'result',result.message);await live.emit({...result,request:request.request});
    assert.equal(p.e.bodyPointsError.textContent,'');const observed=JSON.parse(result.report),observations=parse(observed.input);
    const withoutPoints=v=>({...v,fit:{...v.fit,frames:v.fit.frames.map(f=>({...f,keypoints:[]}))}});
    assert.deepEqual(withoutPoints(observations),withoutPoints(original));assert.equal(observations.fit.frames[0].keypoints[0].pixel_px[0],point.pixel_px[0]+0.125);
    assert.equal(await p.urls.get(p.e.bodyPointsDownloads.children[0].href).text(),observed.input);
    assert.throws(()=>ftx.body_fit_observations(model,observed.input,request.edits),/different input or model bytes/);
    await p.e.bodyStartsLoad.onclick();live=p.current();assert.equal(live.messages[0].input,observed.input);
    const starts=JSON.parse(ftx.body_fit_starts(model,live.messages[0].input,''));
    await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(starts)});
    const parameter=starts.shape[0],replacement=[0.25,0.5,0.75,0.125,0.875].map(t=>parameter.lower*(1-t)+parameter.upper*t)
      .find(value=>value>parameter.lower && value<parameter.upper && starts.starts.every(start=>start.shape[0]!==value));
    assert.ok(Number.isFinite(replacement));editBodyStart(p,'bodyStart-shape-0',String(replacement));
    await p.e.bodyStartsApply.onclick();live=p.current();assert.equal(JSON.parse(live.messages[0].edits).input_sha256,observed.input_sha256);
    const started=JSON.parse(ftx.body_fit_starts(model,live.messages[0].input,live.messages[0].edits));
    await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(started)});
    assert.equal(p.e.bodyStartsError.textContent,'');assert.deepEqual(parse(started.input).fit.frames,observations.fit.frames);
    await p.e.bodyFitStartRun.onclick();live=p.current();assert.equal(live.messages[0].input,started.input);
    const preview=parse(ftx.review_body_fit_start(model,started.input,'0','0'));
    assert.equal(preview.input_sha256,started.input_sha256);assert.equal(preview.shape_values[0],replacement);
    assert.deepEqual(preview.keypoints.find(p=>p.name===point.name).observed_px,observations.fit.frames[0].keypoints[0].pixel_px);
    await p.e.bodyFitRun.onclick();live=p.current();assert.equal(live.messages[0].input,started.input);
    const fitted=ftx.fit_body(model,started.input,'50','100000');assert.equal(JSON.parse(fitted).status,'ACCEPTED');
    await live.emit({type:'result',request:live.messages[0].request,report:fitted});
    assert.equal(await p.urls.get(p.e.bodyFitDownloads.children.find(a=>a.download==='body-fit-input.json').href).text(),started.input);
    await p.e.bodyPointsLoad.onclick();live=p.current();assert.equal(live.messages[0].input,started.input);
    const resumed=JSON.parse(ftx.body_fit_observations(model,started.input,''));
    await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(resumed)});
    chooseBodyPoint(p,point.name);pointField(p,'bodyPointsY',String(point.pixel_px[1]+0.0625));
    await p.e.bodyPointsApply.onclick();live=p.current();assert.equal(JSON.parse(live.messages[0].edits).input_sha256,started.input_sha256);
    const final=JSON.parse(ftx.body_fit_observations(model,live.messages[0].input,live.messages[0].edits));
    await live.emit({type:'result',request:live.messages[0].request,report:JSON.stringify(final)});
    assert.equal(p.e.bodyPointsError.textContent,'');assert.deepEqual(parse(final.input).fit.starts,parse(started.input).fit.starts);
    assert.equal(await new Blob([await p.e.bodyFitInput.files[0].arrayBuffer()]).text(),input);
  });

test('compiled still-window assessment matches complete original native reports and input refusals',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_STILL_FIXTURES},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_STILL_FIXTURES;
    const parse=text=>JSON.parse(text,(key,value,meta)=>['timestamp_us','start_us','end_us'].includes(key)?meta.source:value);
    for(const name of ['stationary','fit-motion','held-out-motion','weak','full-width']) {
      const actual=ftx.still_window(fs.readFileSync(path.join(dir,name+'-input.json'),'utf8'),name+'-input.json');
      assert.deepEqual(parse(actual),parse(fs.readFileSync(path.join(dir,name+'-native.json'),'utf8')),name+' complete original native still report');
      if(name==='full-width')assert.ok(actual.includes('"end_us":18446744073709551615'));
    }
    const input=fs.readFileSync(path.join(dir,'stationary-input.json'),'utf8');
    for(const source of ['{}','\ufeff'+input,' '.repeat(256*1024+1)])assert.throws(()=>ftx.still_window(source,'input.json'));
  });

test('compiled bundle adjustment matches native solutions, failures and exact iteration parsing',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_BUNDLE_FIXTURES},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_BUNDLE_FIXTURES;
    const parse=text=>JSON.parse(text,(key,value,meta)=>['timestamp_us','start_us','end_us','max_skew_us'].includes(key)?meta.source:value);
    for(const [name,iterations] of [['accepted','050'],['budget','1'],['held-out','50'],['degenerate','50'],['full-width','50']]) {
      const input=fs.readFileSync(path.join(dir,name+'-input.json'),'utf8');
      const actual=ftx.bundle_adjust(input,iterations),expected=fs.readFileSync(path.join(dir,name+'-native.json'),'utf8');
      assert.deepEqual(parse(actual),parse(expected),name+' complete original native bundle report');
      if(name==='full-width')assert.ok(actual.includes('"timestamp_us":18446744073709551615'));
    }
    const source=fs.readFileSync(path.join(dir,'accepted-input.json'),'utf8');
    for(const value of ['', '-1','0','101','1.5','50.0','NaN','4294967296','18446744073709551616'])assert.throws(()=>ftx.bundle_adjust(source,value),/iterations/);
    assert.throws(()=>ftx.bundle_adjust(' '.repeat(256*1024+1),'50'),/256 KiB/);
    assert.throws(()=>ftx.bundle_adjust('\ufeff'+source,'50'));
  });

test('compiled triangulation reproduces complete native reports and exact full-width frame identities',
  {skip:process.env.FTX_REAL_WASM!=='1'||!process.env.FTX_TRIANGULATION_FIXTURES},async()=>{
    const ftx=await compiledPhotoBindings(),dir=process.env.FTX_TRIANGULATION_FIXTURES;
    const parse=text=>JSON.parse(text,(key,value,meta)=>key==='timestamp_us'?meta.source:value);
    for(const [name,hasNoise] of [['plain',false],['noise',true],['unavailable',true],['weak',false],['held-out',false],['full-width',true]]) {
      const read=name=>fs.readFileSync(path.join(dir,name),'utf8'),input=read(name+'-input.json'),noise=hasNoise?read(name+'-noise.json'):'';
      const actual=ftx.triangulate(input,noise),expected=read(name+'-native.json');
      assert.deepEqual(parse(actual),parse(expected),name+' full original native report');
      if(name==='full-width')assert.ok(actual.includes('"timestamp_us":18446744073709551615'));
    }
    const input=fs.readFileSync(path.join(dir,'plain-input.json'),'utf8');
    assert.throws(()=>ftx.triangulate(input,'{}'));
    assert.throws(()=>ftx.triangulate(input,' '.repeat(256*1024+1)),/256 KiB/);
    assert.throws(()=>ftx.triangulate('\ufeff'+input,''));
  });

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

test('compiled WASM uses every printable dictionary for photo measurement and camera acquisition',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(),hash=bytes=>require('node:crypto').createHash('sha256').update(bytes).digest('hex');
    const names=JSON.parse(ftx.calibration_dictionaries());assert.equal(names.length,27);
    for(const name of names) {
      const board=new ftx.WebCalibrationSheet('letter',22,16,name);
      let photo,builder;
      try {
        const bytes=board.png(),source=board.sidecar(),sheet=JSON.parse(source),b=sheet.board;
        photo=ftx.WebPhoto.from_reference(bytes,source,'','{}');
        const report=JSON.parse(photo.report());assert.equal(report.ok,true,name);assert.equal(report.inliers,35,name);
        assert.equal(report.image_input.sha256,hash(bytes));
        assert.equal(report.image_input.charuco_detection.sheet_sha256,hash(source));
        assert.equal(report.image_input.charuco_detection.implementation.dictionary,name);
        const pixel=(x,y)=>[b.origin_px[0]-.5+x*b.size_px[0]/176,b.origin_px[1]-.5+y*b.size_px[1]/132];
        const measured=JSON.parse(photo.measure(JSON.stringify({across_back:[pixel(44,44),pixel(132,44)]}),'{}'));
        assert.ok(Math.abs(measured.records[0].value_mm-88)<.2,name);
        assert.equal(measured.records[0].provenance,'PHOTO_ESTIMATED');
        const manifest={schema:'ftx-camera-calibration-images/1',image_model:'pinhole_zero_skew',lens_id:'synthetic-board',
          pinhole_evidence:'Synthetic rendered board; no physical camera claim',reference_evidence:'Synthetic 22 mm squares',
          image_size_px:[2550,3300],rotation_cw_deg:0,held_out_corner_ids:[0,6,28,34],
          views:['a.png','b.png','c.png','check.png'].map((image,i)=>({image,frame:{stream:'synthetic',timestamp_us:i*100000},partition:i===3?'held_out':'fit'}))};
        builder=new ftx.WebCameraCalibration(JSON.stringify(manifest),source,'');
        const receipt=JSON.parse(builder.add_image('a.png',bytes));
        assert.equal(receipt.corners_detected,35,name);assert.equal(receipt.detector.dictionary,name);
        assert.throws(()=>builder.finish(),/complete|missing|acquir|views/i);
        const wrong=structuredClone(sheet);wrong.board.dictionary=name.startsWith('DICT_7X7_')?'DICT_5X5_50':'DICT_7X7_50';
        let rejected;
        try {rejected=ftx.WebPhoto.from_reference(bytes,JSON.stringify(wrong),'','{}');}
        catch(error) {assert.match(String(error),/ChArUco|marker|corner|scale/i);}
        if(rejected)try {
          assert.equal(JSON.parse(rejected.report()).ok,false,name);
          assert.throws(()=>rejected.measure(JSON.stringify({across_back:[pixel(44,44),pixel(132,44)]}),'{}'));
        } finally {rejected.free();}
      } finally {builder?.free();photo?.free();board.free();}
    }
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

// These values come from spec 02 A.5/D.5, not a report generated by this implementation.
function syntheticSectionSource(name,value,sigma,extra={}) {
  return {schema:'fabric-measurements/1',wearer:{label:'synthetic section test'},taken_on:null,
    records:[{name,value_mm:value,sigma_mm:sigma,provenance:'PHOTO_ESTIMATED',method:`synthetic ${name} view`}],
    fused:{},...extra};
}

test('compiled WASM photo sections reproduce the published girth and retain exact two-view sources through ordinary fusion',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(),hash=text=>require('node:crypto').createHash('sha256').update(text).digest('hex');
    const front=' {\r\n"schema":"fabric-measurements/1","wearer":{"label":"synthetic section test"},"taken_on":"2026-10-01",'+
      '"records":[{"name":"waist_width","value_mm":160,"sigma_mm":4,"provenance":"PHOTO_ESTIMATED","method":"synthetic front view"}],'+
      '"fused":{"waist_width":{"value_mm":9999,"sigma_mm":0.001}},"source_integer":18446744073709551615,"source_float":1.0,"source_zero":-0.0\r\n}\r\n';
    const side=' '+JSON.stringify(syntheticSectionSource('waist_depth',130,5))+'\n';
    const options=' { "level":"waist", "same_wearer":true, "clothing_mm":-0.0 }\n';
    const raw=ftx.photo_section(front,side,options),result=JSON.parse(raw);
    assert.equal(result.girth_mm,484.2);assert.equal(result.sigma_mm,14.6);
    assert.deepEqual(result.sigma_terms_mm,{width:6.99,depth:7.87,k:10.05});
    assert.equal(result.schema,'fabric-measurements/1');assert.equal(result.records.length,1);
    assert.deepEqual(result.record,result.records[0]);assert.equal(result.record.provenance,'PHOTO_ESTIMATED');
    assert.equal(result.section_inputs.width.value_mm,160,'Original records override forged cached fusion');
    assert.equal(result.section_inputs.depth.value_mm,130);assert.equal(result.taken_on,null);
    assert.deepEqual(result.section_sources,{width:{text:front,sha256:hash(front)},depth:{text:side,sha256:hash(side)},
      depth_uses_width_source:false,options:{text:options,sha256:hash(options)}});
    assert.equal(ftx.photo_section(front,side,options),raw,'Identical sources and assumptions replay exactly');
    const body={mode:'tape',stature_cm:92,supplemental_same_wearer:true,supplemental_measurements:[raw]};
    const fused=JSON.parse(ftx.measurement_file(JSON.stringify(body),'2026-11-01'));
    assert.equal(fused.measurement_imports[0],raw);assert.equal(fused.taken_on,null);
    assert.equal(fused.fused.waist.primary_provenance,'PHOTO_ESTIMATED');
    assert.equal(fused.fused.waist.sources.find(s=>s.provenance==='PHOTO_ESTIMATED' && s.used).sigma_mm,14.6);
    // A derived girth supplies one estimate, not the other body evidence needed to draft a garment.
    assert.throws(()=>ftx.run(JSON.stringify({program:'zouave_trousers',body:{mode:'file',measurements:result}}),
      '2026-11-01','2026-11-01T12:00'),/missing measurement|stature/i);

    const combined=syntheticSectionSource('waist_width',166,4,{taken_on:'2026-10-02'});
    combined.records.push({...syntheticSectionSource('waist_depth',136,5).records[0]});
    const combinedText=JSON.stringify(combined),corrected=JSON.parse(ftx.photo_section(combinedText,'','{"clothing_mm":3}'));
    assert.equal(corrected.girth_mm,484.2);assert.equal(corrected.sigma_mm,14.6);assert.equal(corrected.taken_on,'2026-10-02');
    assert.equal(corrected.section_sources.depth,null);assert.equal(corrected.section_sources.depth_uses_width_source,true);
    assert.match(corrected.notes,/clothing offset/);
  });

test('compiled WASM photo section calibration remains a coefficient report with explicit uncertainty',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings();
    const source=syntheticSectionSource('chest_width',172,0.001);
    source.records.push(syntheticSectionSource('chest_depth',150,0.001).records[0]);
    const text=JSON.stringify(source),options='{"level":"chest","tape_girth_mm":560,"sigma_tape_mm":10}';
    const calibrated=JSON.parse(ftx.photo_section(text,'',options));
    assert.equal(calibrated.k_child,1.1058);assert.equal(calibrated.k_child_sigma,0.0197);
    assert.equal(calibrated.k_ratio,1.0335);assert.equal(calibrated.k_ratio_sigma,0.0185);
    for(const field of ['schema','record','records','readiness'])assert.equal(calibrated[field],undefined,field);
    assert.equal(calibrated.section_sources.options.text,options);
    assert.throws(()=>ftx.measurement_file(JSON.stringify({mode:'file',measurements:calibrated}),'2026-11-01'),/schema|records/);
    const estimated=JSON.parse(ftx.photo_section(text,'',JSON.stringify({level:'chest',
      k_ratio:calibrated.k_ratio,k_ratio_sigma:calibrated.k_ratio_sigma})));
    assert.ok(Math.abs(estimated.girth_mm-560)<0.2);assert.ok(estimated.sigma_mm>10);
    assert.equal(estimated.record.provenance,'PHOTO_ESTIMATED');assert.match(estimated.notes,/UNCERTAIN/);
  });

test('compiled WASM photo sections refuse missing consent, fabricated provenance, conflicting evidence and unsupported settings',
  {skip:process.env.FTX_REAL_WASM!=='1'},async()=>{
    const ftx=await compiledPhotoBindings(),front=syntheticSectionSource('waist_width',160,4),side=syntheticSectionSource('waist_depth',130,5);
    const run=(a=front,b=side,options={same_wearer:true})=>ftx.photo_section(JSON.stringify(a),JSON.stringify(b),JSON.stringify(options));
    for(const same_wearer of [undefined,false])assert.throws(()=>run(front,side,{same_wearer}),/same.wearer/i);
    assert.throws(()=>run(front,{...side,wearer:{label:'different synthetic wearer'}}),/wearer/i);
    for(const reference of [true,'demo',1])assert.throws(()=>run({...front,wearer:{...front.wearer,reference}}),/reference/i);
    for(const provenance of ['MODEL_FIT','CHART_PRIOR','MEASURED_TAPE']) {
      const changed=structuredClone(front);changed.records[0].provenance=provenance;
      assert.throws(()=>run(changed),/photo evidence|PHOTO_ESTIMATED|provenance/i);
    }
    const mixed=structuredClone(front);mixed.records.push({...mixed.records[0],provenance:'MODEL_FIT',method:'synthetic model'});
    assert.throws(()=>run(mixed),/photo evidence|PHOTO_ESTIMATED|provenance/i);
    const conflict=structuredClone(front);conflict.records.push({...conflict.records[0],value_mm:260,method:'second synthetic view'});
    assert.throws(()=>run(conflict),/remeasure|conflicting/i);
    assert.throws(()=>run({...front,records:[]}),/waist_width|no .*record/i);
    for(const changed of [{unexpected:true},{level:null},{clothing_mm:null},{k_ratio:0},{sigma_tape_mm:10},
      {tape_girth_mm:500,k:1},{tape_girth_mm:500,k_ratio:1},{tape_girth_mm:500,name:'waist'}]) {
      assert.throws(()=>run(front,side,{same_wearer:true,...changed}));
    }
    assert.throws(()=>ftx.photo_section('\ufeff'+JSON.stringify(front),JSON.stringify(side),'{"same_wearer":true}'));
    assert.throws(()=>ftx.photo_section(JSON.stringify(front),JSON.stringify(side),' '.repeat(256*1024+1)),/budget|256 KiB|exceeds/i);
  });

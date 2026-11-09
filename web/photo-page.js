const $ = id => document.getElementById(id);
let worker = null, ticket = 0, busy = false, scale = null, bitmap = null, pending = [];
let sourceUrls = [], resultUrls = [], draftUrls = [];
let pointSource = null;
let importedOptions = null;
let photoRecords = null, workbenchTransfer = null;
let sectionWorker = null, sectionTicket = 0, sectionBusy = false, sectionRecords = null;
const sectionSources = {width:null,depth:null}, sectionUrls = [];
const materialsOrigin = new URL(import.meta.url).origin;
let materialsToken = new URL(window.location.href).searchParams.get("materialsReturn");
let materialsTarget = materialsToken && /^[0-9a-f]{32}$/.test(materialsToken) && materialsOrigin !== "null" ? window.opener : null;
$("returnMaterials").hidden = !materialsTarget;
$("sectionMaterials").hidden = !materialsTarget;
let cameraWorker = null, cameraTicket = 0, cameraBusy = false, cameraResult = null, generatedCalibration = null;
const cameraUrls = [];
let triangulationWorker = null, triangulationTicket = 0, triangulationBusy = false;
const triangulationUrls = [];
let bodyFitWorker = null, bodyFitTicket = 0, bodyFitBusy = false;
let bodyFitSources = null, bodyFitTransfer = null;
const bodyFitUrls = [];
let bodyStartsWorker = null, bodyStartsTicket = 0, bodyStartsBusy = false;
let bodyStartsSource = null, bodyStartsView = null, bodyStartsDraft = [], bodyStartsSelected = 0, bodyStartsPending = false;
const bodyStartsUrls = [];
let bodyStartsInputs = [];
let bodyReviewWorker = null, bodyReviewTicket = 0, bodyReviewBusy = false, bodyReviewValue = null, bodyReviewText = null;
let bodyReviewImageWorker = null, bodyReviewImageTicket = 0, bodyReviewImageBusy = false, bodyReviewBitmap = null;
const bodyReviewUrls = [];
const status = text => {$("status").textContent=text;};
function pointText() {
  const displayed=$("points").value;
  return pointSource?.displayed===displayed?pointSource.text:displayed;
}
function pointPairs(text=pointText()) {
  const pairs=JSON.parse(text);
  if(!pairs || typeof pairs!=="object" || Array.isArray(pairs)) throw Error("Points must be a JSON object of named pairs.");
  return pairs;
}
function checkedPointPairs(text) {
  if(typeof text!=="string" || new Blob([text]).size>256*1024) throw Error("Point JSON must be at most 256 KiB.");
  const pairs=pointPairs(text);
  for(const spec of Object.values(pairs)) {
    const pair=Array.isArray(spec)?spec:spec?.points;
    if(!Array.isArray(pair) || pair.length!==2 || pair.some(p=>!Array.isArray(p) || p.length!==2 || !p.every(Number.isFinite))) {
      throw Error("Point file must contain named pairs of finite [x, y] coordinates.");
    }
  }
  return pairs;
}
function checkedOptions(text) {
  if(typeof text!=="string" || new Blob([text]).size>256*1024) throw Error("Options JSON must be at most 256 KiB.");
  const value=JSON.parse(text), keys=["depth_offset_mm","camera_distance_mm","click_px","sheet_level_sigma_deg","bootstrap","seed"];
  if(!value || typeof value!=="object" || Array.isArray(value) || Object.keys(value).some(key=>!keys.includes(key))) {
    throw Error("Choose a photo-options.json object containing only measurement settings.");
  }
}
function controls() {
  const startsBlocked=bodyStartsBusy || bodyStartsPending;
  $("bodyFitRun").disabled=bodyFitBusy || startsBlocked || bodyStartsView?.starts.length<2 || !$("bodyFitModel").files.length || !$("bodyFitInput").files.length;
  $("bodyFitOpen").disabled=bodyFitBusy || startsBlocked || !$("bodyFitModel").files.length || !$("bodyFitSaved").files.length;
  $("bodyFitCancel").disabled=!bodyFitBusy;
  $("bodyFitWorkbench").disabled=bodyFitBusy || !bodyFitSources;
  $("bodyFitReviewRun").disabled=bodyReviewBusy || bodyFitBusy || !bodyFitSources;
  $("bodyFitReviewCancel").disabled=!bodyReviewBusy && !bodyReviewImageBusy;
  $("bodyFitReviewFrame").disabled=bodyFitBusy || !bodyFitSources;
  $("bodyFitStartRun").disabled=bodyReviewBusy || bodyFitBusy || startsBlocked || bodyStartsView?.starts.length===0 || !$("bodyFitModel").files.length || !$("bodyFitInput").files.length;
  $("bodyFitStartIndex").disabled=bodyFitBusy; $("bodyFitStartFrame").disabled=bodyFitBusy;
  $("bodyFitAcceptedReviewControls").hidden=!bodyFitSources;
  const reviewImageUnavailable=bodyFitBusy || bodyReviewBusy || !bodyReviewValue?.source_image;
  $("bodyFitReviewImage").disabled=reviewImageUnavailable;
  $("bodyFitReviewImageRun").disabled=reviewImageUnavailable || bodyReviewImageBusy || !$("bodyFitReviewImage").files.length;
  $("bodyFitReviewImageClear").disabled=!bodyReviewImageBusy && !bodyReviewBitmap && !$("bodyFitReviewImage").files.length;
  $("bodyStartsLoad").disabled=startsBlocked || bodyFitBusy || bodyStartsView!==null || !$("bodyFitModel").files.length || !$("bodyFitInput").files.length;
  $("bodyStartsEditor").hidden=bodyStartsView===null;
  const startsLocked=bodyStartsBusy || bodyFitBusy, hasStart=!!bodyStartsDraft[bodyStartsSelected];
  $("bodyStartsSelect").disabled=startsLocked || !bodyStartsDraft.length;
  for(const id of ["bodyStartsCopy","bodyStartsRemove","bodyStartsMidpoint"]) $(id).disabled=startsLocked || !hasStart;
  $("bodyStartsCopy").disabled ||= bodyStartsDraft.length>=4;
  $("bodyStartsAdd").disabled=startsLocked || !bodyStartsView || bodyStartsDraft.length>=4;
  $("bodyStartsApply").disabled=startsLocked || !bodyStartsPending || !bodyStartsDraft.length;
  $("bodyStartsCancel").disabled=!bodyStartsPending && !bodyStartsBusy;
  $("bodyStartsReset").disabled=!bodyStartsView && !bodyStartsBusy;
  for(const input of bodyStartsInputs) input.disabled=startsLocked;
  const operation=$("geometryOperation").value, bundle=operation==="bundle-adjust", still=operation==="still-window", triangulate=operation==="triangulate";
  $("geometryNoiseControls").hidden=!triangulate; $("geometryBundleControls").hidden=!bundle;
  $("geometryStillControls").hidden=!still;
  $("triangulationNoise").disabled=!triangulate; $("bundleIterations").disabled=!bundle;
  $("geometryInputKind").textContent=still?"ftx-still-window-input/1":bundle?"ftx-bundle-input/1":"ftx-triangulation-input/1";
  $("triangulationRun").textContent=still?"Assess still-window candidate":bundle?"Refine cameras and landmarks":"Triangulate landmark";
  $("triangulationRun").disabled=triangulationBusy || !$("triangulationInput").files.length;
  $("triangulationCancel").disabled=!triangulationBusy;
  $("triangulationClearNoise").disabled=!triangulate || !$("triangulationNoise").files.length;
  const charuco=$("referenceMode").value==="charuco", calibrated=charuco && ($("calibration").files.length>0 || generatedCalibration!==null);
  $("plainControls").hidden=charuco; $("charucoControls").hidden=!charuco; $("lensControls").hidden=!calibrated;
  $("paper").disabled=charuco; $("sheet").disabled=!charuco; $("calibration").disabled=!charuco;
  $("clearCalibration").disabled=!calibrated;
  $("analyze").disabled=busy || cameraBusy || sectionBusy || !$("photo").files.length || (charuco && !$("sheet").files.length);
  $("measure").disabled=busy || cameraBusy || sectionBusy || scale?.ok !== true || pending.length>0;
  $("openWorkbench").disabled=busy || cameraBusy || sectionBusy || photoRecords===null;
  $("returnMaterials").disabled=busy || cameraBusy || sectionBusy || photoRecords===null || !materialsTarget;
  $("pointFile").disabled=!bitmap || scale?.ok!==true;
  $("draftFile").disabled=cameraBusy || !bitmap || scale?.ok!==true;
  $("saveDraft").disabled=busy || cameraBusy || !bitmap || scale?.ok!==true || pending.length>0;
  $("optionsFile").disabled=cameraBusy;
  $("useFormOptions").disabled=importedOptions===null;
  for(const id of ["distance","depth","click","level","bootstrap"]) $(id).disabled=importedOptions!==null;
  $("optionsSource").textContent=importedOptions ?? "";
  $("optionsSource").hidden=importedOptions===null;
  $("optionsSummary").textContent=importedOptions===null?"Using the form values below (bootstrap seed 0).":
    "Using imported settings below; form values are inactive. Rust checks these settings when measuring.";
  let namedPair=false;
  try {namedPair=!!$("name").value.trim() && Object.hasOwn(pointPairs(),$("name").value.trim());} catch {}
  $("removePair").disabled=!namedPair; $("cancelPoint").disabled=pending.length===0;
  $("cancel").disabled=!busy;
  const images=$("cameraMode").value==="images";
  $("cameraImageControls").hidden=!images;
  $("cameraInputLabel").textContent=images?"Image manifest":"Reference-coordinate input";
  $("cameraRun").disabled=busy || cameraBusy || sectionBusy || !$("cameraInput").files.length ||
    (images && (!$("cameraSheet").files.length || (!$("cameraImages").files.length && !$("cameraDirectory").files.length)));
  $("cameraCancel").disabled=!cameraBusy;
  $("cameraClearNoise").disabled=!$("cameraNoise").files.length;
  $("cameraUse").disabled=busy || cameraBusy || cameraResult?.accepted!==true;
  $("cameraActive").textContent=generatedCalibration?`Using the locally generated calibration for ${generatedCalibration.lensId}.`:"";
  const combined=$("sectionCombined").checked, calibrating=$("sectionMode").value==="calibrate";
  $("sectionDepthControls").hidden=combined;
  $("sectionGirthControls").hidden=calibrating; $("sectionTapeControls").hidden=!calibrating;
  $("sectionRun").textContent=calibrating?"Calibrate section shape factor":"Estimate girth";
  $("sectionRun").disabled=busy || cameraBusy || sectionBusy || !sectionSources.width ||
    (!combined && !sectionSources.depth) || !$("sectionConsent").checked;
  $("sectionCancel").disabled=!sectionBusy;
  $("sectionKeepWidth").disabled=busy || cameraBusy || sectionBusy || !photoRecords;
  $("sectionKeepDepth").disabled=busy || cameraBusy || sectionBusy || !photoRecords;
  $("sectionClear").disabled=!sectionSources.width && !sectionSources.depth;
  $("sectionWorkbench").disabled=busy || cameraBusy || sectionBusy || !sectionRecords;
  $("sectionMaterials").disabled=busy || cameraBusy || sectionBusy || !sectionRecords || !materialsTarget;
  $("sectionWidthSource").textContent=sectionSources.width?`Retained front/combined source: ${sectionSources.width.label}`:"No front-width records selected.";
  $("sectionDepthSource").textContent=sectionSources.depth?`Retained side source: ${sectionSources.depth.label}`:"No side-depth records selected.";
}
function kill() { worker?.terminate(); worker=null; busy=false; controls(); }
function revoke(urls) { urls.forEach(u=>URL.revokeObjectURL(u)); urls.length=0; }
function withdraw(source=false) {
  photoRecords=null;
  if(workbenchTransfer && workbenchTransfer.kind!=="section") workbenchTransfer.records=null;
  ++ticket; kill(); $("pointFile").value=""; $("optionsFile").value=""; revoke(resultUrls); $("downloads").replaceChildren();
  $("draftFile").value=""; revoke(draftUrls); $("draftDownloads").replaceChildren();
  $("result").hidden=true; $("report").textContent=""; $("values").replaceChildren(); $("error").textContent="";
  if(source) {
    scale=null; bitmap?.close(); bitmap=null; pending=[]; pointSource=null; revoke(sourceUrls);
    $("sourceDownloads").replaceChildren(); $("preview").hidden=true; $("scale").textContent="";
    $("points").value="{}"; $("clickStatus").textContent="";
  }
  controls();
}
function link(parent,bytes,name,mime,urls) {
  const a=document.createElement("a"), u=URL.createObjectURL(new Blob([bytes],{type:mime}));
  urls.push(u); a.href=u; a.download=name; a.textContent=name; parent.append(a);
}
function draw() {
  if(!bitmap) return;
  const canvas=$("image"), c=canvas.getContext("2d"); c.drawImage(bitmap,0,0);
  const corners=scale.corners_px;
  if(Array.isArray(corners) && corners.length===4) {
    c.strokeStyle="#00cddd"; c.lineWidth=Math.max(2,canvas.width/400); c.beginPath();
    corners.forEach(([x,y],i)=>i?c.lineTo(x+.5,y+.5):c.moveTo(x+.5,y+.5)); c.closePath(); c.stroke();
  }
  const detected=scale.image_input?.charuco_detection?.reference_input?.corners || [], inliers=new Set(scale.inlier_ids || []);
  for(const {id,point_px:p} of detected) if(Array.isArray(p) && p.length===2 && p.every(Number.isFinite)) {
    c.strokeStyle=inliers.has(id)?"#00cddd":"#b66d00"; c.lineWidth=Math.max(2,canvas.width/600);
    c.beginPath(); c.arc(p[0]+.5,p[1]+.5,Math.max(4,canvas.width/250),0,2*Math.PI); c.stroke();
  }
  c.fillStyle="#dc3030";
  let pairs=[];
  try { pairs=Object.values(JSON.parse($("points").value)).flatMap(p=>Array.isArray(p)?p:(p.points||[])); } catch {}
  for(const p of [...pairs,...pending]) if(Array.isArray(p)&&p.length===2&&p.every(Number.isFinite)) {
    c.beginPath(); c.arc(p[0]+.5,p[1]+.5,Math.max(3,canvas.width/200),0,2*Math.PI); c.fill();
  }
}
function options() {
  if(importedOptions!==null) return importedOptions;
  return JSON.stringify({depth_offset_mm:Number($("depth").value),camera_distance_mm:Number($("distance").value),
    click_px:Number($("click").value),sheet_level_sigma_deg:Number($("level").value),bootstrap:Number($("bootstrap").value),seed:0});
}
async function readJsonFile(file) {
  if(!file) return "";
  // Preserve the exact UTF-8 source, including a BOM (which the Rust JSON parser will reject).
  // Replacing invalid bytes or stripping a BOM would change the source identity before hashing.
  return new TextDecoder("utf-8",{fatal:true,ignoreBOM:true}).decode(await file.arrayBuffer());
}
function bodyInputText(file) {
  // The editor receives complete input text from Rust. Never parse/rewrite its frame IDs in JS.
  if(bodyStartsView && file===bodyStartsSource?.inputFile && $("bodyFitModel").files[0]===bodyStartsSource.modelFile) {
    return Promise.resolve(bodyStartsView.input);
  }
  return readJsonFile(file);
}
function copyBodyStarts(starts) {
  return starts.map(start=>({shape:[...start.shape],poses:start.poses.map(pose=>({frame_index:pose.frame_index,values:[...pose.values]}))}));
}
function stopBodyStarts() {
  ++bodyStartsTicket; bodyStartsWorker?.terminate(); bodyStartsWorker=null; bodyStartsBusy=false;
}
function withdrawBodyStartsDownloads() {
  revoke(bodyStartsUrls); $("bodyStartsDownloads").replaceChildren();
}
function bodyStartsDownload() {
  withdrawBodyStartsDownloads();
  if(bodyStartsView && !bodyStartsPending && !bodyStartsBusy) {
    const name=bodyStartsView.input===bodyStartsSource.input?"body-fit-input-original.json":"body-fit-input-edited.json";
    link($("bodyStartsDownloads"),bodyStartsView.input,name,"application/json",bodyStartsUrls);
  }
}
function withdrawBodyStarts() {
  stopBodyStarts(); withdrawBodyStartsDownloads();
  bodyStartsSource=null; bodyStartsView=null; bodyStartsDraft=[]; bodyStartsInputs=[]; bodyStartsPending=false; bodyStartsSelected=0;
  $("bodyStartsFields").replaceChildren(); $("bodyStartsSelect").replaceChildren();
  $("bodyStartsError").textContent="";
  $("bodyStartsStatus").textContent="Load the selected prepared input to edit its starts.";
  controls();
}
function pendingBodyStarts() {
  stopBodyStarts();
  bodyStartsPending=true; withdrawBodyStartsDownloads(); withdrawBodyFit();
  $("bodyStartsError").textContent="";
  $("bodyStartsStatus").textContent="Pending starting values: check and apply all starts, or cancel the edits. Previous fit results and previews are withdrawn.";
  $("bodyFitStatus").textContent="Starting values changed; apply or cancel pending edits before previewing or fitting.";
  controls();
}
function renderBodyStarts() {
  const list=$("bodyStartsSelect"), fields=$("bodyStartsFields");
  list.replaceChildren(); fields.replaceChildren(); bodyStartsInputs=[];
  bodyStartsSelected=Math.min(bodyStartsSelected,Math.max(0,bodyStartsDraft.length-1));
  for(let i=0;i<bodyStartsDraft.length;i++) {
    const option=document.createElement("option"); option.value=String(i); option.textContent="Start "+(i+1); list.append(option);
  }
  list.value=String(bodyStartsSelected);
  const start=bodyStartsDraft[bodyStartsSelected];
  if(!start) {controls();return;}
  const group=(title,parameters,values,prefix)=>{
    const fieldset=document.createElement("fieldset"), legend=document.createElement("legend");
    legend.textContent=title; fieldset.append(legend);
    parameters.forEach((parameter,index)=>{
      const label=document.createElement("label"), input=document.createElement("input"), text=document.createElement("span");
      input.type="number"; input.step="any"; input.min=String(parameter.lower); input.max=String(parameter.upper);
      input.id=`bodyStart-${prefix}-${index}`; label.htmlFor=input.id;
      input.value=Object.is(values[index],-0)?"-0":String(values[index]);
      text.textContent=`${parameter.name} (${parameter.unit}); ${parameter.lower} < value < ${parameter.upper} `;
      input.addEventListener("input",()=>{
        if(bodyFitBusy) return;
        values[index]=input.value; pendingBodyStarts();
      });
      label.append(text,input); fieldset.append(label); bodyStartsInputs.push(input);
    });
    fields.append(fieldset);
  };
  group("Shared shape — same across every frame",bodyStartsView.shape,start.shape,"shape");
  for(const frame of bodyStartsView.frames) {
    group(frame.label,bodyStartsView.pose,start.poses.find(pose=>pose.frame_index===frame.frame_index).values,"pose-"+frame.frame_index);
  }
  controls();
}
function checkedBodyStartsView(text,input,edits) {
  const view=JSON.parse(text), hash=value=>typeof value==="string" && /^[0-9a-f]{64}$/.test(value);
  if(view.schema!=="ftx-body-starts-review/1" || typeof view.input!=="string" || new Blob([view.input]).size>16*1024*1024 ||
      !hash(view.input_sha256) || !hash(view.original_input_sha256) || !hash(view.model_sha256) ||
      !Array.isArray(view.shape) || !Array.isArray(view.pose) || !Array.isArray(view.frames) ||
      !view.frames.length || view.frames.length>8 || !Array.isArray(view.starts) || view.starts.length>4 ||
      (!edits && (view.input!==input || view.input_sha256!==view.original_input_sha256)) ||
      (edits && (!view.starts.length || view.original_input_sha256!==bodyStartsSource?.input_sha256 || view.model_sha256!==bodyStartsView?.model_sha256))) {
    throw Error("The engine did not return a source-bound initialization editor.");
  }
  for(const parameters of [view.shape,view.pose]) {
    if(parameters.length>128 || parameters.some(p=>typeof p.name!=="string" || typeof p.unit!=="string" ||
        !Number.isFinite(p.lower) || !Number.isFinite(p.upper) || p.lower>=p.upper)) {
      throw Error("The engine returned invalid initialization controls.");
    }
  }
  for(const [index,frame] of view.frames.entries()) {
    if(frame.frame_index!==index || typeof frame.label!=="string" || typeof frame.identity?.stream!=="string" ||
        typeof frame.identity.timestamp_us!=="string" || !/^(0|[1-9]\d*)$/.test(frame.identity.timestamp_us)) {
      throw Error("The engine returned invalid original frame identities.");
    }
  }
  const validVector=(values,parameters)=>Array.isArray(values) && values.length===parameters.length &&
    values.every((value,i)=>Number.isFinite(value) && value>parameters[i].lower && value<parameters[i].upper);
  for(const start of view.starts) {
    if(!validVector(start.shape,view.shape) || !Array.isArray(start.poses) || start.poses.length!==view.frames.length ||
        new Set(start.poses.map(p=>p.frame_index)).size!==view.frames.length ||
        start.poses.some(p=>!Number.isSafeInteger(p.frame_index) || p.frame_index<0 || p.frame_index>=view.frames.length || !validVector(p.values,view.pose))) {
      throw Error("The engine returned incomplete initialization values.");
    }
  }
  if(edits && (JSON.stringify(view.shape)!==JSON.stringify(bodyStartsView.shape) ||
      JSON.stringify(view.pose)!==JSON.stringify(bodyStartsView.pose) || JSON.stringify(view.frames)!==JSON.stringify(bodyStartsView.frames))) {
    throw Error("Initialization editing changed the original controls or frames.");
  }
  return view;
}
function bodyStartsEdits() {
  if(bodyStartsDraft.length<1 || bodyStartsDraft.length>4) throw Error("Supply 1–4 complete starts; fitting needs at least two.");
  const vector=(values,parameters)=>"["+values.map((raw,i)=>{
    const value=Number(raw), parameter=parameters[i];
    if(typeof raw==="string" && !raw.trim() || !Number.isFinite(value) || value<=parameter.lower || value>=parameter.upper) {
      throw Error(`Enter a finite ${parameter.name} strictly between ${parameter.lower} and ${parameter.upper}.`);
    }
    // These tokens contain only validated numbers; source JSON and full-width identities stay native.
    return Object.is(value,-0)?"-0.0":JSON.stringify(value);
  }).join(",")+"]";
  return '{"input_sha256":'+JSON.stringify(bodyStartsSource.input_sha256)+',"model_sha256":'+JSON.stringify(bodyStartsView.model_sha256)+',"starts":['+
    bodyStartsDraft.map(start=>'{"shape":'+vector(start.shape,bodyStartsView.shape)+',"poses":['+
    start.poses.map(pose=>'{"frame_index":'+pose.frame_index+',"values":'+vector(pose.values,bodyStartsView.pose)+'}').join(",")+"]}").join(",")+"]}";
}
async function runBodyStarts(apply=false) {
  if(bodyStartsBusy || bodyFitBusy || (!apply && (bodyStartsView || bodyStartsPending)) ||
      (apply && (!bodyStartsView || !bodyStartsPending))) return;
  const inputFile=$("bodyFitInput").files[0], modelFile=$("bodyFitModel").files[0];
  stopBodyStarts(); const id=bodyStartsTicket;
  $("bodyStartsError").textContent="";
  const fail=error=>{
    if(id!==bodyStartsTicket) return;
    stopBodyStarts(); $("bodyStartsError").textContent=String(error.message || error);
    $("bodyStartsStatus").textContent="Starting values were not applied. Correct them or cancel pending edits."; controls();
  };
  try {
    if(!inputFile || inputFile.size>16*1024*1024 || !modelFile || modelFile.size===0 || modelFile.size>512*1024*1024 ||
        (apply && (inputFile!==bodyStartsSource.inputFile || modelFile!==bodyStartsSource.modelFile))) {
      throw Error("Choose the prepared input up to 16 MiB and its exact converted model up to 512 MiB.");
    }
    const edits=apply?bodyStartsEdits():"";
    bodyStartsBusy=true; controls(); $("bodyStartsStatus").textContent="Reading the source and checking declared starting values…";
    const input=apply?bodyStartsSource.input:await readJsonFile(inputFile);
    if(id!==bodyStartsTicket) return;
    const model=await modelFile.arrayBuffer();
    if(id!==bodyStartsTicket) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); bodyStartsWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==bodyStartsWorker || m.request!==bodyStartsTicket) return;
      if(m.type==="progress") {$("bodyStartsStatus").textContent=m.text;return;}
      if(m.type==="error") {fail(m.message);return;}
      try {
        const view=checkedBodyStartsView(m.report,input,edits);
        if(apply) withdrawBodyFit();
        stopBodyStarts();
        bodyStartsSource ??= {input,inputFile,modelFile,input_sha256:view.original_input_sha256};
        bodyStartsView=view; bodyStartsDraft=copyBodyStarts(view.starts); bodyStartsPending=false;
        renderBodyStarts(); bodyStartsDownload();
        $("bodyStartsStatus").textContent=`${apply?"Applied":"Loaded"} ${view.starts.length} checked starts. `+
          (view.starts.length===0?"Add an explicitly authored start to preview it.":view.starts.length===1?"Preview this start; add a distinct second start before fitting.":"Preview the declared poses or fit from these exact values.");
        if(apply) $("bodyFitStatus").textContent="Checked starting values applied. Preview them or run the fit again.";
      } catch(error) {fail(error);}
    };
    current.onerror=error=>{if(current===bodyStartsWorker)fail(error);};
    current.postMessage({request:id,op:"body-fit-starts",model,input,edits},[model]);
  } catch(error) {fail(error);}
}
function stopBodyFit() {
  bodyFitWorker?.terminate(); bodyFitWorker=null; bodyFitBusy=false; controls();
}
function withdrawBodyFit() {
  bodyFitSources=null;
  if(bodyFitTransfer) bodyFitTransfer.sources=null;
  withdrawBodyReview(); $("bodyFitReview").hidden=true; $("bodyFitReviewFrame").replaceChildren();
  ++bodyFitTicket; stopBodyFit(); revoke(bodyFitUrls);
  $("bodyFitDownloads").replaceChildren(); $("bodyFitResult").hidden=true;
  $("bodyFitAttempts").replaceChildren(); $("bodyFitChecks").replaceChildren();
  for(const id of ["bodyFitError","bodyFitReport","bodyFitSummary","bodyFitUse","bodyFitPredictions","bodyFitFlags"]) $(id).textContent="";
}
function publishBodyFit(text,input,iterations,maxEvaluations,modelSnapshot) {
  // Only presentation parses JSON. Downloads retain the original Rust bytes, including full-u64
  // source identities; no model prediction is passed to the measurement-fusion handoff.
  const report=JSON.parse(text), accepted=report.status==="ACCEPTED", saved=input===null;
  if(report.schema!=="ftx-body-fit/1" || !Array.isArray(report.input?.frames) ||
      !Array.isArray(report.attempts) || !Array.isArray(report.flags) ||
      !Array.isArray(report.model?.measurements) || accepted!==(report.solution!=null) || (saved && !accepted)) {
    throw Error("The engine did not return a consistent body-fit report.");
  }
  const row=(parent,values)=>{
    const tr=document.createElement("tr");
    for(const value of values) {const td=document.createElement("td");td.textContent=String(value);tr.append(td);}
    parent.append(tr);
  };
  const flags=[...report.flags,...(report.advisories || [])];
  for(const [i,attempt] of report.attempts.entries()) {
    const costs=attempt.cost_history;
    row($("bodyFitAttempts"),[i+1,attempt.status,attempt.model_evaluations,
      costs.length?`${costs[0]} / ${costs.at(-1)}`:"Unavailable"]);
    for(const check of attempt.checks) row($("bodyFitChecks"),[i+1,check.frame?.stream ?? "Tape",
      check.name,check.partition,`${check.residual ?? "Unavailable"} / ${check.limit} ${check.unit}`,check.passed?"Pass":"Fail"]);
    for(const flag of attempt.flags) flags.push(`Start ${i+1}: ${flag}`);
  }
  $("bodyFitSummary").textContent=`${saved?"Saved report: ":""}${report.status}: ${report.input.frames.length} frames, ${report.attempts.length} starts, ${report.model_evaluations} model evaluations.`;
  $("bodyFitUse").textContent=accepted?
    "Accepted shape available. Continue in Pattern Workbench with this exact report and model, then choose garment and measurement evidence and confirm the wearer before body checks or draping. The downloads also work in either workbench.":
    "No accepted shape was published. Keep the diagnostic report and resolve the reported evidence or numerical failures before trying again.";
  $("bodyFitPredictions").textContent=accepted && report.solution.predicted_measurements_m.length?
    "MODEL_FIT predictions (mm):\n"+report.model.measurements.map((name,i)=>`${name}: ${report.solution.predicted_measurements_m[i]*1000}`).join("\n"):"";
  $("bodyFitFlags").textContent=[...flags,report.no_claim].filter(Boolean).join("\n");
  $("bodyFitReport").textContent=text;
  link($("bodyFitDownloads"),text,"body-fit-report.json","application/json",bodyFitUrls);
  if(!saved) {
    link($("bodyFitDownloads"),input,"body-fit-input.json","application/json",bodyFitUrls);
    link($("bodyFitDownloads"),JSON.stringify({iterations,max_model_evaluations:maxEvaluations}),"body-fit-options.json","application/json",bodyFitUrls);
  }
  $("bodyFitResult").hidden=false;
  $("bodyFitStatus").textContent=saved?"Saved accepted fit loaded; no fitting was run. Inspect the frames and saved residuals before reuse.":
    accepted?"Body fit accepted; review every start and held-out residual before reuse.":
      "Body fit finished without an accepted shape. Review all diagnostics.";
  bodyFitSources=accepted?{fit:new Blob([text],{type:"application/json"}),model:modelSnapshot}:null;
  $("bodyFitReview").hidden=!accepted;
  if(accepted) {
    $("bodyFitReviewHeading").textContent="Inspect an accepted frame";
    for(const [i,frame] of report.input.frames.entries()) {
      const option=document.createElement("option"); option.value=String(i);
      option.textContent="Frame "+(i+1)+": "+frame.frame.stream; $("bodyFitReviewFrame").append(option);
    }
    $("bodyFitReviewFrame").value="0";
    $("bodyFitReviewStatus").textContent="Choose a frame to inspect its full posed mesh and original observations without fitting again.";
  }
  controls();
}
function stopBodyReview() {
  bodyReviewWorker?.terminate(); bodyReviewWorker=null; bodyReviewBusy=false; controls();
}
function withdrawBodyReview() {
  ++bodyReviewTicket; bodyReviewValue=null; bodyReviewText=null; stopBodyReview(); withdrawBodyReviewImage(); revoke(bodyReviewUrls);
  $("bodyFitReviewCanvas").hidden=true; $("bodyFitReviewDownloads").replaceChildren();
  $("bodyFitStartDetails").hidden=true; $("bodyFitReviewChecks").replaceChildren();
  $("bodyFitReviewImageSource").textContent="Inspect a frame to check whether its original capture photograph is available.";
  for(const id of ["bodyFitReviewStatus","bodyFitReviewSummary","bodyFitReviewError","bodyFitReviewParameters","bodyFitReviewDiagnostics"]) $(id).textContent="";
}
function isStartReview() {return bodyReviewValue?.schema==="ftx-body-start-frame-review/1";}
function withdrawBodyReviewImage(clearFile=true) {
  ++bodyReviewImageTicket; bodyReviewImageWorker?.terminate(); bodyReviewImageWorker=null; bodyReviewImageBusy=false;
  bodyReviewBitmap?.close(); bodyReviewBitmap=null;
  if(clearFile) $("bodyFitReviewImage").value="";
  for(const id of ["bodyFitReviewImageStatus","bodyFitReviewImageError"]) $(id).textContent="";
  drawBodyReview(); controls();
}
function drawBodyReview() {
  if(!bodyReviewValue) return;
  const v=bodyReviewValue, canvas=$("bodyFitReviewCanvas"), [w,h]=v.image_size_px;
  const scale=Math.min(1,1600/Math.max(w,h));
  canvas.width=Math.max(1,Math.round(w*scale)); canvas.height=Math.max(1,Math.round(h*scale));
  const c=canvas.getContext("2d"), xy=p=>[p[0]*canvas.width/w,p[1]*canvas.height/h];
  const line=(a,b)=>{c.moveTo(...xy(a));c.lineTo(...xy(b));};
  c.fillStyle="white"; c.fillRect(0,0,canvas.width,canvas.height);
  if(bodyReviewBitmap) c.drawImage(bodyReviewBitmap,0,0,canvas.width,canvas.height);
  if($("bodyFitReviewMesh").checked) {
    c.strokeStyle="#b4bdc8"; c.fillStyle="#b4bdc8"; c.lineWidth=0.5; c.beginPath();
    if(v.projection_kind==="straight_edges") {
      for(const triangle of v.triangles) {
        const points=triangle.map(i=>v.projected_vertices_px[i]);
        if(points.every(p=>p!==null)) {line(points[0],points[1]);line(points[1],points[2]);line(points[2],points[0]);}
      }
      c.stroke();
    } else {
      for(const p of v.projected_vertices_px) if(p!==null) {const [x,y]=xy(p);c.fillRect(x,y,1,1);}
    }
  }
  for(const {observation:o,comparison:p} of v.silhouette?.lines || []) {
    const point=t=>o.axis==="horizontal"?[t,o.at_px]:[o.at_px,t];
    for(const [runs,color,width] of [[o.intervals_px,"#b45309",5],[p?.predicted_intervals_px || [],"#0369a1",2]]) {
      c.strokeStyle=color; c.lineWidth=width; c.beginPath();
      for(const [lo,hi] of runs) line(point(lo),point(hi)); c.stroke();
    }
    c.fillStyle="#111"; c.font="12px system-ui";
    c.fillText(o.partition==="held_out"?"CHECK":"FIT",...xy(point(0)));
  }
  for(const p of v.keypoints) {
    const a=xy(p.observed_px), b=p.predicted_px?xy(p.predicted_px):null;
    if(b) {c.lineWidth=1; c.strokeStyle="#666"; c.beginPath(); c.moveTo(...a); c.lineTo(...b); c.stroke();}
    c.strokeStyle="#b45309"; c.lineWidth=2; c.beginPath(); c.arc(...a,4,0,2*Math.PI); c.stroke();
    if(b) {c.strokeStyle="#0369a1"; c.beginPath(); c.moveTo(b[0]-4,b[1]); c.lineTo(b[0]+4,b[1]);
      c.moveTo(b[0],b[1]-4); c.lineTo(b[0],b[1]+4); c.stroke();}
    c.fillStyle="#111"; c.font="12px system-ui";
    const residual=Number.isFinite(p.residual_px)?p.residual_px.toFixed(3)+" px":"projection unavailable";
    c.fillText(p.name+" ("+(p.partition==="held_out"?"CHECK":"FIT")+", "+residual+")",a[0]+6,a[1]-6);
  }
  canvas.hidden=false;
}
function publishBodyReview(text,index,startIndex=null) {
  const v=JSON.parse(text), initial=startIndex!==null;
  if(v.schema!==(initial?"ftx-body-start-frame-review/1":"ftx-body-frame-review/1") || v.frame_index!==index || typeof v.frame_label!=="string" ||
      !(typeof v.mesh_obj==="string" || initial && v.mesh_obj===null) || !Array.isArray(v.image_size_px) || v.image_size_px.length!==2 ||
      v.image_size_px.some(n=>!Number.isSafeInteger(n) || n<=0) || !Array.isArray(v.keypoints) ||
      !Array.isArray(v.triangles) || !Array.isArray(v.projected_vertices_px) ||
      !["straight_edges","distorted_vertices"].includes(v.projection_kind) || initial &&
      (v.status!=="INITIALIZATION_REVIEW" || v.body_fitted!==false || v.start_index!==startIndex ||
       !Number.isSafeInteger(v.start_count) || v.start_count<1 || v.start_count>4 || startIndex>=v.start_count ||
       !Number.isSafeInteger(v.frame_count) || v.frame_count<2 || v.frame_count>8 || index>=v.frame_count ||
       !Array.isArray(v.shape_parameters) || !Array.isArray(v.shape_values) || v.shape_parameters.length!==v.shape_values.length ||
       !Array.isArray(v.pose_parameters) || !Array.isArray(v.pose_values) || v.pose_parameters.length!==v.pose_values.length ||
       !Array.isArray(v.diagnostics))) {
    throw Error("The engine did not return the requested body-frame review.");
  }
  bodyReviewValue=v; bodyReviewText=text; drawBodyReview();
  $("bodyFitReviewHeading").textContent=initial?"Declared initialization — no fitted body":"Inspect an accepted frame";
  $("bodyFitReviewImageSource").textContent=v.source_image?
    "Original capture: "+v.source_image.image_name+". Choose the original PNG or JPEG below to inspect it behind the projections.":
    "This frame has no retained original-image identity. Photograph review requires a capture-backed prepared input or saved capture report.";
  const name=(initial?"body-fit-start-"+String(startIndex).padStart(4,"0")+"-frame-":"body-fit-frame-")+String(index).padStart(4,"0");
  link($("bodyFitReviewDownloads"),text,name+".json","application/json",bodyReviewUrls);
  if(v.mesh_obj!==null) link($("bodyFitReviewDownloads"),v.mesh_obj,name+".obj","text/plain",bodyReviewUrls);
  $("bodyFitReviewStatus").textContent=initial?
    "INITIALIZATION_REVIEW: start "+(startIndex+1)+" of "+v.start_count+", frame "+(index+1)+" of "+v.frame_count+" ("+v.frame_label+"). No optimization or acceptance was performed.":
    v.frame_label+": full-mesh evidence rechecked without another fit.";
  $("bodyFitReviewSummary").textContent=v.projected_vertices_px.length+" vertices, "+v.triangles.length+" faces. "+
    v.unprojectable_vertices+" vertices cannot be projected in this camera and are omitted from the drawing. "+
    (v.mesh_obj===null?"No usable mesh was produced; review every diagnostic below. ":"The OBJ retains the full mesh. ")+
    (v.projection_kind==="distorted_vertices"?"Distorted camera: only projected vertices are drawn, because straight image edges would be misleading. ":"")+v.no_claim;
  if(initial) {
    $("bodyFitStartDetails").hidden=false;
    const values=(parameters,selected)=>parameters.map((p,i)=>p.name+": "+selected[i]+(p.unit?" "+p.unit:"")+" ("+p.lower+" < value < "+p.upper+")").join("\n");
    $("bodyFitReviewParameters").textContent="Shared shape\n"+values(v.shape_parameters,v.shape_values)+"\n\nFrame pose\n"+values(v.pose_parameters,v.pose_values);
    const row=values=>{
      const tr=document.createElement("tr");
      for(const value of values) {const td=document.createElement("td");td.textContent=String(value);tr.append(td);}
      $("bodyFitReviewChecks").append(tr);
    };
    for(const p of v.keypoints) row([p.name,p.partition==="held_out"?"CHECK":"FIT",
      Number.isFinite(p.residual_px)?p.residual_px+" px":"Unavailable",p.projection_refusal?JSON.stringify(p.projection_refusal):""]);
    for(const line of v.silhouette?.lines || []) row([line.observation.axis+" at "+line.observation.at_px+" px",
      line.observation.partition==="held_out"?"CHECK":"FIT",line.comparison?JSON.stringify(line.comparison):"Unavailable","Silhouette slice"]);
    $("bodyFitReviewDiagnostics").textContent=v.diagnostics.length?JSON.stringify(v.diagnostics,null,2):"No model or projection refusals reported. This is an initialization, not an accepted body.";
  }
  controls();
}
async function runBodyReviewImage() {
  if(bodyReviewImageBusy || bodyReviewBusy || bodyFitBusy || !bodyReviewText || !bodyReviewValue?.source_image) return;
  const file=$("bodyFitReviewImage").files[0], review=bodyReviewText, value=bodyReviewValue;
  withdrawBodyReviewImage(false); const id=bodyReviewImageTicket;
  const fail=error=>{
    if(id!==bodyReviewImageTicket || review!==bodyReviewText) return;
    withdrawBodyReviewImage(false); $("bodyFitReviewImageError").textContent=String(error.message || error);
    $("bodyFitReviewImageStatus").textContent=isStartReview()?"No photograph shown; the declared start and fit inputs are unchanged.":
      "No photograph shown; the frame review and accepted fit are unchanged.";
  };
  try {
    if(!file || file.size===0 || file.size>128*1024*1024 || new Blob([review]).size>16*1024*1024 ||
        value.image_size_px[0]*value.image_size_px[1]>40000000) {
      throw Error("Choose an original PNG or JPEG up to 128 MiB and 40 million pixels, with a frame review up to 16 MiB.");
    }
    bodyReviewImageBusy=true; controls(); $("bodyFitReviewImageStatus").textContent="Reading the selected original photograph…";
    const image=await file.arrayBuffer();
    if(id!==bodyReviewImageTicket || review!==bodyReviewText) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); bodyReviewImageWorker=current;
    current.onmessage=async({data:m})=>{
      if(current!==bodyReviewImageWorker || m.request!==bodyReviewImageTicket) return;
      if(m.type==="progress") {$("bodyFitReviewImageStatus").textContent=m.text;return;}
      if(m.type==="error") {fail(m.message);return;}
      current.terminate(); bodyReviewImageWorker=null;
      try {
        if(!(m.preview instanceof Uint8Array) || m.preview.byteLength===0) throw Error("The engine did not return a verified photograph preview.");
        const decoded=await createImageBitmap(new Blob([m.preview],{type:"image/png"}),{imageOrientation:"none"});
        if(id!==bodyReviewImageTicket || review!==bodyReviewText) {decoded.close();return;}
        const [w,h]=value.image_size_px;
        if(decoded.width!==w || decoded.height!==h) {decoded.close();throw Error("The verified photograph preview does not match the frame's stored pixel dimensions.");}
        bodyReviewBitmap=decoded; bodyReviewImageBusy=false; drawBodyReview(); controls();
        $("bodyFitReviewImageStatus").textContent="Verified original capture photograph: "+value.source_image.image_name+". "+
          w+" × "+h+" stored pixels; "+value.rotation_cw_deg+"° clockwise rotation already applied. SHA-256 "+
          value.source_image.image_sha256+(isStartReview()?". Visual inspection does not fit or accept a body.":". Visual inspection does not change the accepted fit.");
      } catch(error) {fail(error);}
    };
    current.onerror=error=>{if(current===bodyReviewImageWorker)fail(error);};
    current.postMessage({request:id,op:"review-body-fit-image",review,image},[image]);
  } catch(error) {fail(error);}
}
async function runBodyStartReview() {
  if(bodyReviewBusy || bodyFitBusy || bodyStartsBusy || bodyStartsPending || bodyStartsView?.starts.length===0) return;
  const inputFile=$("bodyFitInput").files[0], modelFile=$("bodyFitModel").files[0];
  const startText=$("bodyFitStartIndex").value, frameText=$("bodyFitStartFrame").value;
  const startIndex=Number(startText)-1, frameIndex=Number(frameText)-1;
  withdrawBodyReview(); const id=bodyReviewTicket;
  $("bodyFitReview").hidden=false; $("bodyFitReviewHeading").textContent="Declared initialization — no fitted body";
  const fail=error=>{
    if(id!==bodyReviewTicket) return;
    withdrawBodyReview(); $("bodyFitReviewError").textContent=String(error.message || error);
    $("bodyFitReviewStatus").textContent="Initialization preview refused; no fitting was run. Existing fit results are unchanged.";
  };
  try {
    if(!inputFile || inputFile.size>16*1024*1024 || !modelFile || modelFile.size===0 || modelFile.size>512*1024*1024 ||
        !startText.trim() || !Number.isSafeInteger(startIndex) || startIndex<0 || startIndex>3 ||
        !frameText.trim() || !Number.isSafeInteger(frameIndex) || frameIndex<0 || frameIndex>7) {
      throw Error("Choose prepared observations up to 16 MiB, a converted model up to 512 MiB, and a declared start (1–4) and frame (1–8).");
    }
    bodyReviewBusy=true; controls(); $("bodyFitReviewStatus").textContent="Reading the declared starts and exact model for a preview…";
    const input=await bodyInputText(inputFile);
    if(id!==bodyReviewTicket) return;
    const model=await modelFile.arrayBuffer();
    if(id!==bodyReviewTicket) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); bodyReviewWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==bodyReviewWorker || m.request!==bodyReviewTicket) return;
      if(m.type==="progress") {$("bodyFitReviewStatus").textContent=m.text;return;}
      if(m.type==="error") {fail(m.message);return;}
      stopBodyReview();
      try {publishBodyReview(m.report,frameIndex,startIndex);} catch(error) {fail(error);}
    };
    current.onerror=error=>{if(current===bodyReviewWorker)fail(error);};
    current.postMessage({request:id,op:"review-body-fit-start",model,input,startIndex,frameIndex},[model]);
  } catch(error) {fail(error);}
}
async function runBodyReview() {
  if(bodyReviewBusy || bodyFitBusy || !bodyFitSources) return;
  const source=bodyFitSources, index=Number($("bodyFitReviewFrame").value);
  withdrawBodyReview(); const id=bodyReviewTicket;
  $("bodyFitReviewHeading").textContent="Inspect an accepted frame";
  try {
    if(!$("bodyFitReviewFrame").value.trim() || !Number.isSafeInteger(index) || index<0 || index>7 || source.fit.size>16*1024*1024) {
      throw Error("Choose an accepted frame and a report no larger than 16 MiB.");
    }
    bodyReviewBusy=true; controls(); $("bodyFitReviewStatus").textContent="Reading the accepted report and exact evaluated model…";
    const report=await source.fit.text();
    if(id!==bodyReviewTicket || source!==bodyFitSources) return;
    const model=await source.model.arrayBuffer();
    if(id!==bodyReviewTicket || source!==bodyFitSources) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); bodyReviewWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==bodyReviewWorker || m.request!==bodyReviewTicket) return;
      if(m.type==="progress") {$("bodyFitReviewStatus").textContent=m.text;return;}
      stopBodyReview();
      if(m.type==="error") {$("bodyFitReviewError").textContent=m.message;$("bodyFitReviewStatus").textContent="Frame review refused; the saved fit report is unchanged.";return;}
      try {publishBodyReview(m.report,index);} catch(error) {
        withdrawBodyReview(); $("bodyFitReviewError").textContent=String(error.message || error);
        $("bodyFitReviewStatus").textContent="No frame review published; the saved fit report is unchanged.";
      }
    };
    current.onerror=error=>{
      if(current!==bodyReviewWorker) return;
      withdrawBodyReview(); $("bodyFitReviewError").textContent=error.message;
      $("bodyFitReviewStatus").textContent="Frame review stopped; the saved fit report is unchanged.";
    };
    current.postMessage({request:id,op:"review-body-fit-frame",model,report,frameIndex:index},[model]);
  } catch(error) {
    if(id!==bodyReviewTicket) return;
    withdrawBodyReview(); $("bodyFitReviewError").textContent=String(error.message || error);
  }
}
async function runBodyFit(saved=false) {
  if(bodyFitBusy || bodyStartsBusy || bodyStartsPending || (!saved && bodyStartsView?.starts.length<2)) return;
  const inputFile=$(saved?"bodyFitSaved":"bodyFitInput").files[0], modelFile=$("bodyFitModel").files[0];
  const iterationsText=$("bodyFitIterations").value, evaluationsText=$("bodyFitEvaluations").value;
  const iterations=Number(iterationsText), maxEvaluations=Number(evaluationsText);
  withdrawBodyFit(); const id=bodyFitTicket;
  try {
    if(!inputFile || inputFile.size>16*1024*1024 || !modelFile || modelFile.size===0 || modelFile.size>512*1024*1024) {
      throw Error("Choose "+(saved?"an original accepted fit report":"prepared observations")+" up to 16 MiB and a converted body model up to 512 MiB.");
    }
    if(!saved && (!iterationsText.trim() || !Number.isSafeInteger(iterations) || iterations<1 || iterations>100 ||
        !evaluationsText.trim() || !Number.isSafeInteger(maxEvaluations) || maxEvaluations<1 || maxEvaluations>1000000)) {
      throw Error("Enter 1..100 steps per start and 1..1000000 total model evaluations.");
    }
    bodyFitBusy=true; controls(); $("bodyFitStatus").textContent=saved?"Reading the saved fit and body model…":"Reading the local observations and body model…";
    const input=await (saved?readJsonFile(inputFile):bodyInputText(inputFile));
    if(id!==bodyFitTicket) return;
    const model=await modelFile.arrayBuffer();
    if(id!==bodyFitTicket) return;
    // Retain the evaluated bytes before transfer, independently of the original file and worker.
    const modelSnapshot=new Blob([model],{type:"application/octet-stream"});
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); bodyFitWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==bodyFitWorker || m.request!==bodyFitTicket) return;
      if(m.type==="progress") {$("bodyFitStatus").textContent=m.text; return;}
      stopBodyFit();
      if(m.type==="error") {$("bodyFitError").textContent=m.message; $("bodyFitStatus").textContent=saved?"Saved fit refused; no reusable body published.":"Body fit refused; no report published."; return;}
      try {
        publishBodyFit(saved?input:m.report,saved?null:input,iterations,maxEvaluations,modelSnapshot);
        if(saved) publishBodyReview(m.report,0);
      } catch(error) {
        withdrawBodyFit(); $("bodyFitError").textContent=String(error.message || error);
        $("bodyFitStatus").textContent="No body-fit report published.";
      }
    };
    current.onerror=error=>{
      if(current!==bodyFitWorker) return;
      withdrawBodyFit(); $("bodyFitError").textContent=error.message;
      $("bodyFitStatus").textContent=saved?"Saved-fit loading stopped; open the selected report again.":"Body-fit worker stopped; run the current input again.";
    };
    current.postMessage(saved?{request:id,op:"open-body-fit",report:input,model}:
      {request:id,op:"fit-body",input,model,iterations,maxEvaluations},[model]);
  } catch(error) {
    if(id!==bodyFitTicket) return;
    withdrawBodyFit(); $("bodyFitError").textContent=String(error.message || error);
    $("bodyFitStatus").textContent="No body-fit report published.";
  }
}
function stopTriangulation() {
  triangulationWorker?.terminate(); triangulationWorker=null; triangulationBusy=false; controls();
}
function withdrawTriangulation() {
  ++triangulationTicket; stopTriangulation(); revoke(triangulationUrls);
  $("triangulationDownloads").replaceChildren(); $("triangulationResult").hidden=true;
  for(const id of ["triangulationReport","triangulationSummary","triangulationPoint","triangulationPrecision","triangulationFlags","triangulationError"]) $(id).textContent="";
}
function publishTriangulation(text, input, noise, operation, iterations) {
  // Parsed values are display-only. Original input and complete native report text are downloaded
  // unchanged, so full-width frame timestamps never pass through JavaScript serialization.
  const report=JSON.parse(text), bundle=operation==="bundle-adjust", still=operation==="still-window", uncertainty=report.conditional_click_uncertainty;
  if(report.schema!==(still?"ftx-still-window/1":bundle?"ftx-bundle/1":"ftx-triangulation/1") || !Array.isArray(report.flags) ||
    !Array.isArray(bundle || still?report.checks:report.reprojection)) {
    throw Error("The engine did not return the requested geometry report.");
  }
  $("triangulationReport").textContent=text;
  if(still) {
    $("triangulationSummary").textContent=`${report.status}: ${report.input.frames.length} frames, ${report.fit_landmarks} FIT landmarks, ${report.held_out_landmarks} held-out landmarks; supplied motion limit ${report.input.max_motion_mm} mm.`;
    $("triangulationPoint").textContent=report.checks.map(check=>
      `${check.name} (${check.partition}): early/late displacement ${check.motion_mm===null?"unavailable":check.motion_mm+" mm"}; ${check.passed?"passes candidate checks":check.flags.join("; ")}`).join("\n");
    $("triangulationPrecision").textContent="Candidate only: human review must establish stillness of the relevant body region. Sparse tracks do not prove untracked anatomy stayed still; the supplied threshold is not a measurement accuracy bound.";
  } else if(bundle) {
    $("triangulationSummary").textContent=`${report.status}: ${report.input.views.length} cameras, ${report.input.landmarks.length} landmarks, ${report.iterations} accepted steps; requested limit ${iterations}.`;
    if(report.objective_history.length) $("triangulationSummary").textContent+=
      ` FIT normalized residual sum of squares: ${report.objective_history[0]} → ${report.objective_history.at(-1)}.`;
    $("triangulationPoint").textContent=report.solution?
      "Refined landmarks [m, z up]: "+report.solution.landmarks.map(point=>`${point.id}: [${point.position_m.join(", ")}]`).join("; "):
      "No optimized geometry was published. Input positions remain initial guesses.";
    $("triangulationPrecision").textContent="Pixel sigmas weight the fit; no camera, metric-scale or landmark covariance is estimated. Intrinsics and metric controls remain fixed.";
  } else {
    $("triangulationSummary").textContent=`${report.status}: ${report.fit_views} FIT views, ${report.held_out_views} held-out views.`;
    $("triangulationPoint").textContent=report.point_m?`World point [m, z up]: ${report.point_m.join(", ")}`:"No usable point was published.";
    $("triangulationPrecision").textContent=uncertainty?
    `Conditional click uncertainty: ${uncertainty.status}. `+(uncertainty.sigma_m?`One-sigma x, y, z [m]: ${uncertainty.sigma_m.join(", ")}`:"No covariance estimate is available."):
    "No click uncertainty was requested; a small pixel residual is not a measurement sigma.";
  }
  $("triangulationFlags").textContent=[...report.flags,...(report.advisories || []),...(uncertainty?.flags || []),report.no_claim,uncertainty?.no_claim].filter(Boolean).join(" ");
  const name=still?"still-window":bundle?"bundle":"triangulation";
  link($("triangulationDownloads"),text,name+".json","application/json",triangulationUrls);
  link($("triangulationDownloads"),input,name+"-input.json","application/json",triangulationUrls);
  if(noise) link($("triangulationDownloads"),noise,"triangulation-click-noise.json","application/json",triangulationUrls);
  if(bundle) link($("triangulationDownloads"),iterations,"bundle-iterations.txt","text/plain",triangulationUrls);
  $("triangulationResult").hidden=false;
  $("triangulationStatus").textContent=still?
    (report.status==="STILL_CANDIDATE"?"Still-window candidate only; human review is required before declaring capture evidence.":"Still-window checks did not pass. Review motion and geometry diagnostics; no stillness assertion was created."):
    bundle?
    (report.solution?"Anchored geometry accepted; review every held-out check and the local-solve assumptions.":"Refinement was not accepted. Diagnostics are available; no optimized geometry was published."):
    report.point_m?
    (uncertainty && uncertainty.status!=="AVAILABLE"?"Geometry accepted; requested click uncertainty is unavailable. Review diagnostics.":
      "Conditional geometry available; review calibration, capture and every held-out check."):
    "Geometry was not accepted. Diagnostics are available; no usable point was published.";
}
async function runTriangulation() {
  if(triangulationBusy) return;
  const operation=$("geometryOperation").value, bundle=operation==="bundle-adjust", still=operation==="still-window", iterations=bundle?$("bundleIterations").value:"";
  const inputFile=$("triangulationInput").files[0], noiseFile=operation==="triangulate"?$("triangulationNoise").files[0]:null;
  withdrawTriangulation(); const id=triangulationTicket;
  try {
    if(operation!=="triangulate" && !bundle && !still) throw Error("Choose a supported geometry operation.");
    if(!inputFile) throw Error("Choose calibrated landmark observations.");
    if(inputFile.size>256*1024 || noiseFile?.size>256*1024) throw Error("Each geometry file must be at most 256 KiB; no source was read.");
    triangulationBusy=true; controls(); $("triangulationStatus").textContent="Reading the local calibrated observations…";
    const [input,noise]=await Promise.all([readJsonFile(inputFile),readJsonFile(noiseFile)]);
    if(id!==triangulationTicket) return;
    if(noiseFile && noise.length===0) throw Error("The selected click-noise file is empty; supply evidence or remove the file explicitly.");
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); triangulationWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==triangulationWorker || m.request!==triangulationTicket) return;
      if(m.type==="progress") {$("triangulationStatus").textContent=m.text; return;}
      stopTriangulation();
      if(m.type==="error") {$("triangulationError").textContent=m.message; $("triangulationStatus").textContent="Geometry calculation failed; no result published."; return;}
      try {publishTriangulation(m.report,input,noise,operation,iterations);} catch(error) {
        withdrawTriangulation(); $("triangulationError").textContent=String(error.message || error);
        $("triangulationStatus").textContent="No geometry report published.";
      }
    };
    current.onerror=error=>{
      if(current!==triangulationWorker) return;
      withdrawTriangulation(); $("triangulationError").textContent=error.message;
      $("triangulationStatus").textContent="Geometry worker stopped; run the current observations again.";
    };
    current.postMessage({request:id,op:operation,input,noise,iterations,source:inputFile.name});
  } catch(error) {
    if(id!==triangulationTicket) return;
    stopTriangulation(); $("triangulationError").textContent=String(error.message || error);
    $("triangulationStatus").textContent="No geometry report published.";
  }
}
function stopSection() {sectionWorker?.terminate(); sectionWorker=null; sectionBusy=false; controls();}
function withdrawSection(source=false) {
  ++sectionTicket; sectionRecords=null;
  if(workbenchTransfer?.kind==="section") workbenchTransfer.records=null;
  if(source) $("sectionConsent").checked=false;
  stopSection(); revoke(sectionUrls); $("sectionDownloads").replaceChildren();
  $("sectionResult").hidden=true; $("sectionReport").textContent=""; $("sectionError").textContent="";
  $("sectionSummary").textContent=""; $("sectionUncertainty").textContent=""; $("sectionNotes").textContent="";
}
function keepSectionSource(side) {
  if(busy || cameraBusy || sectionBusy || !photoRecords) return;
  sectionSources[side]={file:photoRecords,label:`Measured snapshot of ${$("photo").files[0]?.name || "the current photo"}`};
  $(side==="width"?"sectionWidthFile":"sectionDepthFile").value="";
  withdrawSection(true);
  $("sectionStatus").textContent="Photo result retained. You may choose the next photo; review both sources before calculating.";
}
function sectionOptions() {
  const number=(id,optional=false)=>{
    const raw=$(id).value.trim();
    if(!raw && optional) return undefined;
    if(!raw || !Number.isFinite(Number(raw))) throw Error("Complete the active section settings with finite numbers.");
    return Number(raw);
  };
  const value={level:$("sectionLevel").value,same_wearer:$("sectionConsent").checked,clothing_mm:number("sectionClothing")};
  if($("sectionMode").value==="calibrate") {
    value.tape_girth_mm=number("sectionTape"); value.sigma_tape_mm=number("sectionTapeSigma");
  } else {
    value.k=number("sectionK",true); value.k_sigma=number("sectionKSigma",true);
    value.k_ratio=number("sectionRatio"); value.k_ratio_sigma=number("sectionRatioSigma");
  }
  return JSON.stringify(value);
}
async function runSection() {
  if(busy || cameraBusy || sectionBusy) return;
  withdrawSection(); const id=sectionTicket;
  const width=sectionSources.width?.file, depth=$("sectionCombined").checked?null:sectionSources.depth?.file;
  const calibrating=$("sectionMode").value==="calibrate";
  try {
    if(!width || (!$("sectionCombined").checked && !depth)) throw Error("Choose front-width and side-depth evidence first.");
    if(!$("sectionConsent").checked) throw Error("Review the sources and confirm the same wearer and matching anatomical levels.");
    if(width.size+(depth?.size || 0)>16*1024*1024) throw Error("Section source documents must total at most 16 MiB; no source was read.");
    const opts=sectionOptions();
    sectionBusy=true; controls(); $("sectionStatus").textContent="Reading the retained local photo evidence…";
    const [widthText,depthText]=await Promise.all([readJsonFile(width),readJsonFile(depth)]);
    if(id!==sectionTicket) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); sectionWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==sectionWorker || m.request!==sectionTicket) return;
      if(m.type==="progress") {$("sectionStatus").textContent=m.text; return;}
      stopSection();
      if(m.type==="error") {$("sectionError").textContent=m.message; $("sectionStatus").textContent="Section calculation refused; no result published."; return;}
      try {
        const result=JSON.parse(m.report);
        if(calibrating) {
          $("sectionSummary").textContent=`${result.level} calibration: k = ${result.k_child} ± ${result.k_child_sigma} (1σ).`;
          $("sectionUncertainty").textContent=`For this wearer's other levels: k ratio ${result.k_ratio}, ratio σ ${result.k_ratio_sigma}. Enter both under Shape-factor assumptions. Transferring this calibration between levels remains uncertain.`;
          $("sectionNotes").textContent="This coefficient report is not another independent tape or photo measurement. It cannot be sent as girth evidence.";
        } else {
          if(result.schema!=="fabric-measurements/1" || !Array.isArray(result.records) || result.records.length!==1) throw Error("The section engine returned no ordinary girth record.");
          $("sectionSummary").textContent=`${result.level}: ${result.girth_mm} mm ± ${result.sigma_mm} mm (1σ), PHOTO_ESTIMATED.`;
          const terms=result.sigma_terms_mm;
          $("sectionUncertainty").textContent=`Uncertainty contributions (mm): front width ${terms.width}, side depth ${terms.depth}, section shape ${terms.k}. Clothing offset: ${result.clothing_mm_per_side} mm per side.`;
          $("sectionNotes").textContent=result.notes;
          sectionRecords=new Blob([m.report],{type:"application/json"});
        }
        $("sectionReport").textContent=m.report; $("sectionResult").hidden=false;
        link($("sectionDownloads"),m.report,calibrating?"section-calibration.json":"section-measurements.json","application/json",sectionUrls);
        link($("sectionDownloads"),widthText,"section-front-measurements.json","application/json",sectionUrls);
        if(depth) link($("sectionDownloads"),depthText,"section-side-measurements.json","application/json",sectionUrls);
        link($("sectionDownloads"),opts,"section-options.json","application/json",sectionUrls);
        $("sectionStatus").textContent=calibrating?"Section calibration available; retain the coefficients and their uncertainty.":"Photo-estimated girth available; review its uncertainty and original evidence before use.";
      } catch(error) {
        withdrawSection(); $("sectionError").textContent=String(error.message || error);
        $("sectionStatus").textContent="No section result published.";
      }
      controls();
    };
    current.onerror=error=>{
      if(current!==sectionWorker) return;
      withdrawSection(); $("sectionError").textContent=error.message;
      $("sectionStatus").textContent="Section worker stopped; calculate again from the retained sources.";
    };
    current.postMessage({request:id,op:"section",width:widthText,depth:depthText,options:opts});
  } catch(error) {
    if(id!==sectionTicket) return;
    stopSection(); $("sectionError").textContent=String(error.message || error);
    $("sectionStatus").textContent="No section result published.";
  }
}
async function loadOptions() {
  const file=$("optionsFile").files[0];
  if(!file || cameraBusy) return;
  withdraw(); const id=ticket;
  if(file.size>256*1024) {
    $("error").textContent="Options JSON exceeds 256 KiB; no source was read.";
    status("Settings file not loaded; current settings are unchanged."); return;
  }
  busy=true; controls(); status("Reading local measurement settings…");
  try {
    const text=await readJsonFile(file);
    if(id!==ticket) return;
    checkedOptions(text);
    // Do not reserialize: a valid Rust u64 seed need not be representable as a JavaScript number.
    importedOptions=text;
    kill(); status("Settings loaded; review their assumptions for this photo, then measure again.");
  } catch(error) {
    if(id!==ticket) return;
    kill(); $("error").textContent=String(error.message || error);
    status("Settings file not loaded; current settings are unchanged.");
  }
}
async function loadPoints() {
  const file=$("pointFile").files[0];
  if(!file || !bitmap || scale?.ok!==true) return;
  pending=[]; withdraw(); const id=ticket; $("clickStatus").textContent="";
  if(file.size>256*1024) {
    $("error").textContent="Point JSON exceeds 256 KiB; no source was read.";
    status("Points file not loaded; existing pairs are unchanged."); return;
  }
  busy=true; controls(); status("Reading local point pairs…");
  try {
    const text=await readJsonFile(file);
    if(id!==ticket) return;
    checkedPointPairs(text);
    $("points").value=text;
    // Textareas normalize line endings; retain the original source until an actual point edit.
    pointSource={text,displayed:$("points").value};
    kill(); draw(); status("Points loaded; review them on this photo, then measure again.");
  } catch(error) {
    if(id!==ticket) return;
    kill(); $("error").textContent=String(error.message || error);
    status("Points file not loaded; existing pairs are unchanged.");
  }
}
function saveDraft() {
  if(busy || cameraBusy || !bitmap || scale?.ok!==true || pending.length) return;
  revoke(draftUrls); $("draftDownloads").replaceChildren();
  try {
    const points=pointText(), opts=options();
    checkedPointPairs(points); checkedOptions(opts);
    const text=JSON.stringify({schema:"ftx-photo-annotations/1",scale:$("scale").textContent,points,options:opts},null,2);
    if(new Blob([text]).size>32*1024*1024) throw Error("Annotation draft exceeds 32 MiB.");
    link($("draftDownloads"),text,"photo-annotations.json","application/json",draftUrls);
    status("Annotation draft ready to download; this saves work, not a measurement result.");
  } catch(error) {$("error").textContent=String(error.message || error); status("No annotation draft prepared.");}
}
async function loadDraft() {
  const file=$("draftFile").files[0];
  if(!file || cameraBusy || !bitmap || scale?.ok!==true) return;
  withdraw(); const id=ticket;
  if(file.size>32*1024*1024) {
    $("error").textContent="Annotation draft exceeds 32 MiB; no source was read.";
    status("Annotation draft not loaded; current work is unchanged."); return;
  }
  busy=true; controls(); status("Reading local annotation draft…");
  try {
    const text=await readJsonFile(file);
    if(id!==ticket) return;
    const draft=JSON.parse(text);
    if(!draft || draft.schema!=="ftx-photo-annotations/1" || Object.keys(draft).length!==4 ||
      ["scale","points","options"].some(key=>typeof draft[key]!=="string")) throw Error("Choose an ordinary photo-annotations.json draft.");
    if(draft.scale!==$("scale").textContent) throw Error("Draft belongs to a different analyzed photo, reference or calibration. Analyze its original sources first.");
    checkedPointPairs(draft.points); checkedOptions(draft.options);
    $("points").value=draft.points; pointSource={text:draft.points,displayed:$("points").value};
    importedOptions=draft.options; pending=[]; $("clickStatus").textContent="";
    kill(); draw(); status("Annotation draft loaded; review its points and settings, then measure again.");
  } catch(error) {
    if(id!==ticket) return;
    kill(); $("error").textContent=String(error.message || error);
    status("Annotation draft not loaded; current work is unchanged.");
  }
}
function stopCamera() { cameraWorker?.terminate(); cameraWorker=null; cameraBusy=false; controls(); }
function withdrawCamera() {
  ++cameraTicket; stopCamera();
  if(generatedCalibration!==null) {generatedCalibration=null; withdraw(true);}
  cameraResult=null; revoke(cameraUrls); $("cameraDownloads").replaceChildren();
  $("cameraResult").hidden=true; $("cameraReport").textContent=""; $("cameraSummary").textContent="";
  $("cameraChecks").replaceChildren(); $("cameraProgress").textContent=""; $("cameraError").textContent="";
  controls();
}
function publishCamera(text, sources) {
  const report=JSON.parse(text);
  if(report.schema!=="ftx-camera-calibration/1" || !Array.isArray(report.flags) || !Array.isArray(report.checks)) {
    throw Error("The calibration engine did not return an ordinary retained camera report.");
  }
  const accepted=report.status==="ACCEPTED" && report.solution!=null && report.flags.length===0 &&
    (report.conditional_uncertainty===undefined || report.conditional_uncertainty.estimate!=null);
  cameraResult={text,accepted,lensId:report.input?.lens_id};
  $("cameraReport").textContent=text; $("cameraResult").hidden=false;
  const uncertainty=report.conditional_uncertainty?` Conditional intrinsic uncertainty: ${report.conditional_uncertainty.status}.`:
    report.uncertainty?" Conditional intrinsic covariance retained.":" No intrinsic uncertainty was estimated.";
  $("cameraSummary").textContent=`${report.status}: ${report.input?.lens_id || "unnamed lens"}, ${report.input?.image_model || "unknown model"}.`+
    uncertainty+" "+[...report.flags,...(report.advisories || []),...(report.conditional_uncertainty?.flags || [])].join(" ");
  for(const check of report.checks) {
    const corners=check.corners || [], fit=corners.filter(c=>c.partition==="fit").length;
    const residuals=corners.map(c=>c.error_px).filter(Number.isFinite), row=document.createElement("tr");
    for(const value of [`${check.frame?.stream} @ ${check.frame?.timestamp_us} μs`,check.partition,
      `${fit} / ${corners.length-fit}`,residuals.length?Math.max(...residuals):"unavailable",check.failure || "—"]) {
      const cell=document.createElement("td"); cell.textContent=String(value); row.append(cell);
    }
    $("cameraChecks").append(row);
  }
  link($("cameraDownloads"),text,"camera-calibration.json","application/json",cameraUrls);
  link($("cameraDownloads"),sources.input,sources.mode==="images"?"camera-image-manifest.json":"camera-coordinate-input.json","application/json",cameraUrls);
  if(sources.mode==="images") {
    link($("cameraDownloads"),sources.sheet,"camera-sheet.json","application/json",cameraUrls);
    link($("cameraDownloads"),JSON.stringify(report.input,null,2),"camera-detected-coordinates.json","application/json",cameraUrls);
  }
  if(sources.noise) link($("cameraDownloads"),sources.noise,"camera-corner-noise.json","application/json",cameraUrls);
  $("cameraStatus").textContent=accepted?"Calibration accepted. Review every FIT/CHECK result, then use it for the measurement photo.":
    "Calibration or its requested uncertainty was not accepted. Diagnostics are available; no measurement calibration was activated.";
}
async function runCamera() {
  if(busy || cameraBusy) return;
  const mode=$("cameraMode").value, inputFile=$("cameraInput").files[0], noiseFile=$("cameraNoise").files[0];
  const sheetFile=mode==="images"?$("cameraSheet").files[0]:null;
  const direct=Array.from($("cameraImages").files), directory=Array.from($("cameraDirectory").files);
  withdrawCamera(); const id=cameraTicket;
  try {
    if(!inputFile || (mode==="images" && !sheetFile)) throw Error("Choose the original calibration input and, for images, its sheet description.");
    if(inputFile.size>16*1024*1024 || noiseFile?.size>16*1024*1024 || sheetFile?.size>256*1024) {
      throw Error("Calibration input/noise must be at most 16 MiB and sheet JSON at most 256 KiB. No source was read.");
    }
    let images=[];
    if(mode==="images") {
      if(direct.length && directory.length) throw Error("Select calibration photos or their directory, not both. Clear the other selection first.");
      const selected=directory.length?directory.filter(f=>/\.(png|jpe?g)$/i.test(f.name)):direct;
      if(selected.length<1 || selected.length>32) throw Error("Choose 1–32 declared calibration photos; no image was read.");
      images=selected.map(file=>{
        if(file.size>64*1024*1024) throw Error(`Calibration image ${file.name} exceeds 64 MiB; no image was read.`);
        const name=directory.length?file.webkitRelativePath.slice(file.webkitRelativePath.indexOf("/")+1):file.name;
        if(!name) throw Error("A selected image has no relative filename.");
        return {id:name,file};
      });
    }
    cameraBusy=true; controls(); $("cameraStatus").textContent="Reading the local calibration declarations…";
    const [input,sheet,noise]=await Promise.all([readJsonFile(inputFile),readJsonFile(sheetFile),readJsonFile(noiseFile)]);
    if(id!==cameraTicket) return;
    const sources={mode,input,sheet,noise}, current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); cameraWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==cameraWorker || m.request!==cameraTicket) return;
      if(m.type==="progress") {$("cameraStatus").textContent=m.text; return;}
      if(m.type==="calibration-view") {
        $("cameraProgress").textContent+=`${m.done}/${m.total} ${m.image}\n${JSON.stringify(m.diagnostics,null,2)}\n`;
        return;
      }
      stopCamera();
      if(m.type==="error") {$("cameraError").textContent=m.message; $("cameraStatus").textContent="Calibration failed; no measurement calibration was activated."; return;}
      try {publishCamera(m.report,sources);} catch(e) {withdrawCamera(); $("cameraError").textContent=String(e.message || e); $("cameraStatus").textContent="No calibration published.";}
      controls();
    };
    current.onerror=e=>{
      if(current!==cameraWorker) return;
      withdrawCamera(); $("cameraError").textContent=e.message; $("cameraStatus").textContent="Calibration worker stopped; run the current acquisition again.";
    };
    current.postMessage({request:id,op:mode==="images"?"calibrate-images":"calibrate-coordinates",input,sheet,noise,images});
  } catch(e) {
    if(id!==cameraTicket) return;
    stopCamera(); $("cameraError").textContent=String(e.message || e); $("cameraStatus").textContent="No calibration published.";
  }
}
async function run(op) {
  if(cameraBusy || (op==="measure" && pending.length>0)) return;
  const file=$("photo").files[0], points=pointText(), opts=options(), paper=$("paper").value;
  const referenceMode=$("referenceMode").value, charuco=referenceMode==="charuco";
  const sheetFile=charuco?$("sheet").files[0]:null, calibrationFile=charuco?$("calibration").files[0]:null;
  const generated=charuco?generatedCalibration:null;
  const captureFields={};
  if(calibrationFile || generated) {
    captureFields.capture_lens_id=$("lensId").value;
    if($("rotation").value!=="") captureFields.rotation_cw_deg=Number($("rotation").value);
    if($("independence").value!=="") captureFields.independent_calibration_evidence=$("independence").value;
  }
  const capture=JSON.stringify(captureFields);
  withdraw(op==="analyze");
  const id=ticket;
  if(!file) return;
  if(file.size>64*1024*1024) {$("error").textContent="Photo exceeds 64 MiB; no image was read."; return;}
  if(charuco && !sheetFile) {$("error").textContent="Choose the original ChArUco sheet description."; return;}
  if(sheetFile?.size>256*1024 || calibrationFile?.size>16*1024*1024) {
    $("error").textContent="Sheet JSON must be at most 256 KiB; retained calibration at most 16 MiB. No source was read."; return;
  }
  busy=true; controls(); status("Reading the local photo…");
  try {
    const [bytes,sheet,calibration]=await Promise.all([file.arrayBuffer(),readJsonFile(sheetFile),calibrationFile?readJsonFile(calibrationFile):Promise.resolve(generated?.text || "")]);
    if(id!==ticket) return;
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); worker=current;
    current.onmessage=async ({data:m})=>{
      if(current!==worker || m.request!==ticket) return;
      if(m.type==="progress") {status(m.text); return;}
      kill();
      if(m.type==="error") {$("error").textContent=m.message; status("No measurements published."); return;}
      try {
        if(op==="analyze") {
          const preview=new Blob([m.preview],{type:"image/png"}), decoded=await createImageBitmap(preview);
          if(id!==ticket) {decoded.close(); return;}
          bitmap=decoded; scale=m.scale; $("image").width=bitmap.width; $("image").height=bitmap.height;
          $("preview").hidden=false; $("scale").textContent=m.scaleText;
          const reference=scale.reference==="charuco" ? `ChArUco: ${scale.inliers ?? 0}/${scale.corners_detected ?? 0} accepted corners`
            : scale.paper;
          const lens=scale.lens_input ? (scale.intrinsic_uncertainty ? "Verified conditional lens uncertainty included. " : "Supplied lens applied; intrinsic uncertainty is not estimated. ") : "";
          $("summary").textContent=scale.ok ? `${reference}: ${scale.mm_per_px} mm/px. ${lens}${(scale.warnings||[]).join(" ")}`
            : `Reference rejected. ${(scale.warnings||[]).join(" ")}`;
          link($("sourceDownloads"),m.scaleText,"photo-scale.json","application/json",sourceUrls);
          link($("sourceDownloads"),m.preview,"oriented-photo.png","image/png",sourceUrls); draw();
          if(charuco) {
            link($("sourceDownloads"),sheet,"photo-sheet.json","application/json",sourceUrls);
            link($("sourceDownloads"),capture,"photo-capture.json","application/json",sourceUrls);
            if(calibration) link($("sourceDownloads"),calibration,"photo-calibration.json","application/json",sourceUrls);
          }
          status(scale.ok?"Review the detected sheet, then choose point pairs.":"Reference rejected; no measurement calibration available.");
        } else {
          const result=JSON.parse(m.report); $("report").textContent=m.report; $("result").hidden=false;
          for(const r of result.records) {
            const row=document.createElement("tr");
            for(const value of [r.name,r.value_mm ?? r.value_deg,r.sigma_mm ?? r.sigma_deg,r.value_deg===undefined?"mm":"deg"]) {
              const td=document.createElement("td"); td.textContent=String(value); row.append(td);
            }
            $("values").append(row);
          }
          link($("downloads"),m.report,"photo-measurements.json","application/json",resultUrls);
          photoRecords=new Blob([m.report],{type:"application/json"});
          link($("downloads"),points,"photo-points.json","application/json",resultUrls);
          link($("downloads"),opts,"photo-options.json","application/json",resultUrls);
          status("Photo-estimated records available; review uncertainty before use.");
        }
      } catch(e) {if(id!==ticket)return; withdraw(); $("error").textContent=String(e.message||e); status("No measurements published.");}
      controls();
    };
    current.onerror=e=>{if(worker!==current)return; withdraw(); $("error").textContent=e.message; status("Worker stopped; analyze again.");};
    current.postMessage({request:id,op,bytes,paper,referenceMode,sheet,calibration,capture,points,options:opts},[bytes]);
  } catch(e) {if(id!==ticket)return; withdraw(); $("error").textContent=String(e.message||e); status("No measurements published.");}
}
$("analyze").onclick=()=>run("analyze"); $("measure").onclick=()=>run("measure");
$("pointFile").addEventListener("change",loadPoints);
$("draftFile").addEventListener("change",loadDraft); $("saveDraft").onclick=saveDraft;
$("optionsFile").addEventListener("change",loadOptions);
$("useFormOptions").onclick=()=>{importedOptions=null; withdraw(); status("Using form settings; review their assumptions and measure again.");};
$("cancel").onclick=()=>{withdraw(); status("Cancelled; no measurements published.");};
for(const id of ["photo","paper","referenceMode","sheet","calibration","rotation"]) $(id).addEventListener("change",()=>{if(id==="calibration")generatedCalibration=null; withdraw(true); status("Source changed; analyze the current photo.");});
for(const id of ["lensId","independence"]) $(id).addEventListener("input",()=>{withdraw(true); status("Capture evidence changed; analyze the current photo.");});
$("clearCalibration").onclick=()=>{generatedCalibration=null; $("calibration").value=""; $("lensId").value=""; $("rotation").value=""; $("independence").value=""; withdraw(true); status("Calibration removed; analyze the current photo.");};
$("cameraRun").onclick=runCamera;
$("bodyFitRun").onclick=()=>runBodyFit();
$("bodyFitOpen").onclick=()=>runBodyFit(true);
$("bodyFitReviewRun").onclick=runBodyReview;
$("bodyFitStartRun").onclick=runBodyStartReview;
$("bodyStartsLoad").onclick=()=>runBodyStarts();
$("bodyStartsApply").onclick=()=>runBodyStarts(true);
$("bodyStartsSelect").addEventListener("change",()=>{
  if(bodyStartsBusy || bodyFitBusy) return;
  const index=Number($("bodyStartsSelect").value);
  if(!Number.isSafeInteger(index) || !bodyStartsDraft[index]) return;
  bodyStartsSelected=index; renderBodyStarts();
});
$("bodyStartsAdd").onclick=()=>{
  if(!bodyStartsView || bodyStartsBusy || bodyFitBusy || bodyStartsDraft.length>=4) return;
  bodyStartsDraft.push({shape:bodyStartsView.shape.map(()=>""),
    poses:bodyStartsView.frames.map(frame=>({frame_index:frame.frame_index,values:bodyStartsView.pose.map(()=>"")}))});
  bodyStartsSelected=bodyStartsDraft.length-1; pendingBodyStarts(); renderBodyStarts();
};
$("bodyStartsCopy").onclick=()=>{
  if(bodyStartsBusy || bodyFitBusy || !bodyStartsDraft[bodyStartsSelected] || bodyStartsDraft.length>=4) return;
  bodyStartsDraft.push(copyBodyStarts([bodyStartsDraft[bodyStartsSelected]])[0]);
  bodyStartsSelected=bodyStartsDraft.length-1; pendingBodyStarts(); renderBodyStarts();
};
$("bodyStartsRemove").onclick=()=>{
  if(bodyStartsBusy || bodyFitBusy || !bodyStartsDraft[bodyStartsSelected]) return;
  bodyStartsDraft.splice(bodyStartsSelected,1); pendingBodyStarts(); renderBodyStarts();
};
$("bodyStartsMidpoint").onclick=()=>{
  if(bodyStartsBusy || bodyFitBusy || !bodyStartsDraft[bodyStartsSelected]) return;
  const middle=parameters=>parameters.map(p=>p.lower/2+p.upper/2);
  bodyStartsDraft[bodyStartsSelected]={shape:middle(bodyStartsView.shape),
    poses:bodyStartsView.frames.map(frame=>({frame_index:frame.frame_index,values:middle(bodyStartsView.pose)}))};
  pendingBodyStarts(); renderBodyStarts();
};
$("bodyStartsCancel").onclick=()=>{
  stopBodyStarts(); bodyStartsPending=false;
  bodyStartsDraft=bodyStartsView?copyBodyStarts(bodyStartsView.starts):[];
  $("bodyStartsError").textContent=""; renderBodyStarts(); bodyStartsDownload();
  $("bodyStartsStatus").textContent="Loading or pending edits cancelled; the last checked input is unchanged.";
  $("bodyFitStatus").textContent="Pending start edits cancelled. Preview or fit the last checked input again.";
};
$("bodyStartsReset").onclick=()=>{
  withdrawBodyStarts(); withdrawBodyFit();
  $("bodyStartsStatus").textContent="Original selected input restored; all edited values and previous results withdrawn.";
  $("bodyFitStatus").textContent="Original selected input restored. Preview or fit its declared starts again.";
};
for(const id of ["bodyFitStartIndex","bodyFitStartFrame"]) $(id).addEventListener("input",()=>{
  withdrawBodyReview(); $("bodyFitReviewHeading").textContent="Declared initialization — no fitted body";
  $("bodyFitReviewStatus").textContent="Initialization selection changed; preview the chosen declared start and frame.";
});
$("bodyFitReviewCancel").onclick=()=>{withdrawBodyReview();$("bodyFitReviewStatus").textContent="Frame review cancelled; existing fit results are unchanged.";};
$("bodyFitReviewFrame").addEventListener("change",()=>{withdrawBodyReview();$("bodyFitReviewHeading").textContent="Inspect an accepted frame";$("bodyFitReviewStatus").textContent="Frame changed; inspect the selected accepted pose.";});
$("bodyFitReviewMesh").addEventListener("change",drawBodyReview);
$("bodyFitReviewImage").addEventListener("change",()=>{
  withdrawBodyReviewImage(false);
  if($("bodyFitReviewImage").files.length) $("bodyFitReviewImageStatus").textContent="Original photograph selected; verify it before displaying.";
});
$("bodyFitReviewImageRun").onclick=runBodyReviewImage;
$("bodyFitReviewImageClear").onclick=()=>{withdrawBodyReviewImage();$("bodyFitReviewImageStatus").textContent=isStartReview()?
  "Photograph cleared; the declared start and fit inputs are unchanged.":"Photograph cleared; the frame review and accepted fit are unchanged.";};
$("bodyFitWorkbench").onclick=()=>{
  if(bodyFitBusy || !bodyFitSources) return;
  const notify=text=>{$("bodyFitStatus").textContent=text;};
  if(new URL(import.meta.url).origin==="null") {
    notify("Open the local HTTP workbench to transfer this body, or use the original downloads.");return;
  }
  if(bodyFitSources.fit.size>16*1024*1024) {
    notify("This report exceeds the workbench's 16 MiB limit. Keep the original diagnostic download.");return;
  }
  if(bodyFitTransfer && !bodyFitTransfer.target.closed) {
    notify("A body handoff is still opening. Finish or close that Workbench tab first.");return;
  }
  const token=Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,"0")).join("");
  const url=new URL("index.html",import.meta.url);url.searchParams.set("bodyFitImport",token);
  const target=window.open(url.href,"_blank");
  if(!target) {notify("Workbench popup blocked. Download the fit report and choose it with the same model there.");return;}
  bodyFitTransfer={target,token,sources:bodyFitSources};
  notify("Workbench opened with the accepted body. Choose garment and measurement evidence there, then confirm the wearer.");
};
$("bodyFitCancel").onclick=()=>{withdrawBodyFit(); $("bodyFitStatus").textContent="Body fit or saved-fit loading cancelled; no report published.";};
for(const id of ["bodyFitInput","bodyFitModel","bodyFitSaved"]) for(const event of ["change","click","cancel"]) {
  $(id).addEventListener(event,()=>{
    if(id!=="bodyFitSaved") {
      if(event==="change") withdrawBodyStarts();
      else if(bodyStartsView) {
        pendingBodyStarts();
        $("bodyStartsStatus").textContent="Starting values retained while choosing a source. Apply them or cancel pending edits before previewing or fitting.";
      } else stopBodyStarts();
    }
    withdrawBodyFit(); $("bodyFitStatus").textContent=bodyStartsPending?
      "Starting values retained; apply or cancel pending edits before previewing or fitting.":
      "Review the selected model, then fit the observations or open the saved accepted report.";
  });
}
for(const id of ["bodyFitIterations","bodyFitEvaluations"]) $(id).addEventListener("input",()=>{
  withdrawBodyFit(); $("bodyFitStatus").textContent="Work limit changed; run the current observations again.";
});
$("triangulationRun").onclick=runTriangulation;
$("geometryOperation").addEventListener("change",()=>{
  if($("geometryOperation").value!=="triangulate") $("triangulationNoise").value="";
  withdrawTriangulation(); $("triangulationStatus").textContent="Geometry operation changed; select the matching input and run again.";
});
$("bundleIterations").addEventListener("input",()=>{
  withdrawTriangulation(); $("triangulationStatus").textContent="Refinement limit changed; run the current input again.";
});
$("triangulationCancel").onclick=()=>{withdrawTriangulation(); $("triangulationStatus").textContent="Geometry calculation cancelled; no result published.";};
$("triangulationClearNoise").onclick=()=>{
  $("triangulationNoise").value=""; withdrawTriangulation();
  $("triangulationStatus").textContent="Click-noise evidence removed; run the current observations again.";
};
for(const id of ["triangulationInput","triangulationNoise"]) $(id).addEventListener("change",()=>{
  withdrawTriangulation(); $("triangulationStatus").textContent="Triangulation source changed; run the current observations again.";
});
$("cameraCancel").onclick=()=>{withdrawCamera(); $("cameraStatus").textContent="Calibration cancelled; no result published.";};
$("cameraClearNoise").onclick=()=>{$("cameraNoise").value=""; withdrawCamera(); $("cameraStatus").textContent="Separate noise declaration removed; calibrate from the current input.";};
$("cameraUse").onclick=()=>{
  if(cameraResult?.accepted!==true || busy || cameraBusy) return;
  generatedCalibration=cameraResult; $("calibration").value=""; $("referenceMode").value="charuco";
  withdraw(true); status("Generated calibration selected. Assert this photo's capture settings and analyze it.");
};
$("sectionRun").onclick=runSection;
$("sectionCancel").onclick=()=>{withdrawSection(); $("sectionStatus").textContent="Section calculation cancelled; retained sources remain available.";};
$("sectionKeepWidth").onclick=()=>keepSectionSource("width");
$("sectionKeepDepth").onclick=()=>keepSectionSource("depth");
$("sectionClear").onclick=()=>{
  sectionSources.width=null; sectionSources.depth=null;
  $("sectionWidthFile").value=""; $("sectionDepthFile").value="";
  withdrawSection(true); $("sectionStatus").textContent="Retained section sources cleared.";
};
for(const [id,side] of [["sectionWidthFile","width"],["sectionDepthFile","depth"]]) {
  $(id).addEventListener("change",()=>{
    const file=$(id).files[0]; sectionSources[side]=file?{file,label:file.name}:null;
    withdrawSection(true); $("sectionStatus").textContent="Section source changed. Review both views and confirm the wearer again.";
  });
}
for(const id of ["sectionCombined","sectionLevel","sectionMode","sectionConsent"]) {
  $(id).addEventListener("change",()=>{
    withdrawSection(id==="sectionCombined");
    $("sectionStatus").textContent="Section settings changed; review the sources and calculate again.";
  });
}
for(const id of ["sectionClothing","sectionK","sectionKSigma","sectionRatio","sectionRatioSigma","sectionTape","sectionTapeSigma"]) {
  $(id).addEventListener("input",()=>{withdrawSection(); $("sectionStatus").textContent="Section assumptions changed; calculate again.";});
}
for(const id of ["cameraMode","cameraInput","cameraSheet","cameraImages","cameraDirectory","cameraNoise"]) {
  $(id).addEventListener("change",()=>{
    if(id==="cameraImages" && $(id).files.length) $("cameraDirectory").value="";
    if(id==="cameraDirectory" && $(id).files.length) $("cameraImages").value="";
    withdrawCamera(); $("cameraStatus").textContent="Acquisition changed; calibrate from the current declarations and images.";
  });
}
for(const id of ["distance","depth","click","level","bootstrap","points"]) $(id).addEventListener("input",()=>{
  if(id==="points") {pointSource=null; pending=[]; $("clickStatus").textContent="";}
  withdraw(); draw(); status("Inputs changed; measure again.");
});
$("name").addEventListener("input",()=>{pending=[]; $("clickStatus").textContent=""; withdraw(); draw(); status("Review the named pair, then measure again.");});
$("clearPoints").onclick=()=>{pointSource=null; $("points").value="{}"; pending=[]; withdraw(); draw(); $("clickStatus").textContent="All points cleared."; status("All points cleared; choose landmarks for a new pair.");};
$("cancelPoint").onclick=()=>{
  if(!pending.length) return;
  pending=[]; withdraw(); draw(); $("clickStatus").textContent="First click cancelled. Saved pairs are unchanged.";
  status("First click cancelled; review saved pairs and measure again.");
};
$("removePair").onclick=()=>{
  const name=$("name").value.trim();
  if(!name) return;
  try {
    const pairs=pointPairs();
    if(!Object.hasOwn(pairs,name)) return;
    delete pairs[name]; pointSource=null; $("points").value=JSON.stringify(pairs,null,2); pending=[];
    withdraw(); draw(); $("clickStatus").textContent=`Removed ${name}. Other point pairs are unchanged.`;
    status("Point pair removed; review the remaining pairs and measure again.");
  } catch(error) {$("error").textContent=error.message;}
};
$("image").onclick=e=>{
  if(!bitmap || scale?.ok!==true || busy) return;
  const name=$("name").value.trim(), box=$("image").getBoundingClientRect();
  if(!name) {$("error").textContent="Choose a measurement name first."; return;}
  const point=[(e.clientX-box.left)*bitmap.width/box.width-.5,(e.clientY-box.top)*bitmap.height/box.height-.5];
  if(point.some((v,i)=>v<0||v>[bitmap.width,bitmap.height][i]-1)) return;
  withdraw(); pending.push(point);
  if(pending.length===2) {
    try {
      const pairs=pointPairs();
      pointSource=null;
      $("points").value=JSON.stringify(Object.fromEntries([...Object.entries(pairs),[name,pending]]),null,2); pending=[];
    } catch(error) {pending=[]; $("error").textContent=error.message;}
  }
  $("clickStatus").textContent=pending.length?"First point selected; click the second.":"Pair added. Review its name and coordinates.";
  status(pending.length?"First point selected; click the second.":"Point pairs changed; measure again."); draw(); controls();
};
function openWorkbench(records,kind,notify,filename) {
  if(busy || cameraBusy || sectionBusy || !records) return;
  if(workbenchTransfer && !workbenchTransfer.target.closed) {
    notify("A Workbench handoff is still opening. Finish or close that tab before opening another.");return;
  }
  const token=Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,"0")).join("");
  const url=new URL("index.html",import.meta.url);url.searchParams.set("photoImport",token);
  const target=window.open(url.href,"_blank");
  if(!target) {notify(`Workbench popup blocked. Download ${filename} and add it there instead.`);return;}
  workbenchTransfer={target,token,records,kind};
  notify("Workbench opened. Choose primary measurements and confirm the same wearer before checking.");
}
function returnMaterials(records,kind,notify) {
  if(busy || cameraBusy || sectionBusy || !records || !materialsTarget) return;
  if(materialsTarget.closed) {notify("Material Workbench closed. Download the records instead.");return;}
  if(workbenchTransfer && !workbenchTransfer.target.closed) {notify("Finish the pending Workbench handoff first.");return;}
  workbenchTransfer={target:materialsTarget,token:materialsToken,records,kind};
  materialsTarget.postMessage({type:"ftx-photo-records-offer",token:materialsToken},materialsOrigin);
  notify("Returning current photo records to Material Workbench…");
}
const sectionStatus=text=>{$("sectionStatus").textContent=text;};
$("openWorkbench").onclick=()=>openWorkbench(photoRecords,"photo",status,"photo-measurements.json");
$("sectionWorkbench").onclick=()=>openWorkbench(sectionRecords,"section",sectionStatus,"section-measurements.json");
$("returnMaterials").onclick=()=>returnMaterials(photoRecords,"photo",status);
$("sectionMaterials").onclick=()=>returnMaterials(sectionRecords,"section",sectionStatus);
window.addEventListener("message",event=>{
  const m=event.data;
  const bodyTransfer=bodyFitTransfer;
  if(bodyTransfer && event.origin===new URL(import.meta.url).origin && event.source===bodyTransfer.target &&
      m?.type==="ftx-body-fit-sources-ready" && m.token===bodyTransfer.token) {
    bodyTransfer.target.postMessage(bodyTransfer.sources?
      {type:"ftx-body-fit-sources",token:bodyTransfer.token,...bodyTransfer.sources}:
      {type:"ftx-body-fit-sources-cancelled",token:bodyTransfer.token},event.origin);
    bodyFitTransfer=null;return;
  }
  if(materialsTarget && event.origin===materialsOrigin && event.source===materialsTarget && m?.token===materialsToken &&
    ["ftx-materials-photo-cancelled","ftx-materials-photo-received"].includes(m.type)) {
    if(workbenchTransfer?.target===materialsTarget) workbenchTransfer=null;
    materialsTarget=null;materialsToken=null;window.opener=null;controls();
    status(m.type==="ftx-materials-photo-received" ? "Photo records received by Material Workbench. Review them there and confirm the same wearer." :
      "Material Workbench cancelled this return. Reopen the photo tool there or download the current records.");return;
  }
  const transfer=workbenchTransfer,origin=new URL(import.meta.url).origin;
  if(!transfer || event.origin!==origin || event.source!==transfer.target ||
    m?.type!=="ftx-photo-records-ready" || m.token!==transfer.token) return;
  transfer.target.postMessage(transfer.records ? {type:"ftx-photo-records",token:transfer.token,records:transfer.records} :
    {type:"ftx-photo-records-cancelled",token:transfer.token},origin);
  workbenchTransfer=null;
});
window.addEventListener("pagehide",()=>{
  if(materialsTarget) {
    materialsTarget.postMessage({type:"ftx-photo-records-cancelled",token:materialsToken},materialsOrigin);
    materialsTarget=null;materialsToken=null;window.opener=null;
  }
  withdrawBodyStarts(); withdrawBodyFit(); withdrawTriangulation(); withdrawSection(true); withdrawCamera(); withdraw(true);
});
controls();

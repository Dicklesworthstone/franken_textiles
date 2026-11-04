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
  $("triangulationRun").disabled=triangulationBusy || !$("triangulationInput").files.length;
  $("triangulationCancel").disabled=!triangulationBusy;
  $("triangulationClearNoise").disabled=!$("triangulationNoise").files.length;
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
function stopTriangulation() {
  triangulationWorker?.terminate(); triangulationWorker=null; triangulationBusy=false; controls();
}
function withdrawTriangulation() {
  ++triangulationTicket; stopTriangulation(); revoke(triangulationUrls);
  $("triangulationDownloads").replaceChildren(); $("triangulationResult").hidden=true;
  for(const id of ["triangulationReport","triangulationSummary","triangulationPoint","triangulationPrecision","triangulationFlags","triangulationError"]) $(id).textContent="";
}
function publishTriangulation(text, input, noise) {
  // Parsed values are display-only. Original input and complete native report text are downloaded
  // unchanged, so full-width frame timestamps never pass through JavaScript serialization.
  const report=JSON.parse(text), uncertainty=report.conditional_click_uncertainty;
  if(report.schema!=="ftx-triangulation/1" || !Array.isArray(report.flags) || !Array.isArray(report.reprojection)) {
    throw Error("The engine did not return an ordinary triangulation report.");
  }
  $("triangulationReport").textContent=text;
  $("triangulationSummary").textContent=`${report.status}: ${report.fit_views} FIT views, ${report.held_out_views} held-out views.`;
  $("triangulationPoint").textContent=report.point_m?`World point [m, z up]: ${report.point_m.join(", ")}`:"No usable point was published.";
  $("triangulationPrecision").textContent=uncertainty?
    `Conditional click uncertainty: ${uncertainty.status}. `+(uncertainty.sigma_m?`One-sigma x, y, z [m]: ${uncertainty.sigma_m.join(", ")}`:"No covariance estimate is available."):
    "No click uncertainty was requested; a small pixel residual is not a measurement sigma.";
  $("triangulationFlags").textContent=[...report.flags,...(report.advisories || []),...(uncertainty?.flags || []),report.no_claim,uncertainty?.no_claim].filter(Boolean).join(" ");
  link($("triangulationDownloads"),text,"triangulation.json","application/json",triangulationUrls);
  link($("triangulationDownloads"),input,"triangulation-input.json","application/json",triangulationUrls);
  if(noise) link($("triangulationDownloads"),noise,"triangulation-click-noise.json","application/json",triangulationUrls);
  $("triangulationResult").hidden=false;
  $("triangulationStatus").textContent=report.point_m?
    (uncertainty && uncertainty.status!=="AVAILABLE"?"Geometry accepted; requested click uncertainty is unavailable. Review diagnostics.":
      "Conditional geometry available; review calibration, capture and every held-out check."):
    "Geometry was not accepted. Diagnostics are available; no usable point was published.";
}
async function runTriangulation() {
  if(triangulationBusy) return;
  const inputFile=$("triangulationInput").files[0], noiseFile=$("triangulationNoise").files[0];
  withdrawTriangulation(); const id=triangulationTicket;
  try {
    if(!inputFile) throw Error("Choose calibrated landmark observations.");
    if(inputFile.size>256*1024 || noiseFile?.size>256*1024) throw Error("Each triangulation file must be at most 256 KiB; no source was read.");
    triangulationBusy=true; controls(); $("triangulationStatus").textContent="Reading the local calibrated observations…";
    const [input,noise]=await Promise.all([readJsonFile(inputFile),readJsonFile(noiseFile)]);
    if(id!==triangulationTicket) return;
    if(noiseFile && noise.length===0) throw Error("The selected click-noise file is empty; supply evidence or remove the file explicitly.");
    const current=new Worker(new URL("photo-worker.js",import.meta.url),{type:"module"}); triangulationWorker=current;
    current.onmessage=({data:m})=>{
      if(current!==triangulationWorker || m.request!==triangulationTicket) return;
      if(m.type==="progress") {$("triangulationStatus").textContent=m.text; return;}
      stopTriangulation();
      if(m.type==="error") {$("triangulationError").textContent=m.message; $("triangulationStatus").textContent="Triangulation failed; no result published."; return;}
      try {publishTriangulation(m.report,input,noise);} catch(error) {
        withdrawTriangulation(); $("triangulationError").textContent=String(error.message || error);
        $("triangulationStatus").textContent="No triangulation report published.";
      }
    };
    current.onerror=error=>{
      if(current!==triangulationWorker) return;
      withdrawTriangulation(); $("triangulationError").textContent=error.message;
      $("triangulationStatus").textContent="Triangulation worker stopped; run the current observations again.";
    };
    current.postMessage({request:id,op:"triangulate",input,noise});
  } catch(error) {
    if(id!==triangulationTicket) return;
    stopTriangulation(); $("triangulationError").textContent=String(error.message || error);
    $("triangulationStatus").textContent="No triangulation report published.";
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
$("triangulationRun").onclick=runTriangulation;
$("triangulationCancel").onclick=()=>{withdrawTriangulation(); $("triangulationStatus").textContent="Triangulation cancelled; no result published.";};
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
  withdrawTriangulation(); withdrawSection(true); withdrawCamera(); withdraw(true);
});
controls();

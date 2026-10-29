const $ = id => document.getElementById(id);
let worker = null, ticket = 0, busy = false, scale = null, bitmap = null, pending = [];
let sourceUrls = [], resultUrls = [], draftUrls = [];
let pointSource = null;
let importedOptions = null;
let photoRecords = null, workbenchTransfer = null;
let cameraWorker = null, cameraTicket = 0, cameraBusy = false, cameraResult = null, generatedCalibration = null;
const cameraUrls = [];
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
  const charuco=$("referenceMode").value==="charuco", calibrated=charuco && ($("calibration").files.length>0 || generatedCalibration!==null);
  $("plainControls").hidden=charuco; $("charucoControls").hidden=!charuco; $("lensControls").hidden=!calibrated;
  $("paper").disabled=charuco; $("sheet").disabled=!charuco; $("calibration").disabled=!charuco;
  $("clearCalibration").disabled=!calibrated;
  $("analyze").disabled=busy || cameraBusy || !$("photo").files.length || (charuco && !$("sheet").files.length);
  $("measure").disabled=busy || cameraBusy || scale?.ok !== true || pending.length>0;
  $("openWorkbench").disabled=busy || cameraBusy || photoRecords===null;
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
  $("cameraRun").disabled=busy || cameraBusy || !$("cameraInput").files.length ||
    (images && (!$("cameraSheet").files.length || (!$("cameraImages").files.length && !$("cameraDirectory").files.length)));
  $("cameraCancel").disabled=!cameraBusy;
  $("cameraClearNoise").disabled=!$("cameraNoise").files.length;
  $("cameraUse").disabled=busy || cameraBusy || cameraResult?.accepted!==true;
  $("cameraActive").textContent=generatedCalibration?`Using the locally generated calibration for ${generatedCalibration.lensId}.`:"";
}
function kill() { worker?.terminate(); worker=null; busy=false; controls(); }
function revoke(urls) { urls.forEach(u=>URL.revokeObjectURL(u)); urls.length=0; }
function withdraw(source=false) {
  photoRecords=null;
  if(workbenchTransfer) workbenchTransfer.records=null;
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
$("cameraCancel").onclick=()=>{withdrawCamera(); $("cameraStatus").textContent="Calibration cancelled; no result published.";};
$("cameraClearNoise").onclick=()=>{$("cameraNoise").value=""; withdrawCamera(); $("cameraStatus").textContent="Separate noise declaration removed; calibrate from the current input.";};
$("cameraUse").onclick=()=>{
  if(cameraResult?.accepted!==true || busy || cameraBusy) return;
  generatedCalibration=cameraResult; $("calibration").value=""; $("referenceMode").value="charuco";
  withdraw(true); status("Generated calibration selected. Assert this photo's capture settings and analyze it.");
};
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
$("openWorkbench").onclick=()=>{
  if(busy || cameraBusy || !photoRecords) return;
  if(workbenchTransfer && !workbenchTransfer.target.closed) {
    status("A Workbench handoff is still opening. Finish or close that tab before opening another.");return;
  }
  const token=Array.from(crypto.getRandomValues(new Uint8Array(16)),v=>v.toString(16).padStart(2,"0")).join("");
  const url=new URL("index.html",import.meta.url);url.searchParams.set("photoImport",token);
  const target=window.open(url.href,"_blank");
  if(!target) {status("Workbench popup blocked. Download photo-measurements.json and add it there instead.");return;}
  workbenchTransfer={target,token,records:photoRecords};
  status("Workbench opened. Choose primary measurements and confirm the same wearer before checking.");
};
window.addEventListener("message",event=>{
  const transfer=workbenchTransfer,origin=new URL(import.meta.url).origin,m=event.data;
  if(!transfer || event.origin!==origin || event.source!==transfer.target ||
    m?.type!=="ftx-photo-records-ready" || m.token!==transfer.token) return;
  transfer.target.postMessage(transfer.records ? {type:"ftx-photo-records",token:transfer.token,records:transfer.records} :
    {type:"ftx-photo-records-cancelled",token:transfer.token},origin);
  workbenchTransfer=null;
});
window.addEventListener("pagehide",()=>{withdrawCamera(); withdraw(true);});
controls();

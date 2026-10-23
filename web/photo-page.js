const $ = id => document.getElementById(id);
let worker = null, ticket = 0, busy = false, scale = null, bitmap = null, pending = [];
let sourceUrls = [], resultUrls = [];
const status = text => {$("status").textContent=text;};
function controls() {
  const charuco=$("referenceMode").value==="charuco", calibrated=charuco && $("calibration").files.length>0;
  $("plainControls").hidden=charuco; $("charucoControls").hidden=!charuco; $("lensControls").hidden=!calibrated;
  $("paper").disabled=charuco; $("sheet").disabled=!charuco; $("calibration").disabled=!charuco;
  $("clearCalibration").disabled=!calibrated;
  $("analyze").disabled=busy || !$("photo").files.length || (charuco && !$("sheet").files.length);
  $("measure").disabled=busy || scale?.ok !== true;
  $("cancel").disabled=!busy;
}
function kill() { worker?.terminate(); worker=null; busy=false; controls(); }
function revoke(urls) { urls.forEach(u=>URL.revokeObjectURL(u)); urls.length=0; }
function withdraw(source=false) {
  ++ticket; kill(); revoke(resultUrls); $("downloads").replaceChildren();
  $("result").hidden=true; $("report").textContent=""; $("values").replaceChildren(); $("error").textContent="";
  if(source) {
    scale=null; bitmap?.close(); bitmap=null; pending=[]; revoke(sourceUrls);
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
  return JSON.stringify({depth_offset_mm:Number($("depth").value),camera_distance_mm:Number($("distance").value),
    click_px:Number($("click").value),sheet_level_sigma_deg:Number($("level").value),bootstrap:Number($("bootstrap").value),seed:0});
}
async function readJsonFile(file) {
  if(!file) return "";
  // Preserve the exact UTF-8 source, including a BOM (which the Rust JSON parser will reject).
  // Replacing invalid bytes or stripping a BOM would change the source identity before hashing.
  return new TextDecoder("utf-8",{fatal:true,ignoreBOM:true}).decode(await file.arrayBuffer());
}
async function run(op) {
  const file=$("photo").files[0], points=$("points").value, opts=options(), paper=$("paper").value;
  const referenceMode=$("referenceMode").value, charuco=referenceMode==="charuco";
  const sheetFile=charuco?$("sheet").files[0]:null, calibrationFile=charuco?$("calibration").files[0]:null;
  const captureFields={};
  if(calibrationFile) {
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
    const [bytes,sheet,calibration]=await Promise.all([file.arrayBuffer(),readJsonFile(sheetFile),readJsonFile(calibrationFile)]);
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
            if(calibrationFile) link($("sourceDownloads"),calibration,"photo-calibration.json","application/json",sourceUrls);
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
$("cancel").onclick=()=>{withdraw(); status("Cancelled; no measurements published.");};
for(const id of ["photo","paper","referenceMode","sheet","calibration","rotation"]) $(id).addEventListener("change",()=>{withdraw(true); status("Source changed; analyze the current photo.");});
for(const id of ["lensId","independence"]) $(id).addEventListener("input",()=>{withdraw(true); status("Capture evidence changed; analyze the current photo.");});
$("clearCalibration").onclick=()=>{$("calibration").value=""; $("lensId").value=""; $("rotation").value=""; $("independence").value=""; withdraw(true); status("Calibration removed; analyze the current photo.");};
for(const id of ["distance","depth","click","level","bootstrap","points"]) $(id).addEventListener("input",()=>{withdraw(); draw(); status("Inputs changed; measure again.");});
$("name").addEventListener("input",()=>{pending=[]; withdraw(); draw();});
$("clearPoints").onclick=()=>{$("points").value="{}"; pending=[]; withdraw(); draw();};
$("image").onclick=e=>{
  if(!bitmap || scale?.ok!==true || busy) return;
  const name=$("name").value.trim(), box=$("image").getBoundingClientRect();
  if(!name) {$("error").textContent="Choose a measurement name first."; return;}
  const point=[(e.clientX-box.left)*bitmap.width/box.width-.5,(e.clientY-box.top)*bitmap.height/box.height-.5];
  if(point.some((v,i)=>v<0||v>[bitmap.width,bitmap.height][i]-1)) return;
  withdraw(); pending.push(point);
  if(pending.length===2) {
    try {
      const pairs=JSON.parse($("points").value);
      if(!pairs || typeof pairs!=="object" || Array.isArray(pairs)) throw Error("Points must be a JSON object of named pairs.");
      $("points").value=JSON.stringify(Object.fromEntries([...Object.entries(pairs),[name,pending]]),null,2); pending=[];
    } catch(error) {pending=[]; $("error").textContent=error.message;}
  }
  $("clickStatus").textContent=pending.length?"First point selected; click the second.":"Pair added. Review its name and coordinates."; draw();
};
window.addEventListener("pagehide",()=>withdraw(true));
controls();

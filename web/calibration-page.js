// Each generation owns a disposable worker and one set of three matching downloads.
const el=id=>document.getElementById(id);
let worker=null,request=0,urls=[];
function withdraw(message="Settings changed; generate a new matching reference.") {
  request++;worker?.terminate();worker=null;
  for(const url of urls)URL.revokeObjectURL(url);
  urls=[];el("sheetDownloads").replaceChildren();
  el("sheetGenerate").disabled=false;el("sheetCancel").disabled=true;
  el("sheetStatus").textContent=message;
}
el("customSheet").hidden=false;
for(const id of ["sheetPaper","sheetSquare","sheetMarker","sheetDictionary"])
  for(const event of ["input","change"])el(id).addEventListener(event,()=>withdraw());
el("sheetCancel").onclick=()=>withdraw("Generation cancelled; no generated reference is available.");
el("sheetGenerate").onclick=()=>{
  withdraw("Generating all three files locally…");
  const id=request,current=worker=new Worker(new URL("./photo-worker.js",import.meta.url),{type:"module"});
  el("sheetGenerate").disabled=true;el("sheetCancel").disabled=false;
  current.onmessage=({data:m})=>{
    if(id!==request || worker!==current)return;
    if(m.type==="progress"){el("sheetStatus").textContent=m.text;return;}
    if(m.type==="error"){withdraw(m.message);return;}
    if(m.type!=="result" || m.request!==id)return;
    current.terminate();worker=null;
    try {
      const sidecar=JSON.parse(m.sidecar),paper=sidecar.paper;
      const files=[["pdf",m.pdf,"application/pdf"],["png",m.png,"image/png"],["json",m.sidecar,"application/json"]];
      const links=files.map(([ext,bytes,type])=>{
        const a=document.createElement("a"),url=URL.createObjectURL(new Blob([bytes],{type}));urls.push(url);
        a.href=url;a.download=`calib-sheet-${paper}.${ext}`;a.textContent=`Download ${ext.toUpperCase()} `;return a;
      });
      el("sheetDownloads").replaceChildren(...links);
      el("sheetStatus").textContent=`Generated ${paper.toUpperCase()}: ${sidecar.board.square_mm} mm squares, ${sidecar.board.marker_mm} mm markers, ${sidecar.board.dictionary}. Print at 100% / Actual size and check both axes with a ruler.`;
      el("sheetGenerate").disabled=false;el("sheetCancel").disabled=true;
    } catch(e){withdraw(`No reference published: ${e.message || e}`);}
  };
  current.onerror=e=>{if(id===request && worker===current)withdraw(`No reference published: ${e.message || "worker failed"}`);};
  current.postMessage({request:id,op:"calibration-sheet",paper:el("sheetPaper").value,
    square:el("sheetSquare").valueAsNumber,marker:el("sheetMarker").valueAsNumber,dictionary:el("sheetDictionary").value});
};

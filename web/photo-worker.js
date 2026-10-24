// One request per disposable worker. Calibration images are decoded sequentially; only retained
// source observations reach the camera solver. Files never leave the local worker/browser.
self.onmessage = async ({data: m}) => {
  let photo, calibration, sheet;
  try {
    self.postMessage({type:"progress",request:m.request,text:"Loading the local measurement engine…"});
    const ftx = await import("./pkg/ftx_wasm.js");
    await ftx.default({module_or_path:new URL("./pkg/ftx_wasm_bg.wasm",import.meta.url)});
    if(m.op==="calibration-sheet") {
      sheet=new ftx.WebCalibrationSheet(m.paper,m.square,m.marker,m.dictionary);
      const png=sheet.png(),pdf=sheet.pdf(),sidecar=sheet.sidecar();
      self.postMessage({type:"result",request:m.request,png,pdf,sidecar},[png.buffer,pdf.buffer]);
      return;
    }
    if(m.op==="calibrate-coordinates") {
      self.postMessage({type:"progress",request:m.request,text:"Fitting the declared camera and checking held-out coordinates…"});
      const report=ftx.camera_calibration(m.input,m.noise);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="calibrate-images") {
      calibration=new ftx.WebCameraCalibration(m.input,m.sheet,m.noise);
      const ids=JSON.parse(calibration.image_ids()), supplied=new Map();
      if(!Array.isArray(ids) || ids.length<1 || ids.length>32 || new Set(ids).size!==ids.length || ids.some(id=>typeof id!=="string")) {
        throw Error("Calibration acquisition returned invalid image identities.");
      }
      if(!Array.isArray(m.images) || m.images.length!==ids.length) throw Error("Select exactly the images declared by the calibration manifest.");
      for(const image of m.images) {
        if(!ids.includes(image.id) || supplied.has(image.id) || !image.file || image.file.size>64*1024*1024) {
          throw Error(`Unknown, duplicate or oversized calibration image: ${image.id}`);
        }
        supplied.set(image.id,image.file);
      }
      // Validate the complete selection before reading any image. Do not Promise.all this loop:
      // each decoder and encoded buffer must be released before acquiring the next photograph.
      for(let i=0;i<ids.length;i++) {
        const id=ids[i];
        self.postMessage({type:"progress",request:m.request,text:`Detecting calibration photo ${i+1}/${ids.length}: ${id}`});
        const diagnostics=JSON.parse(calibration.add_image(id,new Uint8Array(await supplied.get(id).arrayBuffer())));
        self.postMessage({type:"calibration-view",request:m.request,image:id,done:i+1,total:ids.length,diagnostics});
      }
      self.postMessage({type:"progress",request:m.request,text:"Fitting all declared views and checking held-out evidence…"});
      const report=calibration.finish();
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if (m.referenceMode === "charuco") {
      photo = ftx.WebPhoto.from_reference(new Uint8Array(m.bytes),m.sheet,m.calibration,m.capture);
    } else if (m.referenceMode === "plain" || m.referenceMode === undefined) {
      photo = new ftx.WebPhoto(new Uint8Array(m.bytes),m.paper);
    } else throw Error("Unknown photo reference mode");
    const scaleText = photo.report(), scale = JSON.parse(scaleText);
    if (m.op === "analyze") {
      const preview = photo.preview();
      self.postMessage({type:"result",request:m.request,scale,scaleText,preview},[preview.buffer]);
    } else if (m.op === "measure") {
      const report = photo.measure(m.points,m.options);
      self.postMessage({type:"result",request:m.request,scale,report});
    } else throw Error("Unknown photo operation");
  } catch(e) {
    self.postMessage({type:"error",request:m.request,message:String(e?.message || e)});
  } finally { photo?.free(); calibration?.free(); sheet?.free(); }
};

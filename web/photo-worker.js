// One request per disposable worker. Calibration images are decoded sequentially; only retained
// source observations reach the camera solver. Files never leave the local worker/browser.
self.onmessage = async ({data: m}) => {
  let photo, calibration, sheet, bodyImage;
  try {
    self.postMessage({type:"progress",request:m.request,text:"Loading the local measurement engine…"});
    const ftx = await import("./pkg/ftx_wasm.js");
    await ftx.default({module_or_path:new URL("./pkg/ftx_wasm_bg.wasm",import.meta.url)});
    if(m.op==="review-body-fit-start") {
      if(typeof m.input!=="string" || new TextEncoder().encode(m.input).length>16*1024*1024 ||
          !(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024 ||
          !Number.isSafeInteger(m.startIndex) || m.startIndex<0 || m.startIndex>3 ||
          !Number.isSafeInteger(m.frameIndex) || m.frameIndex<0 || m.frameIndex>7) {
        throw Error("Initialization preview needs prepared input up to 16 MiB, model up to 512 MiB, start 0..3 and frame 0..7.");
      }
      self.postMessage({type:"progress",request:m.request,text:"Evaluating the declared shape and pose without fitting; failed projections remain diagnostics…"});
      const report=ftx.review_body_fit_start(new Uint8Array(m.model),m.input,String(m.startIndex),String(m.frameIndex));
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="review-body-fit-image") {
      if(typeof m.review!=="string" || new TextEncoder().encode(m.review).length>16*1024*1024 ||
          !(m.image instanceof ArrayBuffer) || m.image.byteLength===0 || m.image.byteLength>128*1024*1024) {
        throw Error("Frame image review needs the original review up to 16 MiB and an original image up to 128 MiB.");
      }
      self.postMessage({type:"progress",request:m.request,text:"Checking the original capture image hash and already-oriented pixels…"});
      bodyImage=new ftx.WebBodyFitImage(new Uint8Array(m.image),m.review);
      const preview=bodyImage.preview(),report=bodyImage.report();
      self.postMessage({type:"result",request:m.request,preview,report},[preview.buffer]);
      return;
    }
    if(m.op==="review-body-fit-frame" || m.op==="open-body-fit") {
      const frameIndex=m.op==="open-body-fit"?0:m.frameIndex;
      if(typeof m.report!=="string" || new TextEncoder().encode(m.report).length>16*1024*1024 ||
          !(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024 ||
          !Number.isSafeInteger(frameIndex) || frameIndex<0 || frameIndex>7) {
        throw Error("Frame review needs an accepted report up to 16 MiB, model up to 512 MiB and a frame index in 0..7.");
      }
      self.postMessage({type:"progress",request:m.request,text:"Evaluating the full accepted pose and rechecking every supplied FIT/CHECK keypoint and silhouette slice…"});
      const report=ftx.review_body_fit_frame(new Uint8Array(m.model),m.report,String(frameIndex));
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="fit-body") {
      if(typeof m.input!=="string" || new TextEncoder().encode(m.input).length>16*1024*1024 ||
          !(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024) {
        throw Error("Body fitting requires prepared JSON up to 16 MiB and a converted model up to 512 MiB.");
      }
      if(!Number.isSafeInteger(m.iterations) || m.iterations<1 || m.iterations>100 ||
          !Number.isSafeInteger(m.maxEvaluations) || m.maxEvaluations<1 || m.maxEvaluations>1000000) {
        throw Error("Body-fit limits must be 1..100 steps and 1..1000000 model evaluations.");
      }
      self.postMessage({type:"progress",request:m.request,text:"Checking the model identity and fitting every declared start, frame and held-out observation…"});
      const report=ftx.fit_body(new Uint8Array(m.model),m.input,String(m.iterations),String(m.maxEvaluations));
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="still-window") {
      self.postMessage({type:"progress",request:m.request,text:"Checking early/late motion and calibrated geometry for every tracked landmark…"});
      const report=ftx.still_window(m.input,m.source);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="bundle-adjust") {
      self.postMessage({type:"progress",request:m.request,text:"Refining anchored cameras and landmarks and checking every held-out view…"});
      const report=ftx.bundle_adjust(m.input,m.iterations);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="triangulate") {
      self.postMessage({type:"progress",request:m.request,text:"Checking calibrated rays, held-out views and declared click uncertainty…"});
      const report=ftx.triangulate(m.input,m.noise);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="section") {
      self.postMessage({type:"progress",request:m.request,text:"Checking the photo evidence and propagating section uncertainty…"});
      const report=ftx.photo_section(m.width,m.depth,m.options);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
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
  } finally { photo?.free(); calibration?.free(); sheet?.free(); bodyImage?.free(); }
};

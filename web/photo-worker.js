// One request per disposable worker. Calibration images are decoded sequentially; only retained
// source observations reach the camera solver. Files never leave the local worker/browser.
self.onmessage = async ({data: m}) => {
  let photo, calibration, sheet, bodyImage, bodyCapture;
  try {
    self.postMessage({type:"progress",request:m.request,text:"Loading the local measurement engine…"});
    const ftx = await import("./pkg/ftx_wasm.js");
    await ftx.default({module_or_path:new URL("./pkg/ftx_wasm_bg.wasm",import.meta.url)});
    if((typeof ftx.initSync==="function" || "wasm_interface_stamp" in ftx) &&
        (typeof ftx.wasm_interface_stamp!=="function" || ftx.wasm_interface_stamp()!=="ftx-wasm-interface/2026-11-27.3")) {
      throw Error("Stale web/pkg bundle: rebuild web/pkg via ./tools/build_web.sh");
    }
    if(m.op==="body-capture-catalogue") {
      if(!(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024) {
        throw Error("Choose a converted model up to 512 MiB.");
      }
      if(typeof ftx.body_capture_catalogue!=="function") throw Error("Rebuild web/pkg: missing the body capture catalogue.");
      self.postMessage({type:"progress",request:m.request,text:"Checking the exact converted model and its available shape and pose controls…"});
      const report=ftx.body_capture_catalogue(new Uint8Array(m.model));
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="body-photo-capture") {
      const bounded=(value,limit)=>typeof value==="string" && new TextEncoder().encode(value).length<=limit;
      if(!bounded(m.manifest,256*1024) || !bounded(m.calibration,16*1024*1024) ||
          !bounded(m.sheet,256*1024) || !bounded(m.choices,256*1024) ||
          !m.modelFile || typeof m.modelFile.arrayBuffer!=="function" || !Number.isSafeInteger(m.modelFile.size) ||
          m.modelFile.size<1 || m.modelFile.size>512*1024*1024 || !Array.isArray(m.images) || m.images.length<2 || m.images.length>8) {
        throw Error("Still capture needs 2–8 images, a model up to 512 MiB, calibration up to 16 MiB and declarations up to 256 KiB each.");
      }
      if(typeof ftx.WebBodyCapture!=="function") throw Error("Rebuild web/pkg: missing the still-photo body capture engine.");
      const calibratedOverrides=Object.hasOwn(m,"calibrations");
      if(calibratedOverrides && (!bounded(m.calibrations,16*1024*1024) ||
          [m.manifest,m.calibration,m.sheet,m.calibrations].reduce((sum,text)=>sum+new TextEncoder().encode(text).length,0)>16*1024*1024)) {
        throw Error("Complete default and per-photo capture source documents exceed 16 MiB.");
      }
      if(calibratedOverrides && typeof ftx.WebBodyCapture.with_calibrations!=="function") {
        throw Error("Rebuild web/pkg: missing independently calibrated capture photographs.");
      }
      const supplied=new Map();
      for(const image of m.images) {
        if(!image || typeof image.id!=="string" || supplied.has(image.id) || !image.file ||
            typeof image.file.arrayBuffer!=="function" || !Number.isSafeInteger(image.file.size) ||
            image.file.size<1 || image.file.size>64*1024*1024) {
          throw Error("Choose unique original images up to 64 MiB each.");
        }
        supplied.set(image.id,image.file);
      }
      bodyCapture=calibratedOverrides?ftx.WebBodyCapture.with_calibrations(m.manifest,m.calibration,m.sheet,m.calibrations):
        new ftx.WebBodyCapture(m.manifest,m.calibration,m.sheet);
      const rectify=JSON.parse(m.manifest).rectify===true;
      if(rectify && typeof bodyCapture.take_image!=="function") {
        throw Error("Rebuild web/pkg: missing calibrated capture image correction.");
      }
      const ids=JSON.parse(bodyCapture.image_ids());
      if(!Array.isArray(ids) || ids.length!==supplied.size || new Set(ids).size!==ids.length || ids.some(id=>!supplied.has(id))) {
        throw Error("Select exactly the source images declared by this capture.");
      }
      for(let index=0;index<ids.length;index++) {
        const id=ids[index], file=supplied.get(id);
        self.postMessage({type:"progress",request:m.request,text:`Checking stationary board in photograph ${index+1}/${ids.length}: ${id}`});
        const bytes=new Uint8Array(await file.arrayBuffer());
        if(bytes.byteLength!==file.size) throw Error("A capture image changed while it was being read.");
        const diagnostics=JSON.parse(bodyCapture.add_image(id,bytes));
        if(rectify) {
          const correction=diagnostics.rectification, image=correction?.image;
          if(correction?.schema!=="ftx-capture-rectification/1" || !image ||
              typeof image.image_name!=="string" || !/^rectified-[0-9a-f]{64}\.png$/.test(image.image_name) ||
              !/^[0-9a-f]{64}$/.test(image.image_sha256) ||
              !Array.isArray(image.image_size_px) || image.image_size_px.length!==2 ||
              image.image_size_px.some(value=>!Number.isSafeInteger(value) || value<2) ||
              image.image_size_px[0]*image.image_size_px[1]>16000000) {
            throw Error("The engine did not return a complete corrected capture image binding.");
          }
          const png=bodyCapture.take_image();
          if(!(png instanceof Uint8Array) || png.byteLength===0 || png.byteLength>128*1024*1024) {
            throw Error("The engine did not return the corrected capture PNG.");
          }
          const output=png.byteOffset===0 && png.byteLength===png.buffer.byteLength?png:png.slice();
          self.postMessage({type:"body-capture-image",request:m.request,sourceImage:id,image,png:output.buffer},[output.buffer]);
        } else if(diagnostics.rectification) {
          throw Error("The engine corrected capture pixels without the requested option.");
        }
        self.postMessage({type:"body-capture-view",request:m.request,image:id,done:index+1,total:ids.length,diagnostics});
      }
      self.postMessage({type:"progress",request:m.request,text:"Checking explicit model choices and preparing the source-bound draft…"});
      const model=new Uint8Array(await m.modelFile.arrayBuffer());
      if(model.byteLength!==m.modelFile.size) throw Error("The converted model changed while it was being read.");
      const report=bodyCapture.finish(model,m.choices);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="body-fit-mask") {
      if(typeof m.input!=="string" || new TextEncoder().encode(m.input).length>16*1024*1024 ||
          typeof m.selection!=="string" || new TextEncoder().encode(m.selection).length>64*1024 ||
          !(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024 ||
          !(m.mask instanceof ArrayBuffer) || m.mask.byteLength===0 || m.mask.byteLength>128*1024*1024) {
        throw Error("Mask import needs prepared input up to 16 MiB, model up to 512 MiB, a binary PNG up to 128 MiB and declarations up to 64 KiB.");
      }
      if(typeof ftx.body_fit_mask!=="function") throw Error("Rebuild web/pkg: missing binary body-mask import.");
      self.postMessage({type:"progress",request:m.request,text:"Checking exact source bindings and binary mask pixels; sampling the declared FIT and CHECK slices…"});
      const report=ftx.body_fit_mask(new Uint8Array(m.model),m.input,new Uint8Array(m.mask),m.selection);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="body-fit-starts" || m.op==="body-fit-observations" || m.op==="body-fit-targets") {
      const observations=m.op==="body-fit-observations", targets=m.op==="body-fit-targets", editLimit=observations || targets?256:64;
      if(typeof m.input!=="string" || new TextEncoder().encode(m.input).length>16*1024*1024 ||
          typeof m.edits!=="string" || new TextEncoder().encode(m.edits).length>editLimit*1024 ||
          !(m.model instanceof ArrayBuffer) || m.model.byteLength===0 || m.model.byteLength>512*1024*1024) {
        throw Error("Body editing needs prepared input up to 16 MiB, model up to 512 MiB and edits up to "+editLimit+" KiB.");
      }
      const entry=targets?"body_fit_targets":observations?"body_fit_observations":"body_fit_starts";
      if(typeof ftx[entry]!=="function") throw Error("Rebuild web/pkg: missing the requested body authoring engine.");
      self.postMessage({type:"progress",request:m.request,text:targets?"Checking exact model targets and unfitted rest geometry…":observations?"Checking the exact sources, declared targets and stored-pixel observations…":"Checking the exact model, source evidence and declared initialization bounds…"});
      const report=ftx[entry](new Uint8Array(m.model),m.input,m.edits);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
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
      self.postMessage({type:"progress",request:m.request,text:"Checking the input-bound image hash and already-oriented pixels…"});
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
    if(m.op==="triangulate-distance") {
      if(typeof m.input!=="string") throw Error("Distance evidence must be original JSON text.");
      if(typeof ftx.triangulate_distance!=="function") throw Error("Rebuild web/pkg: missing calibrated landmark distance.");
      self.postMessage({type:"progress",request:m.request,text:"Rechecking both landmarks and their shared camera and reference errors…"});
      const report=ftx.triangulate_distance(m.input);
      self.postMessage({type:"result",request:m.request,report});
      return;
    }
    if(m.op==="triangulate") {
      if(m.lensNoise!==undefined && typeof m.lensNoise!=="string") throw Error("Shared-lens evidence must be original JSON text.");
      if(m.sheetNoise!==undefined && typeof m.sheetNoise!=="string") throw Error("Sheet-corner evidence must be original JSON text.");
      if(m.referenceScaleNoise!==undefined && typeof m.referenceScaleNoise!=="string") throw Error("Reference-size evidence must be original JSON text.");
      if(m.referenceScaleNoise && typeof ftx.triangulate_with_reference_scale_noise!=="function") throw Error("Rebuild web/pkg: missing shared reference-scale triangulation uncertainty.");
      if(!m.referenceScaleNoise && m.sheetNoise && typeof ftx.triangulate_with_camera_noise!=="function") throw Error("Rebuild web/pkg: missing sheet-corner triangulation uncertainty.");
      if(!m.referenceScaleNoise && !m.sheetNoise && m.lensNoise && typeof ftx.triangulate_with_lens_noise!=="function") throw Error("Rebuild web/pkg: missing shared-lens triangulation uncertainty.");
      self.postMessage({type:"progress",request:m.request,text:m.referenceScaleNoise?
        "Rechecking the original sheet cameras and propagating their shared reference-scale uncertainty…":m.sheetNoise?
        "Rechecking saved sheet poses and propagating independent corner noise through the calibrated cameras…":m.lensNoise?
        "Rechecking saved calibrations and propagating shared-lens uncertainty through the calibrated rays…":
        "Checking calibrated rays, held-out views and declared click uncertainty…"});
      const report=m.referenceScaleNoise?
        ftx.triangulate_with_reference_scale_noise(m.input,m.noise,m.lensNoise || "",m.sheetNoise || "",m.referenceScaleNoise):
        m.sheetNoise?ftx.triangulate_with_camera_noise(m.input,m.noise,m.lensNoise || "",m.sheetNoise):
        m.lensNoise?ftx.triangulate_with_lens_noise(m.input,m.noise,m.lensNoise):ftx.triangulate(m.input,m.noise);
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
  } finally { photo?.free(); calibration?.free(); sheet?.free(); bodyImage?.free(); bodyCapture?.free(); }
};

// One request per disposable worker. The Rust calibration and image are always released.
self.onmessage = async ({data: m}) => {
  let photo;
  try {
    self.postMessage({type:"progress",request:m.request,text:"Loading the local measurement engine…"});
    const ftx = await import("./pkg/ftx_wasm.js");
    await ftx.default({module_or_path:new URL("./pkg/ftx_wasm_bg.wasm",import.meta.url)});
    photo = new ftx.WebPhoto(new Uint8Array(m.bytes),m.paper);
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
  } finally { photo?.free(); }
};

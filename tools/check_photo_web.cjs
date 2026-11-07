// Actual local photo controls through compiled WASM. Native captures are made by ftx measure.
// FTX_PLAYWRIGHT_MODULE=playwright-core NODE_PATH=... node tools/check_photo_web.cjs NATIVE_DIR
// Use --section for the standalone synthetic front/side section and Workbench handoff workflow.
// Use --charuco NATIVE_DIR for a printed dictionary's real photo controls and native report replay.
// Use --triangulate NATIVE_DIR for exact prepared inputs and original native geometry reports.
// Use --bundle NATIVE_DIR for anchored refinement and transitions through the shared geometry tool.
// Use --still-window NATIVE_DIR for sparse stationarity candidates and original native report replay.
// Use --fit-body NATIVE_DIR for prepared multi-frame Anny fits and original native reports.
const {chromium}=require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const assert=require("node:assert/strict"), fs=require("node:fs"), path=require("node:path"), http=require("node:http"), os=require("node:os");
const sha256=bytes=>require("node:crypto").createHash("sha256").update(bytes).digest("hex");
const root=path.resolve(__dirname,".."), web=path.join(root,"web"), sectionOnly=process.argv[2]==="--section", charucoOnly=process.argv[2]==="--charuco", triangulationOnly=process.argv[2]==="--triangulate", bundleOnly=process.argv[2]==="--bundle", stillOnly=process.argv[2]==="--still-window", bodyFitOnly=process.argv[2]==="--fit-body", native=sectionOnly?null:process.argv[charucoOnly||triangulationOnly||bundleOnly||stillOnly||bodyFitOnly?3:2];
const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(web,"pkg");
if(!native && !sectionOnly) throw Error("Supply native captures, --charuco NATIVE_DIR, --triangulate NATIVE_DIR, --bundle NATIVE_DIR, --still-window NATIVE_DIR, --fit-body NATIVE_DIR, or --section");
const work=fs.mkdtempSync(path.join(os.tmpdir(),"ftx-photo-browser-"));
async function photoHandoff(browser,origin) {
  const context=await browser.newContext({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"}),errors=[],requests=[];
  context.on("page",p=>p.on("pageerror",e=>errors.push(String(e))));
  context.on("request",r=>requests.push({url:r.url(),method:r.method()}));
  // Give both ordinary round exporters the same explicit clock, preserving all numerical code.
  await context.route("**/worker.js",async route=>{
    const response=await route.fetch(),body=await response.text();assert.ok(body.includes("const d = new Date();"));
    await route.fulfill({response,body:body.replace("const d = new Date();","const d = new Date(2026, 9, 29, 12, 0);")});
  });
  const source=await context.newPage();
  const ready=async p=>p.waitForFunction(()=>/^Ready/.test(document.getElementById("status").textContent));
  const prepare=async p=>{
    await p.check('input[name="program"][value="zouave_bolero"]');
    await p.check('input[name="bodyMode"][value="tape"]');
  };
  const run=async p=>{
    await p.click("#go");await p.waitForFunction(()=>!document.getElementById("results").classList.contains("hidden") || !document.getElementById("errorBox").classList.contains("hidden"));
    assert.equal(await p.locator("#errorBox").isVisible(),false,await p.textContent("#errorText"));
    const event=p.waitForEvent("download");await p.click("#resFiles a[download$='.zip']");const d=await event;
    assert.equal(await d.failure(),null);return fs.readFileSync(await d.path());
  };
  try {
    await source.goto(origin+"/photo.html");
    await source.setInputFiles("#photo",path.join(native,"photo.jpg"));await source.selectOption("#paper","letter");
    await source.click("#analyze");await source.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Review the detected"));
    await source.fill("#points",fs.readFileSync(path.join(native,"points-jpeg.json"),"utf8"));
    await source.click("#measure");await source.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Photo-estimated records available"));
    const text=await source.textContent("#report");assert.deepEqual(JSON.parse(text),(()=>{
      const value=JSON.parse(fs.readFileSync(path.join(native,"native-measure-jpeg.json"))).result;delete value.photo.image_input.path;return value;
    })());
    assert.equal(await source.evaluate(()=>localStorage.length),0);
    const [receiver]=await Promise.all([source.waitForEvent("popup"),source.click("#openWorkbench")]);
    await ready(receiver);await receiver.waitForFunction(()=>document.getElementById("photoImportText").textContent.startsWith("Photo-record file received"));
    assert.equal(await receiver.evaluate(()=>window.opener),null);assert.equal(await receiver.locator("#results").isVisible(),false);
    await receiver.click("#go");await receiver.waitForFunction(()=>document.getElementById("errorText").textContent.includes("tape measurements"));
    await prepare(receiver);await receiver.click("#go");await receiver.waitForFunction(()=>document.getElementById("errorText").textContent.includes("same wearer"));
    await receiver.check("#supplementalConsent");const transferred=await run(receiver);
    fs.writeFileSync(path.join(work,"photo-handoff.zip"),transferred,{flag:"wx"});
    const measurements=JSON.parse(require("node:child_process").execFileSync("unzip",["-p",path.join(work,"photo-handoff.zip"),"measurements.json"]));
    assert.equal(measurements.measurement_imports[0],text);assert.equal(measurements.fused.across_back.primary_provenance,"PHOTO_ESTIMATED");
    assert.equal(measurements.taken_on,null);assert.match(await receiver.textContent("#resGate"),/before cutting cloth/);
    await receiver.screenshot({path:path.join(work,"photo-handoff-workbench.png"),fullPage:true});
    const manual=await context.newPage();await manual.goto(origin+"/index.html");await ready(manual);await prepare(manual);
    await manual.locator("#supplementalEvidence").evaluate(e=>{e.open=true;});
    await manual.setInputFiles("#supplementalFiles",{name:"photo-measurements.json",mimeType:"application/json",buffer:Buffer.from(text)});
    await manual.check("#supplementalConsent");assert.deepEqual(await run(manual),transferred,"Complete round ZIP equals ordinary manual upload");
    await receiver.click("#clearSupplemental");assert.equal(await receiver.locator("#results").isVisible(),false);
    const cleared=await run(receiver);fs.writeFileSync(path.join(work,"photo-handoff-cleared.zip"),cleared,{flag:"wx"});
    const clearedMeasurements=JSON.parse(require("node:child_process").execFileSync("unzip",["-p",path.join(work,"photo-handoff-cleared.zip"),"measurements.json"]));
    assert.equal(clearedMeasurements.measurement_imports,undefined);
    // Hold the real receiving document until a source edit invalidates its pending transfer.
    let release,arrived;const parked=new Promise(resolve=>{arrived=resolve;});
    const hold=route=>{release=route;arrived();};await context.route("**/index.html?photoImport=*",hold);
    const popup=source.waitForEvent("popup");await source.click("#openWorkbench");await parked;
    await source.fill("#points","{}");assert.equal(await source.isDisabled("#openWorkbench"),true);
    await release.continue();const cancelled=await popup;await ready(cancelled);
    await cancelled.waitForFunction(()=>document.getElementById("photoImportText").textContent.startsWith("Photo review changed"));
    assert.equal(await cancelled.evaluate(()=>window.opener),null);assert.equal(await cancelled.locator("#results").isVisible(),false);
    await cancelled.click("#go");await cancelled.waitForFunction(()=>document.getElementById("errorText").textContent.includes("Photo review changed"));
    await context.unroute("**/index.html?photoImport=*",hold);
    assert.deepEqual(errors,[]);assert.ok(requests.every(r=>r.url.startsWith(origin+"/")&&r.method==="GET"));
    assert.equal(await source.evaluate(()=>Object.values(localStorage).some(v=>v.includes('fabric-measurements/1'))),false);
    console.log("PASS photo-to-Workbench: real local popup, exact original PHOTO_ESTIMATED records, explicit primary evidence/consent, unchanged NOT_READY, complete manual-upload ZIP equality, clear/stale cancellation and opener cleanup; no uploads or stored photo records.");
  } finally {await context.close();}
}
async function sectionWorkflow(browser,origin) {
  const context=await browser.newContext({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"}),errors=[],requests=[];
  context.on("page",p=>p.on("pageerror",e=>errors.push(String(e))));
  context.on("request",r=>requests.push({url:r.url(),method:r.method()}));
  const page=await context.newPage();page.setDefaultTimeout(30000);
  const front=' {\r\n"schema":"fabric-measurements/1","wearer":{"label":"synthetic section workflow"},"taken_on":"2026-10-01",'+
    '"records":[{"name":"waist_width","value_mm":160,"sigma_mm":4,"provenance":"PHOTO_ESTIMATED","method":"synthetic front view"}],'+
    '"fused":{"waist_width":{"value_mm":9999}},"source_integer":18446744073709551615,"source_float":1.0,"source_zero":-0.0\r\n}\r\n';
  const side=' '+JSON.stringify({schema:"fabric-measurements/1",wearer:{label:"synthetic section workflow"},taken_on:null,
    records:[{name:"waist_depth",value_mm:130,sigma_mm:5,provenance:"PHOTO_ESTIMATED",method:"synthetic side view"}],fused:{}})+'\n';
  const source=async(id,text)=>page.setInputFiles(id,{name:id.includes("Width")?"front.json":"side.json",mimeType:"application/json",buffer:Buffer.from(text)});
  const download=async name=>{
    const event=page.waitForEvent("download");await page.click(`a[download="${name}"]`);const d=await event;
    assert.equal(d.suggestedFilename(),name);assert.equal(await d.failure(),null);return fs.readFileSync(await d.path());
  };
  const calculate=async()=>{
    await page.click("#sectionRun");
    await page.waitForFunction(()=>/available|refused|No section result/.test(document.getElementById("sectionStatus").textContent));
  };
  const absent=async()=>{
    assert.equal(await page.locator("#sectionResult").isVisible(),false);
    assert.equal(await page.locator('a[download="section-measurements.json"]').count(),0);
    assert.equal(await page.isDisabled("#sectionWorkbench"),true);
  };
  try {
    await page.goto(origin+"/photo.html");
    await source("#sectionWidthFile",front);await source("#sectionDepthFile",side);
    assert.equal(await page.isDisabled("#sectionRun"),true,"Explicit wearer and anatomical-level review is required");
    await page.check("#sectionConsent");await calculate();assert.equal(await page.textContent("#sectionError"),"");
    const original=(await download("section-measurements.json")).toString(),result=JSON.parse(original);
    assert.equal(result.girth_mm,484.2);assert.equal(result.sigma_mm,14.6);
    assert.deepEqual(result.sigma_terms_mm,{width:6.99,depth:7.87,k:10.05});
    assert.equal(result.record.provenance,"PHOTO_ESTIMATED");assert.equal(result.taken_on,null);
    assert.equal(result.section_sources.width.text,front);assert.equal(result.section_sources.width.sha256,sha256(front));
    assert.equal(result.section_sources.depth.text,side);assert.equal(result.section_sources.depth.sha256,sha256(side));
    assert.equal((await download("section-front-measurements.json")).toString(),front);
    assert.equal((await download("section-side-measurements.json")).toString(),side);
    assert.equal((await download("section-options.json")).toString(),result.section_sources.options.text);
    assert.match(await page.textContent("#sectionSummary"),/484\.2.*14\.6.*PHOTO_ESTIMATED/);
    assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0,"The photo tool stores no evidence or preferences");
    fs.writeFileSync(path.join(work,"section-measurements.json"),original,{flag:"wx"});

    const [receiver]=await Promise.all([page.waitForEvent("popup"),page.click("#sectionWorkbench")]);
    await receiver.waitForFunction(()=>/^Ready/.test(document.getElementById("status").textContent));
    await receiver.waitForFunction(()=>document.getElementById("photoImportText").textContent.startsWith("Photo-record file received"));
    assert.equal(await receiver.evaluate(()=>window.opener),null);assert.equal(await receiver.locator("#results").isVisible(),false);
    await receiver.click("#go");await receiver.waitForFunction(()=>document.getElementById("errorText").textContent.includes("tape measurements"));
    await receiver.check('input[name="program"][value="zouave_bolero"]');await receiver.check('input[name="bodyMode"][value="tape"]');
    await receiver.click("#go");await receiver.waitForFunction(()=>document.getElementById("errorText").textContent.includes("same wearer"));
    await receiver.check("#supplementalConsent");await receiver.click("#go");
    await receiver.waitForFunction(()=>!document.getElementById("results").classList.contains("hidden") || !document.getElementById("errorBox").classList.contains("hidden"),null,{timeout:60000});
    assert.equal(await receiver.locator("#errorBox").isVisible(),false,await receiver.textContent("#errorText"));
    const event=receiver.waitForEvent("download");await receiver.click("#resFiles a[download$='.zip']");const zip=await event;
    assert.equal(await zip.failure(),null);
    const zipPath=path.join(work,"section-handoff.zip");fs.writeFileSync(zipPath,fs.readFileSync(await zip.path()),{flag:"wx"});
    const extract=name=>JSON.parse(require("node:child_process").execFileSync("unzip",["-p",zipPath,name]));
    const measurements=extract("measurements.json");assert.equal(measurements.measurement_imports[0],original);
    assert.equal(measurements.fused.waist.primary_provenance,"PHOTO_ESTIMATED");assert.equal(measurements.taken_on,null);
    assert.equal(extract("readiness.json").status,"NOT_READY");
    const expectedPreferences=await receiver.evaluate(()=>({
      "fab-wb-bodyMode":JSON.stringify("tape"),"fab-wb-program":JSON.stringify("zouave_bolero"),
      "fab-wb-style-zouave_bolero":JSON.stringify(document.getElementById("style").value),
    }));
    await receiver.screenshot({path:path.join(work,"section-handoff-workbench.png"),fullPage:true});await receiver.close();
    console.log("PASS section handoff: actual local popup, explicit primary evidence and wearer consent, exact original report, PHOTO_ESTIMATED fusion, unknown date and NOT_READY preserved.");

    await page.selectOption("#sectionMode","calibrate");await absent();await page.fill("#sectionTape","500");await calculate();
    const calibrated=JSON.parse(await download("section-calibration.json"));assert.ok(calibrated.k_child>1);assert.ok(calibrated.k_child_sigma>0);
    for(const field of ["schema","records","record","readiness"])assert.equal(calibrated[field],undefined,field);
    assert.equal(await page.isDisabled("#sectionWorkbench"),true);assert.equal(await page.isDisabled("#sectionMaterials"),true);
    assert.match(await page.textContent("#sectionNotes"),/not another independent/);
    await page.selectOption("#sectionMode","girth");
    await source("#sectionWidthFile",front.replace('"PHOTO_ESTIMATED"','"MODEL_FIT"'));
    assert.equal(await page.isChecked("#sectionConsent"),false);await absent();await page.check("#sectionConsent");await calculate();
    assert.match(await page.textContent("#sectionError"),/photo evidence|PHOTO_ESTIMATED|provenance/i);await absent();
    await source("#sectionWidthFile",front);await page.check("#sectionConsent");

    // Hold the real WASM module to exercise edits and cancellation before a worker can publish.
    for(const change of ["assumptions","cancel"]) {
      let release,arrived;const parked=new Promise(resolve=>{arrived=resolve;});
      const hold=route=>{release=route;arrived();};await context.route("**/ftx_wasm_bg.wasm",hold);
      await page.click("#sectionRun");let timeout;
      try {await Promise.race([parked,new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error("Section module request was not intercepted")),30000);})]);}
      finally {clearTimeout(timeout);}
      if(change==="assumptions")await page.fill("#sectionClothing","1");else await page.click("#sectionCancel");
      await absent();assert.equal(await page.isDisabled("#sectionCancel"),true);
      await release.continue().catch(()=>{});await context.unroute("**/ftx_wasm_bg.wasm",hold);
      await page.fill("#sectionClothing","0");await calculate();assert.equal(await page.textContent("#sectionError"),"");
      assert.deepEqual(JSON.parse(await download("section-measurements.json")),result,"New work wins after an interrupted section worker");
    }
    const section=page.locator("section").filter({has:page.locator("#sectionRun")});
    for(const [label,width,height] of [["desktop",1280,900],["mobile",390,844]]) {
      await page.setViewportSize({width,height});await section.scrollIntoViewIfNeeded();
      const overflow=await section.evaluate(element=>[...element.querySelectorAll("input,select,button,p,pre")]
        .filter(e=>e.getClientRects().length).map(e=>({id:e.id,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}))
        .filter(e=>e.left<0 || e.right>innerWidth+1));
      assert.deepEqual(overflow,[],`${label} section controls fit the viewport`);
      await section.screenshot({path:path.join(work,`section-${label}.png`)});
    }
    assert.deepEqual(errors,[]);
    assert.deepEqual(requests.filter(r=>{
      const url=new URL(r.url);
      return !["http:","blob:"].includes(url.protocol) || url.origin!==origin || r.method!=="GET";
    }),[],"Section requests and Blob downloads stay on the exact local origin and are read-only");
    for(const p of context.pages()) {
      assert.deepEqual(await p.evaluate(()=>Object.fromEntries(Object.entries(localStorage))),expectedPreferences,"Only the ordinary Workbench's explicit UI preferences are retained");
      assert.equal(await p.evaluate(()=>sessionStorage.length),0);
    }
    console.log("PASS section workflow: compiled Rust worker/spec girth and uncertainty, exact source/options downloads, calibration-only coefficients, provenance refusal, stale-worker cancellation, desktop/mobile controls, no uploads or stored measurement/source data, no page errors. Artifacts:",work);
  } catch(error) {
    for(const p of context.pages())console.error("Section failure state",p.url(),await p.evaluate(()=>Object.fromEntries(
      ["status","photoImportText","errorText","sectionStatus","sectionError"].map(id=>[id,document.getElementById(id)?.textContent]))).catch(()=>"page unavailable"));
    console.error("Section page errors",errors);throw error;
  } finally {await context.close();}
}
async function triangulationWorkflow(browser,origin) {
  const context=await browser.newContext({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"}),errors=[],requests=[];
  context.on("page",page=>page.on("pageerror",error=>errors.push(String(error))));
  context.on("request",request=>requests.push({url:request.url(),method:request.method()}));
  const page=await context.newPage(),source=name=>path.join(native,name);
  const parse=text=>JSON.parse(text,(key,value,meta)=>["timestamp_us","start_us","end_us","max_skew_us"].includes(key)?meta.source:value);
  const download=async name=>{
    // Chromium LocalFrame::ShouldThrottleDownload limits a burst to ten files per second.
    // Pace the real clicks; every download and its exact bytes must still be observed.
    await page.waitForTimeout(125);
    const event=page.waitForEvent("download");await page.click(`#triangulationDownloads a[download="${name}"]`);
    const file=await event;assert.equal(await file.failure(),null);return fs.readFileSync(await file.path(),"utf8");
  };
  const run=async()=>{
    await page.click("#triangulationRun");
    await page.waitForFunction(()=>document.getElementById("triangulationCancel").disabled,null,{timeout:30000});
  };
  const select=async(name,noise)=>{
    await page.setInputFiles("#triangulationInput",source(name+"-input.json"));
    await page.setInputFiles("#triangulationNoise",noise?source(name+"-noise.json"):[]);
  };
  try {
    await page.goto(origin+"/photo.html");
    if(stillOnly) {
      await page.setInputFiles("#triangulationNoise",{name:"old-noise.json",mimeType:"application/json",buffer:Buffer.from("{}")});
      await page.selectOption("#geometryOperation","bundle-adjust");await page.fill("#bundleIterations","1.5");await page.selectOption("#geometryOperation","still-window");
      assert.equal(await page.inputValue("#triangulationNoise"),"");
      assert.equal(await page.isVisible("#geometryNoiseControls"),false);assert.equal(await page.isDisabled("#bundleIterations"),true);
      for(const name of ["stationary","fit-motion","held-out-motion","weak","full-width"]) {
        await page.setInputFiles("#triangulationInput",source(name+"-input.json"));await run();
        assert.equal(await page.isVisible("#triangulationResult"),true,await page.textContent("#triangulationError"));
        const report=await download("still-window.json");
        assert.deepEqual(parse(report),parse(fs.readFileSync(source(name+"-native.json"),"utf8")),name+" complete original native still report");
        assert.equal(await download("still-window-input.json"),fs.readFileSync(source(name+"-input.json"),"utf8"));
        assert.equal(await page.locator("#triangulationDownloads a").count(),2);
        assert.equal(await page.textContent("#triangulationReport"),report);
        assert.match(await page.textContent("#triangulationPrecision"),/Candidate only: human review/);
        assert.equal(await page.isDisabled("#openWorkbench"),true);assert.equal(await page.locator("#downloads a").count(),0);
        if(name==="weak")assert.match(await page.textContent("#triangulationPoint"),/unavailable.*below 3 deg/);
        if(name==="full-width")assert.ok(report.includes('"end_us":18446744073709551615'));
      }
      await page.setViewportSize({width:390,height:844});await page.setInputFiles("#triangulationInput",source("stationary-input.json"));
      let release,arrived;const reached=new Promise(resolve=>{arrived=resolve;}),hold=route=>{release=route;arrived();};
      await context.route("**/ftx_wasm_bg.wasm",hold);
      const requested=page.waitForRequest(request=>request.url().endsWith("/ftx_wasm_bg.wasm"),{timeout:30000});
      await page.click("#triangulationRun");await requested;await reached;await page.click("#triangulationCancel");
      await release.continue().catch(()=>{});await context.unroute("**/ftx_wasm_bg.wasm",hold);
      assert.equal(await page.isVisible("#triangulationResult"),false);assert.equal(await page.locator("#triangulationDownloads a").count(),0);
      await run();assert.deepEqual(parse(await download("still-window.json")),parse(fs.readFileSync(source("stationary-native.json"),"utf8")));
      for(const buffer of [Buffer.from([255]),Buffer.from('\ufeff{}'),Buffer.alloc(256*1024+1,32)]) {
        await page.setInputFiles("#triangulationInput",{name:"bad-still.json",mimeType:"application/json",buffer});await run();
        assert.equal(await page.isVisible("#triangulationResult"),false);assert.equal(await page.locator("#triangulationDownloads a").count(),0);
        assert.notEqual(await page.textContent("#triangulationError"),"");
      }
      for(const [mode,name,downloadName] of [["triangulate","triangulation","triangulation"],["bundle-adjust","bundle","bundle"],["still-window","stationary","still-window"]]) {
        await page.selectOption("#geometryOperation",mode);if(mode==="bundle-adjust")await page.fill("#bundleIterations","50");
        await page.setInputFiles("#triangulationInput",source(name+"-input.json"));await run();
        assert.deepEqual(parse(await download(downloadName+".json")),parse(fs.readFileSync(source(name+"-native.json"),"utf8")));
      }
      assert.deepEqual(errors,[]);assert.ok(requests.every(request=>request.url.startsWith(origin+"/")&&request.method==="GET"));
      assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0);
      console.log("PASS still-window: five complete original native/WASM reports, exact source/report/hash/filename/full-u64 identity, motion/weak/input negatives, real cancellation, three-mode mobile round trip, no capture/readiness promotion/uploads/storage/page errors.");
      return;
    }
    if(bundleOnly) {
      await page.setInputFiles("#triangulationNoise",{name:"old-noise.json",mimeType:"application/json",buffer:Buffer.from("{}")});
      await page.selectOption("#geometryOperation","bundle-adjust");
      assert.equal(await page.isVisible("#geometryNoiseControls"),false);
      assert.equal(await page.inputValue("#triangulationNoise"),"");
      for(const [name,iterations] of [["accepted","50"],["budget","1"],["held-out","50"],["degenerate","50"],["full-width","50"]]) {
        console.log("Bundle case:",name,iterations);
        await page.setInputFiles("#triangulationInput",source(name+"-input.json"));await page.fill("#bundleIterations",iterations);await run();
        assert.equal(await page.isVisible("#triangulationResult"),true,await page.textContent("#triangulationError"));
        const report=await download("bundle.json");assert.deepEqual(parse(report),parse(fs.readFileSync(source(name+"-native.json"),"utf8")),name+" complete native bundle equality");
        assert.equal(await download("bundle-input.json"),fs.readFileSync(source(name+"-input.json"),"utf8"));
        assert.equal(await download("bundle-iterations.txt"),iterations);
        assert.equal(await page.textContent("#triangulationReport"),report);
        if(name==="full-width") {
          assert.ok(report.includes('"timestamp_us":18446744073709551615'));
          assert.ok(report.includes('"end_us":18446744073709551615'));
        }
        if(parse(report).solution===null)assert.match(await page.textContent("#triangulationPoint"),/No optimized geometry.*initial guesses/);
        else assert.match(await page.textContent("#triangulationPoint"),/Refined landmarks/);
        assert.equal(await page.isDisabled("#openWorkbench"),true);
      }
      await page.setViewportSize({width:390,height:844});
      await page.setInputFiles("#triangulationInput",source("accepted-input.json"));await page.fill("#bundleIterations","50");
      let release,arrived;const reached=new Promise(resolve=>{arrived=resolve;});
      const hold=route=>{release=route;arrived();};await context.route("**/ftx_wasm_bg.wasm",hold);
      const requested=page.waitForRequest(request=>request.url().endsWith("/ftx_wasm_bg.wasm"),{timeout:30000});
      await page.click("#triangulationRun");await requested;await reached;
      await page.fill("#bundleIterations","1");assert.equal(await page.locator("#triangulationDownloads a").count(),0);
      await release.continue().catch(()=>{});await context.unroute("**/ftx_wasm_bg.wasm",hold);
      await run();assert.deepEqual(parse(await download("bundle.json")),parse(fs.readFileSync(source("budget-native.json"),"utf8")));
      for(const iterations of ["1.5","-1","0","101",""]) {
        await page.fill("#bundleIterations",iterations);await run();
        assert.equal(await page.isVisible("#triangulationResult"),false);assert.equal(await page.locator("#triangulationDownloads a").count(),0);
        assert.match(await page.textContent("#triangulationError"),/iterations/);
      }
      await page.fill("#bundleIterations","50");
      for(const buffer of [Buffer.from([255]),Buffer.from('\ufeff{}'),Buffer.alloc(256*1024+1,32)]) {
        await page.setInputFiles("#triangulationInput",{name:"bad-bundle.json",mimeType:"application/json",buffer});await run();
        assert.equal(await page.isVisible("#triangulationResult"),false);assert.equal(await page.locator("#triangulationDownloads a").count(),0);
        assert.notEqual(await page.textContent("#triangulationError"),"");
      }
      await page.fill("#bundleIterations","1.5");await page.selectOption("#geometryOperation","triangulate");
      assert.equal(await page.isDisabled("#bundleIterations"),true);
      await page.setInputFiles("#triangulationInput",source("triangulation-input.json"));await run();
      assert.deepEqual(parse(await download("triangulation.json")),parse(fs.readFileSync(source("triangulation-native.json"),"utf8")));
      await page.selectOption("#geometryOperation","bundle-adjust");assert.equal(await page.locator("#triangulationDownloads a").count(),0);
      await page.fill("#bundleIterations","50");await page.setInputFiles("#triangulationInput",source("accepted-input.json"));await run();
      assert.deepEqual(parse(await download("bundle.json")),parse(fs.readFileSync(source("accepted-native.json"),"utf8")));
      assert.deepEqual(errors,[]);assert.ok(requests.every(request=>request.url.startsWith(origin+"/")&&request.method==="GET"));
      assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0);
      console.log("PASS bundle: five complete native/WASM reports, exact source/report/limit downloads, full-u64 frames, rank/held-out/budget/invalid-limit/input failures, cancellation on budget edits, mobile controls, triangulation round trip, no uploads/storage/page errors or measurement promotion.");
      return;
    }
    for(const [name,noise] of [["plain",false],["noise",true],["unavailable",true],["weak",false],["held-out",false],["full-width",true]]) {
      await select(name,noise);await run();
      assert.equal(await page.isVisible("#triangulationResult"),true,await page.textContent("#triangulationError"));
      const report=await download("triangulation.json"),expected=fs.readFileSync(source(name+"-native.json"),"utf8");
      assert.deepEqual(parse(report),parse(expected),name+" complete native report equality");
      assert.equal(await page.textContent("#triangulationReport"),report);
      assert.equal(await download("triangulation-input.json"),fs.readFileSync(source(name+"-input.json"),"utf8"));
      if(noise) assert.equal(await download("triangulation-click-noise.json"),fs.readFileSync(source(name+"-noise.json"),"utf8"));
      else assert.equal(await page.locator('a[download="triangulation-click-noise.json"]').count(),0);
      if(name==="full-width")assert.ok(report.includes('"timestamp_us":18446744073709551615'));
      if(name==="unavailable")assert.match(await page.textContent("#triangulationStatus"),/requested click uncertainty is unavailable/);
      if(name==="weak"||name==="held-out")assert.match(await page.textContent("#triangulationPoint"),/No usable point/);
      assert.equal(await page.isDisabled("#openWorkbench"),true,"Geometry creates no photo-measurement handoff");
    }
    await page.setViewportSize({width:390,height:844});
    await page.click("#triangulationClearNoise");assert.equal(await page.isVisible("#triangulationResult"),false);
    assert.equal(await page.locator("#triangulationDownloads a").count(),0);
    await select("plain",false);await run();assert.equal(await page.isVisible("#triangulationResult"),true);
    for(const bad of [{name:"noise.json",mimeType:"application/json",buffer:Buffer.from("{}")},
      {name:"empty.json",mimeType:"application/json",buffer:Buffer.alloc(0)},
      {name:"bad-utf8.json",mimeType:"application/json",buffer:Buffer.from([255])},
      {name:"oversized.json",mimeType:"application/json",buffer:Buffer.alloc(256*1024+1,32)}]) {
      await page.setInputFiles("#triangulationNoise",bad);await run();
      assert.equal(await page.isVisible("#triangulationResult"),false);assert.equal(await page.locator("#triangulationDownloads a").count(),0);
      assert.notEqual(await page.textContent("#triangulationError"),"");
    }
    await select("noise",true);
    let release,arrived;const reached=new Promise(resolve=>{arrived=resolve;});
    const hold=route=>{release=route;arrived();};await context.route("**/ftx_wasm_bg.wasm",hold);
    const requested=page.waitForRequest(request=>request.url().endsWith("/ftx_wasm_bg.wasm"),{timeout:30000});
    await page.click("#triangulationRun");await requested;await reached;
    await page.click("#triangulationCancel");assert.equal(await page.locator("#triangulationDownloads a").count(),0);
    await release.continue().catch(()=>{});await context.unroute("**/ftx_wasm_bg.wasm",hold);
    await page.setInputFiles("#triangulationInput",source("full-width-input.json"));await run();
    assert.equal(await page.isVisible("#triangulationResult"),false);
    assert.equal(await page.locator("#triangulationDownloads a").count(),0,"Mismatched retained noise refuses instead of reviving prior output");
    await select("plain",false);await run();assert.equal(await page.isVisible("#triangulationResult"),true);
    assert.deepEqual(errors,[]);assert.ok(requests.every(request=>request.url.startsWith(origin+"/")&&request.method==="GET"));
    assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0);
    console.log("PASS triangulation: six complete native/WASM reports, exact source/noise/report downloads, full-u64 frames, accepted and unavailable precision, weak/held-out/noise/encoding/budget refusals, cancellation, desktop/mobile controls, no measurement promotion/uploads/storage/page errors.");
  } finally {await context.close();}
}
async function bodyModelFileHandoff(browser,origin,received=false) {
  const context=await browser.newContext({userAgent:'OpenAI File Downloader, XaiImageApiFetch/1.0'}),errors=[],requests=[];
  context.on('page',p=>p.on('pageerror',e=>errors.push(String(e))));
  context.on('request',r=>requests.push({url:r.url(),method:r.method()}));
  const original=fs.readFileSync(path.join(native,'model.ftxbody')),model=path.join(work,received?'received-model.ftxbody':'manual-model.ftxbody');
  fs.writeFileSync(model,original,{flag:'wx'});
  try {
    let page=await context.newPage();await page.goto(origin+'/index.html');
    await page.waitForFunction(()=>/^Ready/.test(document.getElementById('status').textContent));
    await page.click('#go');
    await page.waitForFunction(()=>!document.getElementById('results').classList.contains('hidden') || !document.getElementById('errorBox').classList.contains('hidden'));
    assert.equal(await page.locator('#errorBox').isVisible(),false,await page.textContent('#errorText'));
    await page.setInputFiles('#bodyFile',model);
    await page.waitForFunction(()=>document.getElementById('status').textContent==='Body model loaded.');
    const source=page;
    if(received) {
      const popup=source.waitForEvent('popup');
      await source.evaluate(({token,report})=>{
        let target;
        const send=event=>{
          if(event.origin!==location.origin || event.source!==target || event.data?.type!=='ftx-body-fit-sources-ready' || event.data.token!==token)return;
          window.removeEventListener('message',send);
          target.postMessage({type:'ftx-body-fit-sources',token,fit:new Blob([report],{type:'application/json'}),
            model:document.getElementById('bodyFile').files[0]},location.origin);
        };
        window.addEventListener('message',send);
        target=window.open('index.html?bodyFitImport='+token,'_blank');
      },{token:'f'.repeat(32),report:fs.readFileSync(path.join(native,'accepted-report.json'),'utf8')});
      page=await popup;
      await page.waitForFunction(()=>document.getElementById('status').textContent==='Body model loaded.');
      await page.click('#go');
      await page.waitForFunction(()=>!document.getElementById('results').classList.contains('hidden'));
      await page.locator('#bodyFitConsent').evaluate(e=>{e.closest('details').open=true;});
      await page.check('#bodyFitConsent');
    }
    fs.writeFileSync(model,Buffer.from('changed after the worker loaded the model'));
    const reread=await source.evaluate(async()=>{
      try {return [...new Uint8Array(await document.getElementById('bodyFile').files[0].arrayBuffer())];}
      catch {return null;}
    });
    assert.notDeepEqual(reread,[...original],'The selected File no longer supplies the evaluated bytes');
    if(received) {
      const event=page.waitForEvent('download');
      await page.click('#bodyFitImportFiles a[download="body-model.ftxbody"]');const file=await event;
      assert.equal(await file.failure(),null);assert.deepEqual(fs.readFileSync(await file.path()),original);
    }
    const [receiver]=await Promise.all([page.waitForEvent('popup'),page.click('#chooseMaterials')]);
    await receiver.waitForFunction(()=>/Received pattern.json|handoff refused/.test(document.getElementById('roundImportText').textContent));
    assert.match(await receiver.textContent('#roundImportText'),/^Received pattern.json/);
    assert.equal(await receiver.evaluate(()=>window.opener),null);
    assert.equal(await receiver.isChecked('#bodyFitConsent'),false);
    const event=receiver.waitForEvent('download');
    await receiver.click('#receivedBodyFiles a[download="body-model.ftxbody"]');const file=await event;
    assert.equal(await file.failure(),null);assert.deepEqual(fs.readFileSync(await file.path()),original);
    assert.deepEqual(errors,[]);assert.deepEqual(requests.filter(r=>{
      const url=new URL(r.url);
      return !['http:','blob:'].includes(url.protocol)||url.origin!==origin||r.method!=='GET';
    }),[],'Model requests and Blob downloads stay on the exact local origin and are read-only');
    console.log(`PASS ${received?'received':'manual'} model handoff: selected File changed after load; source downloads and Materials retained the exact evaluated bytes.`);
  } finally {await context.close();}
}
async function bodyFitWorkflow(browser,origin) {
  const context=await browser.newContext({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"}),errors=[],requests=[];
  const page=await context.newPage();page.setDefaultTimeout(60000);
  page.on('pageerror',e=>errors.push(String(e)));context.on('request',r=>requests.push({url:r.url(),method:r.method()}));
  const parse=text=>JSON.parse(text,(key,value,meta)=>key==='timestamp_us'?meta.source:value);
  const download=async name=>{
    const event=page.waitForEvent('download');await page.click(`a[download="${name}"]`);const result=await event;
    assert.equal(await result.failure(),null);return fs.readFileSync(await result.path(),'utf8');
  };
  const run=async()=>{
    await page.click('#bodyFitRun');
    await page.waitForFunction(()=>/Body fit accepted|Body fit finished|Body fit refused|No body-fit report|Body-fit worker stopped/.test(document.getElementById('bodyFitStatus').textContent),null,{timeout:120000});
  };
  const absent=async()=>{
    assert.equal(await page.locator('#bodyFitResult').isVisible(),false);
    assert.equal(await page.locator('#bodyFitDownloads a').count(),0);
  };
  try {
    await page.goto(origin+'/photo.html');
    await page.setInputFiles('#bodyFitModel',path.join(native,'model.ftxbody'));
    await page.locator('#bodyFitIterations').evaluate(e=>{e.closest('details').open=true;});
    for(const item of JSON.parse(fs.readFileSync(path.join(native,'cases.json'),'utf8')).cases) {
      await page.setInputFiles('#bodyFitInput',path.join(native,item.input));await absent();
      await page.fill('#bodyFitIterations',item.iterations);await page.fill('#bodyFitEvaluations',item.max_evaluations);
      await run();assert.equal(await page.textContent('#bodyFitError'),'');
      const report=await download('body-fit-report.json');
      assert.deepEqual(parse(report),parse(fs.readFileSync(path.join(native,item.report),'utf8')),item.name+' actual page/worker/WASM/native equality');
      assert.equal(await download('body-fit-input.json'),fs.readFileSync(path.join(native,item.input),'utf8'));
      assert.equal(await page.locator('#bodyFitAttempts tr').count(),JSON.parse(report).attempts.length);
      assert.equal(await page.locator('#bodyFitChecks tr').count(),JSON.parse(report).attempts.reduce((n,a)=>n+a.checks.length,0));
      assert.match(await page.textContent('#bodyFitSummary'),new RegExp(item.status));
      assert.equal(await page.isDisabled('#openWorkbench'),true,'Model predictions are never photo measurement records');
      if(item.status!=='ACCEPTED')assert.match(await page.textContent('#bodyFitUse'),/No accepted shape/);
      if(item.name==='accepted') {
        await page.screenshot({path:path.join(work,'body-fit-accepted.png'),fullPage:true});
        await page.setViewportSize({width:390,height:844});
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),true,'Accepted report stays within the narrow viewport');
        await page.screenshot({path:path.join(work,'body-fit-mobile.png'),fullPage:true});
        await page.setViewportSize({width:1280,height:720});
      }
    }
    // Cancel a real worker while its module is loading, then finish a fresh request.
    let parked,release;const arrived=new Promise(resolve=>{parked=resolve;});
    await context.route('**/ftx_wasm_bg.wasm',route=>{release=route;parked();});
    await page.setInputFiles('#bodyFitInput',path.join(native,'accepted-input.json'));
    await page.fill('#bodyFitIterations','50');await page.fill('#bodyFitEvaluations','100000');
    await page.click('#bodyFitRun');await arrived;await page.click('#bodyFitCancel');await absent();
    await release.continue().catch(()=>{});await context.unroute('**/ftx_wasm_bg.wasm');
    await run();assert.match(await page.textContent('#bodyFitSummary'),/ACCEPTED/);
    await page.fill('#bodyFitIterations','49');await absent();
    await page.setInputFiles('#bodyFitModel',{name:'wrong.ftxbody',mimeType:'application/octet-stream',buffer:Buffer.from('wrong model')});
    await run();assert.match(await page.textContent('#bodyFitError'),/SHA-256 mismatch/);await absent();
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),true,'Narrow layout stays within the viewport');
    assert.equal(await page.evaluate(()=>localStorage.length+sessionStorage.length),0);
    await page.setInputFiles('#bodyFitModel',path.join(native,'model.ftxbody'));
    await page.fill('#bodyFitIterations','50');await run();
    assert.match(await page.textContent('#bodyFitSummary'),/ACCEPTED/);
    const originalReport=await download('body-fit-report.json');
    const [receiver]=await Promise.all([page.waitForEvent('popup'),page.click('#bodyFitWorkbench')]);
    receiver.on('pageerror',e=>errors.push(String(e)));
    const received=async name=>{
      const event=receiver.waitForEvent('download');await receiver.click(`#bodyFitImportFiles a[download="${name}"]`);
      const file=await event;assert.equal(await file.failure(),null);return fs.readFileSync(await file.path());
    };
    try {
      await receiver.waitForFunction(()=>document.getElementById('status').textContent==='Body model loaded.');
      assert.equal(await receiver.evaluate(()=>window.opener),null);
      assert.equal(await receiver.locator('#results').isVisible(),false);
      assert.equal(await receiver.isChecked('#bodyFitConsent'),false);
      assert.equal((await received('body-fit-report.json')).toString(),originalReport);
      assert.deepEqual(await received('body-model.ftxbody'),fs.readFileSync(path.join(native,'model.ftxbody')));
      await page.fill('#bodyFitIterations','49');await absent();
      assert.equal((await received('body-fit-report.json')).toString(),originalReport,'Received evidence remains an immutable snapshot');
      await receiver.click('#clearBodyFitImport');
      assert.equal(await receiver.locator('#bodyFitImportFiles a').count(),0);
      assert.equal(await receiver.isChecked('#bodyFitConsent'),false);
    } finally {await receiver.close();}
    await page.fill('#bodyFitIterations','50');await run();
    let pendingRoute,handoffArrived;const handoffParked=new Promise(resolve=>{handoffArrived=resolve;});
    const importRoute='**/index.html?bodyFitImport=*';
    await context.route(importRoute,route=>{pendingRoute=route;handoffArrived();});
    const opening=page.waitForEvent('popup');await page.click('#bodyFitWorkbench');
    let handoffTimeout;
    try {await Promise.race([handoffParked,new Promise((_,reject)=>{handoffTimeout=setTimeout(()=>reject(Error('Body handoff did not reach its receiver')),30000);})]);}
    finally {clearTimeout(handoffTimeout);}
    await page.fill('#bodyFitIterations','49');await absent();await pendingRoute.continue();
    const stale=await opening;stale.on('pageerror',e=>errors.push(String(e)));
    try {
      await stale.waitForFunction(()=>document.getElementById('bodyFitImportText').textContent.includes('Body fit changed before transfer'));
      assert.equal(await stale.evaluate(()=>window.opener),null);
      assert.equal(await stale.locator('#bodyFitImportFiles a').count(),0);
      assert.equal(await stale.locator('#results').isVisible(),false);
      assert.equal(await stale.isChecked('#bodyFitConsent'),false);
    } finally {await stale.close();await context.unroute(importRoute);}
    assert.deepEqual(await page.evaluate(()=>Object.fromEntries(Object.entries(localStorage))),
      {'fab-wb-program':'"zouave_bolero"','fab-wb-bodyMode':'"chart"'},'Only ordinary workbench preferences are stored');
    assert.equal(await page.evaluate(()=>sessionStorage.length),0);
    await page.locator('#bodyFitSaved').evaluate(e=>{e.closest('details').open=true;});
    await page.setInputFiles('#bodyFitSaved',{name:'saved-fit.json',mimeType:'application/json',buffer:Buffer.from(originalReport)});
    await page.click('#bodyFitOpen');
    await page.waitForFunction(()=>/Saved accepted fit loaded|Saved fit refused|No body-fit report|Saved-fit loading stopped/.test(document.getElementById('bodyFitStatus').textContent));
    assert.equal(await page.textContent('#bodyFitError'),'');
    assert.match(await page.textContent('#bodyFitSummary'),/^Saved report: ACCEPTED/);
    assert.equal(await download('body-fit-report.json'),originalReport);
    assert.equal(await page.locator('#bodyFitReviewCanvas').isVisible(),true);
    assert.equal(await page.locator('#bodyFitDownloads a').count(),1,'Saved fits do not reconstruct original input or work options');
    assert.deepEqual(errors,[]);assert.ok(requests.every(r=>r.url.startsWith(origin+'/')&&r.method==='GET'));
    console.log('PASS actual browser body fit: complete native equality, all starts/checks, exact downloads and accepted-body popup sources, immutable received evidence, stale-transfer cancellation, explicit wearer consent, no measurement promotion, no uploads or stored evidence. Artifacts:',work);
  } finally {await context.close();}
}
const server=http.createServer((req,res)=>{
  let file,base=web;
  try {
    let pathname=decodeURIComponent(new URL(req.url,"http://localhost").pathname);
    if(pathname.startsWith("/pkg/")) {base=pkg;pathname=pathname.slice(4);}
    file=path.resolve(base,"."+pathname);
  } catch {res.writeHead(400).end();return;}
  if(req.method!=="GET" || !file.startsWith(base+path.sep)) {res.writeHead(403).end();return;}
  fs.readFile(file,(error,data)=>{
    if(error) {res.writeHead(404).end();return;}
    const mime={".html":"text/html",".js":"text/javascript",".wasm":"application/wasm",".png":"image/png"}[path.extname(file)] || "application/octet-stream";
    res.writeHead(200,{"Content-Type":mime});res.end(data);
  });
});
(async()=>{
  await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
  const origin=`http://127.0.0.1:${server.address().port}`;
  const browser=await chromium.launch(process.env.FTX_CHROMIUM_EXECUTABLE?{executablePath:process.env.FTX_CHROMIUM_EXECUTABLE}:{}), page=await browser.newPage({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
  const errors=[],requests=[];
  page.on("pageerror",e=>errors.push(String(e)));
  page.context().on("request",r=>requests.push({url:r.url(),method:r.method()}));
  const upload=async name=>page.setInputFiles("#photo",path.join(native,name));
  const action=async id=>{
    await page.click("#"+id);
    await page.waitForFunction(()=>/Review the detected|Reference rejected;|No measurements published\.|Photo-estimated records available/.test(document.getElementById("status").textContent),null,{timeout:30000});
  };
  const download=async name=>{
    const event=page.waitForEvent("download"); await page.click(`a[download="${name}"]`);
    const d=await event; assert.equal(d.suggestedFilename(),name);assert.equal(await d.failure(),null);
    return fs.readFileSync(await d.path());
  };
  const absent=async()=>assert.equal(await page.locator('a[download="photo-measurements.json"]').count(),0);
  const expected=(name,measured)=>{
    const value=JSON.parse(fs.readFileSync(path.join(native,name),"utf8")).result;
    delete (measured?value.photo.image_input:value.image_input).path;
    return value;
  };
  try {
    if(sectionOnly) {await sectionWorkflow(browser,origin);return;}
    if(bodyFitOnly) {await bodyFitWorkflow(browser,origin);await bodyModelFileHandoff(browser,origin);await bodyModelFileHandoff(browser,origin,true);return;}
    if(triangulationOnly||bundleOnly||stillOnly) {await triangulationWorkflow(browser,origin);return;}
    await page.goto(origin+"/photo.html");
    if(charucoOnly) {
      const sheetName=fs.existsSync(path.join(native,'calib-sheet-letter.json'))?'calib-sheet-letter':'calib-sheet-a4';
      const source=fs.readFileSync(path.join(native,sheetName+'.json'),'utf8'),sheet=JSON.parse(source);
      const charucoExpected=measured=>{
        const value=expected(measured?'native-measurements.json':'native-scale.json',measured);
        const receipt=(measured?value.photo:value).image_input.charuco_detection;
        // The CLI additionally annotates filesystem sources and its embedded optional adapter.
        assert.equal(receipt.sheet_path,path.join(native,sheetName+'.json'));
        assert.equal(receipt.adapter_sha256,sha256(fs.readFileSync(path.join(root,'crates/ftx-cli/src/measure/charuco_detector.py'))));
        delete receipt.sheet_path;delete receipt.adapter_sha256;
        return value;
      };
      await page.selectOption('#referenceMode','charuco');await upload(sheetName+'.png');
      await page.setInputFiles('#sheet',path.join(native,sheetName+'.json'));await action('analyze');
      assert.equal(await page.textContent('#error'),'');
      const scale=JSON.parse(await download('photo-scale.json'));
      assert.deepEqual(scale,charucoExpected(false));assert.equal(scale.ok,true);
      assert.equal(scale.image_input.charuco_detection.implementation.dictionary,sheet.board.dictionary);
      assert.equal(scale.image_input.charuco_detection.sheet_sha256,sha256(source));
      assert.equal((await download('photo-sheet.json')).toString(),source);
      await page.fill('#points',fs.readFileSync(path.join(native,'points.json'),'utf8'));
      await page.setInputFiles('#optionsFile',{name:'options.json',mimeType:'application/json',buffer:Buffer.from('{}')});
      await page.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Settings loaded'));
      await action('measure');assert.equal(await page.textContent('#error'),'');
      const measured=JSON.parse(await download('photo-measurements.json'));
      assert.deepEqual(measured,charucoExpected(true));
      assert.ok(Math.abs(measured.records[0].value_mm-88)<.2);assert.equal(measured.records[0].provenance,'PHOTO_ESTIMATED');
      const wrong=structuredClone(sheet);wrong.board.dictionary=sheet.board.dictionary.startsWith('DICT_7X7_')?'DICT_5X5_50':'DICT_7X7_50';
      await page.setInputFiles('#sheet',{name:'wrong.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(wrong))});
      await absent();assert.equal(await page.isDisabled('#measure'),true);await action('analyze');
      const wrongScale=await page.locator('a[download="photo-scale.json"]').count()?JSON.parse(await download('photo-scale.json')):null;
      if(wrongScale)assert.equal(wrongScale.ok,false);else assert.notEqual(await page.textContent('#error'),'');
      assert.equal(await page.isDisabled('#measure'),true);await absent();
      assert.deepEqual(errors,[]);assert.ok(requests.every(r=>r.url.startsWith(origin+'/')&&r.method==='GET'));
      assert.equal(await page.evaluate(()=>localStorage.length),0);
      console.log('PASS printed dictionary -> real photo worker -> exact native scale/measurement reports, original sheet receipt, mismatch refusal and stale-output clearing:',sheet.board.dictionary,work);
      return;
    }
    let annotationDraft;
    for(const [file,tag,width,height,end,knownLength] of [["photo.png","png",720,960,630,400],["photo.jpg","jpeg",600,800,530,250]]) {
      await upload(file); await absent(); assert.equal(await page.isDisabled("#measure"),true);
      await page.selectOption("#paper","letter"); await action("analyze");
      assert.equal(await page.textContent("#error"),"");
      assert.deepEqual(await page.locator("#image").evaluate(c=>[c.width,c.height]),[width,height]);
      const scale=JSON.parse(await download("photo-scale.json"));
      assert.deepEqual(scale,expected(`native-scale-${tag}.json`,false));
      assert.equal(await page.locator("#saveDraft").count(),1,"Save annotations before measurement succeeds");
      assert.equal(await page.locator("#cancelPoint").count(),1,"Cancel a first click without clearing saved pairs");
      assert.equal(scale.ok,true); if(tag==="jpeg")assert.equal(scale.image_input.exif_orientation,6);
      const box=await page.locator("#image").boundingBox();
      const click=async x=>page.locator("#image").click({position:{x:(x+.5)*box.width/width,y:400.5*box.height/height}});
      await click(30);assert.equal(await page.isDisabled("#measure"),true);
      const typed=fs.readFileSync(path.join(native,`points-${tag}.json`),"utf8");
      await page.fill("#points",typed);assert.equal(await page.isDisabled("#measure"),false);
      await click(40);assert.equal(await page.inputValue("#points"),typed);
      assert.equal(await page.isDisabled("#measure"),true);
      await page.click("#cancelPoint");assert.equal(await page.inputValue("#points"),typed);
      assert.equal(await page.isDisabled("#cancelPoint"),true);assert.equal(await page.isDisabled("#measure"),false);
      await click(40);
      await page.click("#clearPoints");assert.equal(await page.isDisabled("#measure"),false);
      for(const x of [30,end]) await page.locator("#image").click({position:{x:(x+.5)*box.width/width,y:400.5*box.height/height}});
      const clicked=JSON.parse(await page.inputValue("#points")).across_back;
      assert.ok(clicked,`Canvas pair missing: ${await page.textContent("#clickStatus")} ${await page.textContent("#error")}`);
      assert.equal(clicked.length,2);
      for(const [i,x] of [30,end].entries()) {
        assert.ok(Math.abs(clicked[i][0]-x)<=1 && Math.abs(clicked[i][1]-400)<=1,"Scaled canvas clicks retain full-resolution pixel coordinates within pointer quantization");
      }
      await action("measure");
      const clickedReport=JSON.parse(await download("photo-measurements.json"));
      assert.ok(Math.abs(clickedReport.records[0].value_mm-knownLength)<1);
      assert.equal(clickedReport.records[0].provenance,"PHOTO_ESTIMATED");
      const points=fs.readFileSync(path.join(native,`points-${tag}.json`),"utf8");
      await page.fill("#points",points); await absent(); await action("measure");
      const rawReport=await download("photo-measurements.json"), report=JSON.parse(rawReport);
      assert.deepEqual(report,expected(`native-measure-${tag}.json`,true));
      assert.equal((await download("photo-points.json")).toString(),points);
      assert.deepEqual(JSON.parse(await download("photo-options.json")),{depth_offset_mm:0,camera_distance_mm:2500,click_px:3,sheet_level_sigma_deg:1.5,bootstrap:200,seed:0});
      assert.ok(report.records[0].sigma_mm>0); assert.equal(report.taken_on,null);
      assert.equal(await page.locator("#optionsFile").count(),1,"Replay saved measurement settings directly");
      const savedOptions=' {\r\n"depth_offset_mm":0,"camera_distance_mm":2500,"click_px":3,"sheet_level_sigma_deg":1.5,"bootstrap":200,"seed":0\r\n}\r\n';
      const loadOptions=async text=>{
        await page.setInputFiles("#optionsFile",{name:"photo-options.json",mimeType:"application/json",buffer:Buffer.from(text)});
        await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Settings loaded"));
      };
      await page.fill("#click","9");await loadOptions(savedOptions);await absent();
      assert.equal(await page.isDisabled("#click"),true);assert.equal(await page.inputValue("#click"),"9");
      assert.equal(await page.textContent("#optionsSource"),savedOptions);
      await action("measure");assert.deepEqual(JSON.parse(await download("photo-measurements.json")),report);
      assert.equal((await download("photo-options.json")).toString(),savedOptions,"Exact imported settings override inactive form values");
      await loadOptions('{"bootstrap":200,"seed":18446744073709551615}');await action("measure");
      const seeded=(await download("photo-measurements.json")).toString();
      assert.match(seeded,/"seed":\s*18446744073709551615/,'Full-width seed reaches Rust without JavaScript rounding');
      assert.equal(JSON.parse(seeded).records[0].value_mm,report.records[0].value_mm);
      await loadOptions('{"bootstrap":19}');await action("measure");await absent();
      assert.match(await page.textContent("#error"),/bootstrap/,'Imported invalid settings refuse without falling back to form values');
      await page.click("#useFormOptions");assert.equal(await page.isDisabled("#click"),false);
      assert.equal(await page.inputValue("#click"),"9");await page.fill("#click","3");
      assert.equal(await page.locator("#pointFile").count(),1,"Replay a saved point file directly");
      const saved=' \r\n'+points.replace(/\r?\n/g,'\r\n')+'\r\n';
      await page.setInputFiles("#pointFile",{name:"saved-points.json",mimeType:"application/json",buffer:Buffer.from(saved)});
      await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Points loaded"));
      assert.equal(await page.inputValue("#points"),saved.replace(/\r\n/g,'\n'));
      assert.equal(await page.locator("#pointFile").evaluate(e=>e.files.length),0);
      await absent();await action("measure");assert.equal(await page.textContent("#error"),"");
      assert.deepEqual(JSON.parse(await download("photo-measurements.json")),report);
      assert.equal((await download("photo-points.json")).toString(),saved,"Unedited import retains original CRLF source bytes");
      const beforeWrongFile=await page.inputValue("#points");
      await page.setInputFiles("#pointFile",{name:"measurements-not-points.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify(report))});
      await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Points file not loaded"));
      assert.equal(await page.inputValue("#points"),beforeWrongFile);await absent();
      await action("measure");assert.deepEqual(JSON.parse(await download("photo-measurements.json")),report);
      assert.equal((await download("photo-points.json")).toString(),saved,"A rejected import preserves the original source too");
      await loadOptions('{"bootstrap":19}');await action("measure");await absent();
      assert.match(await page.textContent("#error"),/bootstrap/);
      await page.click("#saveDraft");
      const refusedDraft=JSON.parse(await download("photo-annotations.json"));
      assert.equal(refusedDraft.points,saved);assert.equal(refusedDraft.options,'{"bootstrap":19}');
      assert.equal(refusedDraft.scale,await page.textContent("#scale"));await absent();
      await page.click("#useFormOptions");await page.click("#saveDraft");
      const draftBytes=await download("photo-annotations.json"),draft=JSON.parse(draftBytes);
      annotationDraft=draftBytes;
      assert.equal(draft.schema,"ftx-photo-annotations/1");assert.equal(draft.points,saved);
      const restore=async()=>{
        await page.setInputFiles("#draftFile",{name:"photo-annotations.json",mimeType:"application/json",buffer:draftBytes});
        await page.waitForFunction(()=>/Annotation draft (loaded|not loaded)/.test(document.getElementById("status").textContent));
      };
      await page.reload();await upload(file);await page.selectOption("#paper","letter");await action("analyze");
      await restore();assert.match(await page.textContent("#status"),/draft loaded/);await absent();
      assert.equal(await page.inputValue("#points"),saved.replace(/\r\n/g,'\n'));assert.equal(await page.isDisabled("#click"),true);
      await action("measure");assert.deepEqual(JSON.parse(await download("photo-measurements.json")),report);
      assert.equal((await download("photo-points.json")).toString(),saved);
      assert.equal((await download("photo-options.json")).toString(),draft.options);
      await upload(tag==="png"?"photo.jpg":"photo.png");await action("analyze");
      const otherPoints='{"arm_length":[[5,6],[9,10]]}';await page.fill("#points",otherPoints);await loadOptions('{"seed":7}');
      await restore();assert.match(await page.textContent("#status"),/draft not loaded/);
      assert.equal(await page.inputValue("#points"),otherPoints);assert.equal(await page.textContent("#optionsSource"),'{"seed":7}');await absent();
      await upload(file);await action("analyze");await restore();await action("measure");
      assert.deepEqual(JSON.parse(await download("photo-measurements.json")),report);
      await page.click("#useFormOptions");
      const annotated={points:clicked,view:"front",depth_offset_mm:0};
      await page.fill("#points",JSON.stringify({...JSON.parse(points),arm_length:annotated}));
      await page.fill("#name","across_back");await page.click("#removePair");
      const edited=await page.inputValue("#points");assert.deepEqual(JSON.parse(edited),{arm_length:annotated});
      assert.deepEqual(JSON.parse(await download("photo-scale.json")),scale);
      await absent();await action("measure");
      assert.equal(await page.textContent("#error"),"");
      const remaining=JSON.parse(await download("photo-measurements.json"));
      assert.equal(remaining.records.length,1);assert.equal(remaining.records[0].name,"arm_length");
      assert.ok(Math.abs(remaining.records[0].value_mm-knownLength)<1);assert.ok(remaining.records[0].sigma_mm>0);
      assert.equal((await download("photo-points.json")).toString(),edited);
      for(const x of [30,end])await click(x);
      assert.deepEqual(JSON.parse(await page.inputValue("#points")).arm_length,annotated);
      assert.ok(JSON.parse(await page.inputValue("#points")).across_back,"Removed name can be added again");
      fs.writeFileSync(path.join(work,`${tag}-report.json`),rawReport,{flag:"wx"});
      console.log(`PASS ${tag}: actual scale, orientation, scaled clicks, PHOTO_ESTIMATED length/uncertainty, complete native equality and exact point/options downloads`);
    }
    await page.screenshot({path:path.join(work,"photo-workbench.png"),fullPage:true});
    await page.fill("#points",'{"across_back":[[30,400],[9999,400]]}'); await absent(); await action("measure");
    assert.match(await page.textContent("#error"),/outside/); await absent();
    await page.fill("#points",fs.readFileSync(path.join(native,"points-jpeg.json"),"utf8"));
    await page.fill("#bootstrap","19"); await action("measure");
    assert.match(await page.textContent("#error"),/bootstrap/); await absent();
    await page.fill("#bootstrap","200"); await action("measure");
    // Delay the actual module response to exercise cancellation/replacement while work is pending.
    for(const change of ["cancel","options","points","source","paper","removePair","importOptions","useFormOptions","draft"]) {
      if(await page.isDisabled("#measure")) {await upload("photo.jpg");await page.selectOption("#paper","letter");await action("analyze");await page.fill("#points",fs.readFileSync(path.join(native,"points-jpeg.json"),"utf8"));}
      if(change==="useFormOptions") {
        await page.setInputFiles("#optionsFile",{name:"saved.json",mimeType:"application/json",buffer:Buffer.from('{"bootstrap":100}')});
        await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Settings loaded"));
      }
      let release, arrived;
      const parked=new Promise(resolve=>{arrived=resolve;});
      const route=async r=>{release=r;arrived();};
      await page.context().route("**/ftx_wasm_bg.wasm",route);
      await page.click("#measure");
      let timeout;
      try {await Promise.race([parked,new Promise((_,reject)=>{timeout=setTimeout(()=>reject(Error("Worker module request was not intercepted")),30000);})]);}
      finally {clearTimeout(timeout);}
      if(change==="cancel")await page.click("#cancel");
      if(change==="options")await page.fill("#distance","2600");
      if(change==="points")await page.fill("#points",'{"across_back":[[31,400],[530,400]]}');
      if(change==="source")await upload("photo.png");
      if(change==="paper")await page.selectOption("#paper","a4");
      if(change==="removePair")await page.click("#removePair");
      if(change==="importOptions") {
        await page.setInputFiles("#optionsFile",{name:"saved.json",mimeType:"application/json",buffer:Buffer.from('{"bootstrap":100}')});
        await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Settings loaded"));
      }
      if(change==="useFormOptions")await page.click("#useFormOptions");
      if(change==="draft") {
        await page.setInputFiles("#draftFile",{name:"photo-annotations.json",mimeType:"application/json",buffer:annotationDraft});
        await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Annotation draft loaded"));
      }
      await absent(); assert.equal(await page.isDisabled("#cancel"),true);
      await release.continue().catch(()=>{}); await page.context().unroute("**/ftx_wasm_bg.wasm",route);
      await absent();
      if(!await page.isDisabled("#useFormOptions"))await page.click("#useFormOptions");
    }
    await page.selectOption("#paper","a4"); await upload("photo.png"); await action("analyze");
    assert.equal(JSON.parse(await download("photo-scale.json")).ok,false);
    assert.equal(await page.isDisabled("#measure"),true);await absent();
    await page.selectOption("#paper","auto"); await upload("ambiguous.png"); await action("analyze");
    const ambiguous=JSON.parse(await download("photo-scale.json"));
    assert.equal(ambiguous.ok,false); assert.match(ambiguous.warnings.join(" "),/PAPER_AMBIGUOUS/);
    assert.equal(await page.isDisabled("#measure"),true);await absent();
    for(const file of ["blank.png","transparent.png"]) {
      await upload(file);await action("analyze");await absent();assert.equal(await page.isDisabled("#measure"),true);
      if(file==="transparent.png")assert.match(await page.textContent("#error"),/opaque/);
    }
    await page.setInputFiles("#photo",{name:"cut.jpg",mimeType:"image/jpeg",buffer:fs.readFileSync(path.join(native,"photo.jpg")).subarray(0,1000)});
    await action("analyze");assert.notEqual(await page.textContent("#error"),"");await absent();
    // Actual generated board -> Rust detector -> the same original-pixel clicks and scale path.
    const sheetPath=path.join(web,"calibration/calib-sheet-letter.json"), boardPath=path.join(web,"calibration/calib-sheet-letter.png");
    const sheetText=fs.readFileSync(sheetPath,"utf8"), sheet=JSON.parse(sheetText), b=sheet.board;
    const pixel=(x,y)=>[b.origin_px[0]-.5+x*b.size_px[0]/b.size_mm[0],b.origin_px[1]-.5+y*b.size_px[1]/b.size_mm[1]];
    const boardPoints=JSON.stringify({across_back:[pixel(44,44),pixel(132,44)]});
    await page.selectOption("#referenceMode","charuco");
    await page.setInputFiles("#photo",boardPath);assert.equal(await page.isDisabled("#analyze"),true);
    await page.setInputFiles("#sheet",sheetPath);await action("analyze");assert.equal(await page.textContent("#error"),"");
    const boardScale=JSON.parse(await download("photo-scale.json"));
    assert.equal(boardScale.ok,true);assert.equal(boardScale.coordinate_source,"detected_charuco");assert.equal(boardScale.inliers,35);
    assert.equal(boardScale.image_input.sha256,sha256(fs.readFileSync(boardPath)));
    assert.equal(boardScale.image_input.charuco_detection.sheet_sha256,sha256(sheetText));
    assert.equal(boardScale.image_input.charuco_detection.implementation.backend,"rust_charuco");
    assert.equal((await download("photo-sheet.json")).toString(),sheetText);
    await page.fill("#points",boardPoints);await action("measure");
    const boardMeasured=JSON.parse(await download("photo-measurements.json"));
    assert.ok(Math.abs(boardMeasured.records[0].value_mm-88)<0.2);assert.equal(boardMeasured.records[0].provenance,"PHOTO_ESTIMATED");
    const lens={schema:"ftx-photo-lens/1",lens_id:"synthetic-identity",image_size_px:[2550,3300],rotation_cw_deg:0,
      calibration_evidence:"Synthetic software fixture; not a real lens",intrinsics:{focal_px:[3000,3000],principal_px:[1275,1650]}};
    const lensText=JSON.stringify(lens), setLens=async value=>page.setInputFiles("#calibration",{name:"lens.json",mimeType:"application/json",buffer:Buffer.from(value)});
    await setLens(lensText);await absent();assert.equal(await page.isDisabled("#measure"),true);
    await action("analyze");assert.match(await page.textContent("#error"),/capture_lens_id|rotation_cw_deg|lens identifier/);await absent();
    await page.fill("#lensId","synthetic-identity");await page.selectOption("#rotation","0");await action("analyze");
    assert.equal(await page.textContent("#error"),"");const calibratedScale=JSON.parse(await download("photo-scale.json"));
    assert.equal(calibratedScale.lens_input.sha256,sha256(lensText));assert.equal(calibratedScale.lens_input.recomputed,false);
    assert.equal(calibratedScale.image_input.charuco_detection.implementation.uses_lens_intrinsics,true);
    assert.ok(calibratedScale.homography_undistorted_px_to_board_mm);assert.equal(calibratedScale.homography_px_to_board_mm,undefined);
    assert.equal((await download("photo-calibration.json")).toString(),lensText);
    assert.deepEqual(JSON.parse(await download("photo-capture.json")),{capture_lens_id:"synthetic-identity",rotation_cw_deg:0});
    await page.fill("#points",boardPoints);await action("measure");
    assert.ok(Math.abs(JSON.parse(await download("photo-measurements.json")).records[0].value_mm-88)<0.2);
    // An actual capture control change withdraws both source calibration and measured outputs.
    await page.selectOption("#rotation","90");await absent();assert.equal(await page.locator('a[download="photo-scale.json"]').count(),0);
    assert.equal(await page.isDisabled("#measure"),true);await action("analyze");assert.match(await page.textContent("#error"),/rotation/);
    await page.selectOption("#rotation","0");await page.fill("#independence","A string cannot supply missing covariance");await action("analyze");
    assert.match(await page.textContent("#error"),/intrinsic uncertainty/);await page.fill("#independence","");
    await setLens(JSON.stringify({...lens,image_size_px:[1280,960]}));await action("analyze");assert.match(await page.textContent("#error"),/image_size_px/);await absent();
    await page.click("#clearCalibration");await action("analyze");assert.equal(JSON.parse(await download("photo-scale.json")).lens_input,undefined);
    console.log("PASS ChArUco: real Rust detection, exact board/lens receipts, calibrated original-pixel measurement, capture validation and stale-output clearing.");
    assert.deepEqual(errors,[]);assert.ok(requests.every(r=>r.url.startsWith(origin+"/")&&r.method==="GET"));
    assert.equal(await page.evaluate(()=>localStorage.length),0);
    console.log("PASS: source/paper/options/points/cancellation withdraw old outputs; wrong paper, blank/transparent/corrupt image, invalid points/options refuse. No uploads, third-party requests, storage or page errors. Artifacts:",work);
    await page.goto(origin+"/index.html");
    await page.waitForFunction(()=>/^Ready/.test(document.getElementById("status").textContent),null,{timeout:30000});
    await page.check('input[name="program"][value="zouave_trousers"]');
    await page.check('input[name="bodyMode"][value="file"]');
    await page.setInputFiles("#measFile",path.join(work,"jpeg-report.json")); await page.click("#go");
    await page.waitForFunction(()=>!document.getElementById("errorBox").classList.contains("hidden"),null,{timeout:30000});
    assert.match(await page.textContent("#errorText"),/missing measurement 'waist_height'/);
    assert.equal(await page.locator("#results").isVisible(),false);
    assert.deepEqual(errors,[]);
    console.log("PASS: ordinary Workbench consumes the downloaded file but refuses drafting with missing required measurements; photo evidence is not a ready body.");
    await photoHandoff(browser,origin);
  } finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);server.close();process.exitCode=1;});

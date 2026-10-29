// Drives web/ in headless Chromium: a round per program (chart body), the tape form, a refusal, and with
// --model a fold-up per garment, compared byte for byte with `ftx drape` on the same round's documents.
//
//   tools/build_web.sh && cargo build --release -p ftx-cli
//   NODE_PATH=$(npm root -g) node tools/check_web.cjs [--model PATH.ftxbody] [--ftx target/release/ftx]
//   node tools/check_web.cjs --viewer /path/to/fold_viewer.html   # offline file:// rendering and controls
//   node tools/check_web.cjs --model PATH.ftxbody --timings 20   # browser timing lane; no native comparison
//   node tools/check_web.cjs --calibration-sheets NATIVE_DIR  # six local downloads, without page JavaScript
//   node tools/check_web.cjs --generated-calibration-sheets NATIVE_DIR  # actual WASM generation/replay
//   node tools/check_web.cjs --web-dir STAGED_WEB   # retained build with the same page/worker assets
//
// Needs Playwright with its Chromium (npm i -g playwright && npx playwright install chromium).
const { chromium } = require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const { spawn, spawnSync, execFileSync } = require("child_process");
const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { pathToFileURL } = require("url");
const { performance } = require("perf_hooks");

const arg = (name) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : null; };
const root = path.resolve(__dirname, "..");
const webDir = path.resolve(arg("--web-dir") || path.join(root, "web"));
const model = arg("--model");
const acceptedFit = arg("--accepted-fit"), acceptedMeasurements = arg("--accepted-measurements");
const standalone = arg("--viewer");
const calibrationSheets = arg("--calibration-sheets");
const generatedSheets = arg("--generated-calibration-sheets");
const evidenceReplay = arg("--measurement-evidence");
const bodyCheckReplay = arg("--body-check");
const traceEdits = process.argv.includes("--trace-edits");
const timingArg = arg("--timings");
const timingRuns = Number(timingArg || 0);
if (process.argv.includes("--body-check") && (!bodyCheckReplay || model || evidenceReplay || standalone || timingArg)) {
  throw Error("--body-check needs a native capture directory and runs separately from drape/evidence/timing checks");
}
if ((acceptedFit || acceptedMeasurements) && (!model || !acceptedFit || !acceptedMeasurements || standalone || timingArg)) {
  throw Error("accepted-fit checks need --model, --accepted-fit and --accepted-measurements; run separately from timing/viewer checks");
}
if (process.argv.includes("--timings") && (!model || standalone || !/^[1-9][0-9]*$/.test(timingArg || "") || timingRuns > 100)) {
  throw Error("--timings needs --model and an integer run count from 1 to 100; it is separate from --viewer");
}
if (process.argv.includes("--calibration-sheets") && (!calibrationSheets || model || standalone || timingArg)) {
  throw Error("--calibration-sheets needs a native capture directory and runs separately from model/viewer/timing checks");
}
if (process.argv.includes("--generated-calibration-sheets") && (!generatedSheets || calibrationSheets || model || standalone || timingArg)) {
  throw Error("--generated-calibration-sheets needs its native capture directory and runs separately");
}
const userAgent = "OpenAI File Downloader, XaiImageApiFetch/1.0";
const ftx = arg("--ftx") || path.join(root, "target/release/ftx");
const sha = (b) => crypto.createHash("sha256").update(b).digest("hex");
let failed = 0;
const check = (ok, what) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}`); if (!ok) failed++; };

async function tracingEdits(browser, origin) {
  const assert=require("node:assert/strict"),context=await browser.newContext({userAgent});
  const errors=[],requests=[],work=fs.mkdtempSync(path.join(os.tmpdir(),"ftx-trace-drafts-"));
  context.on("request",r=>requests.push({url:r.url(),method:r.method()}));
  await context.addInitScript(()=>{
    const NativeDate=Date;
    window.Date=class extends NativeDate {constructor(...a){super(...(a.length?a:[2026,9,29,12,0]));}};
  });
  await context.route("**/worker.js",async route=>{
    const response=await route.fetch();
    // Both ordinary Workbench paths receive the same explicit export clock, not altered numerics.
    await route.fulfill({response,body:"const NativeDate=Date;globalThis.Date=class extends NativeDate {constructor(...a){super(...(a.length?a:[2026,9,29,12,0]));}};\n"+await response.text()});
  });
  if(process.env.FTX_WASM_PKG) await context.route("**/pkg/*",async route=>{
    const name=path.basename(new URL(route.request().url()).pathname);
    assert.ok(["ftx_wasm.js","ftx_wasm_bg.wasm"].includes(name));
    await route.fulfill({path:path.join(process.env.FTX_WASM_PKG,name),contentType:name.endsWith(".wasm")?"application/wasm":"text/javascript"});
  });
  const page=await context.newPage();page.on("pageerror",e=>errors.push(String(e)));
  const cases=JSON.parse(fs.readFileSync(path.join(root,"tests/fixtures/oracle/trace/raster_trace.json"))).cases;
  const image=Buffer.from(cases[0].input.png,"base64"),other=Buffer.from(cases.find(c=>c.input.png!==cases[0].input.png).input.png,"base64");
  const download=async(name,tab=page)=>{const event=tab.waitForEvent("download");await tab.click(`a[download="${name}"]`);const d=await event;assert.equal(await d.failure(),null);return fs.readFileSync(await d.path());};
  const action=async id=>{await page.click("#"+id);await page.waitForFunction(()=>/^(Review ready|No current review)/.test(document.getElementById("status").textContent));};
  const trace=async bytes=>{
    await page.setInputFiles("#image",{name:"oracle.png",mimeType:"image/png",buffer:bytes});
    await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Ready."));
    await action("trace");assert.equal(await page.isVisible("#error"),false);
  };
  const load=async bytes=>{
    await page.setInputFiles("#draftFile",{name:"trace-edits.json",mimeType:"application/json",buffer:bytes});
    await page.waitForFunction(()=>/^(Edits draft loaded|Draft not loaded)/.test(document.getElementById("status").textContent));
  };
  try {
    await page.goto(origin+"/trace.html");await trace(image);
    assert.equal(await page.locator("#saveDraft").count(),1,"Reopen saved tracing edits");
    assert.equal(await page.locator("#openPattern").count(),1,"Check traced pattern without download/reupload");
    assert.equal(JSON.parse(await page.textContent("#details")).source_sha256,sha(image));
    await page.fill("#names",'{"pieces":{"p1":{"id":"front","name":"Front"}}}');
    await page.fill("#targets",'[{"reference":"front/e1","target_mm":200}]');await action("apply");
    assert.equal(await page.isVisible("#error"),false);
    assert.equal(JSON.parse(await page.textContent("#details")).scale_status,"FITTED");
    const pattern=await download("pattern.json");
    await page.click("#saveDraft");const draft=JSON.parse(await download("trace-edits.json"));
    assert.equal(draft.source_sha256,sha(image));draft.names=' \r\n'+draft.names+'\r\n';const bytes=Buffer.from(JSON.stringify(draft));
    await page.reload();await trace(image);await load(bytes);
    assert.match(await page.textContent("#status"),/^Edits draft loaded/);assert.equal(await page.isVisible("#results"),false);
    assert.equal(await page.inputValue("#names"),draft.names.replace(/\r\n/g,"\n"));
    await action("apply");assert.equal(await page.isVisible("#error"),false);
    assert.ok((await download("pattern.json")).equals(pattern),"Fresh-page restore reproduces complete scaled pattern bytes");
    await page.click("#saveDraft");assert.equal(JSON.parse(await download("trace-edits.json")).names,draft.names);
    await page.screenshot({path:path.join(work,"trace-draft.png"),fullPage:true});
    await trace(other);await load(bytes);assert.match(await page.textContent("#status"),/^Draft not loaded/);
    assert.equal(await page.inputValue("#names"),"{}");assert.equal(await page.inputValue("#targets"),"[]");
    await trace(image);await page.uncheck("#dashes");
    await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Ready."));await action("trace");await load(bytes);
    assert.match(await page.textContent("#status"),/^Draft not loaded/);assert.equal(await page.inputValue("#names"),"{}");
    await page.check("#dashes");await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Ready."));await action("trace");
    await page.fill("#names","{");await action("apply");assert.equal(await page.isVisible("#error"),true);
    await page.click("#saveDraft");assert.equal(JSON.parse(await download("trace-edits.json")).names,"{");
    await page.evaluate(()=>{const original=File.prototype.arrayBuffer;window.originalTraceRead=original;File.prototype.arrayBuffer=function(){return original.call(this).then(bytes=>new Promise(resolve=>{window.releaseDraft=()=>resolve(bytes);}));};});
    await page.setInputFiles("#draftFile",{name:"delayed.json",mimeType:"application/json",buffer:bytes});
    await page.waitForFunction(()=>typeof window.releaseDraft==="function");await page.click("#cancel");
    await page.waitForFunction(()=>document.getElementById("status").textContent.startsWith("Ready."));
    await page.evaluate(async()=>{window.releaseDraft();await new Promise(resolve=>setTimeout(resolve,0));});
    assert.equal(await page.inputValue("#names"),"{");assert.equal(await page.isVisible("#results"),false);
    assert.equal(await page.isDisabled("#apply"),true);assert.deepEqual(errors,[]);
    assert.ok(requests.every(r=>r.url.startsWith(origin+"/")&&r.method==="GET"));
    assert.equal(await page.evaluate(()=>localStorage.length),0);
    await page.evaluate(()=>{
      File.prototype.arrayBuffer=window.originalTraceRead;window.traceStored=false;
      const set=Storage.prototype.setItem;Storage.prototype.setItem=function(...a){window.traceStored=true;return set.apply(this,a);};
    });
    await trace(image);await load(bytes);await action("apply");
    const opened=page.waitForEvent("popup");await page.click("#openPattern");const bench=await opened;
    await bench.waitForFunction(()=>!document.getElementById("go").disabled && document.getElementById("traceImportText").textContent.startsWith("Loaded the local tracing"));
    assert.equal(await bench.isChecked('input[name="program"][value="audit"]'),true);
    assert.equal(await bench.isVisible("#results"),false,"Handoff never automatically runs");
    assert.equal(await bench.evaluate(()=>window.opener===null),true,"Opener released after handoff");
    await bench.screenshot({path:path.join(work,"trace-workbench.png"),fullPage:true});
    const round=async tab=>{await tab.click("#go");await tab.waitForFunction(()=>!document.getElementById("results").classList.contains("hidden") || !document.getElementById("errorBox").classList.contains("hidden"));assert.equal(await tab.isVisible("#errorBox"),false,await tab.textContent("#errorText"));return download(await tab.locator('#resFiles a[download$=".zip"]').getAttribute("download"),tab);};
    const handed=await round(bench);
    // The complete native round is the source of both handoff and ordinary-upload inputs.
    const roundPath=path.join(work,'source-round.zip');fs.writeFileSync(roundPath,handed);
    const entries=execFileSync('unzip',['-Z1',roundPath],{encoding:'utf8'}).trim().split('\n');
    const sources=Object.fromEntries(['pattern.json','measurements.json','fit_spec.json'].map(name=>{
      const matches=entries.filter(entry=>entry===name||entry.endsWith('/'+name));assert.equal(matches.length,1,name);
      return [name,execFileSync('unzip',['-p',roundPath,matches[0]])];
    }));
    const materialOpened=bench.waitForEvent('popup');await bench.click('#chooseMaterials');const material=await materialOpened;
    material.on('pageerror',e=>errors.push(String(e)));
    await material.waitForFunction(()=>document.getElementById('roundImportText').textContent.startsWith('Received pattern.json')&&!document.getElementById('bind').disabled);
    assert.equal(await material.evaluate(()=>window.opener===null),true);
    assert.equal(await material.isDisabled('#exports'),true,'No automatic material binding');
    assert.equal(await material.isDisabled('#fold'),true,'No body is transferred');
    const bind=async tab=>{await tab.click('#bind');await tab.waitForFunction(()=>!document.getElementById('exports').disabled||!document.getElementById('error').hidden);
      assert.equal(await tab.isVisible('#error'),false,await tab.textContent('#error'));};
    await bind(material);
    assert.ok((await download('original-pattern.json',material)).equals(sources['pattern.json']),'Original pattern bytes come from the native round');
    const exportRound=async tab=>{await tab.click('#exports');await tab.locator('a[download="export.zip"]').waitFor();return download('export.zip',tab);};
    const materialZip=await exportRound(material),boundPattern=await download('pattern.json',material);
    const materialManual=await context.newPage();await materialManual.goto(origin+'/materials.html');
    await materialManual.waitForFunction(()=>document.getElementById('status').textContent.startsWith('Ready'));
    for(const [id,name] of [['patternFile','pattern.json'],['measurements','measurements.json'],['fit','fit_spec.json']])
      await materialManual.setInputFiles('#'+id,{name,mimeType:'application/json',buffer:sources[name]});
    await bind(materialManual);
    assert.ok((await download('pattern.json',materialManual)).equals(boundPattern),'Same native material binding');
    assert.ok((await exportRound(materialManual)).equals(materialZip),'Handed round and manual three-file upload produce identical complete material export archives');
    await material.click('#clearRoundImport');assert.equal(await material.locator('a[download="export.zip"]').count(),0);
    assert.equal(await material.isDisabled('#bind'),true);assert.equal(await material.isDisabled('#exports'),true);
    await material.close();await materialManual.close();
    console.log('PASS round-to-material: exact native saved files, explicit binding/exports, complete manual-upload archive equality, no body transfer and clear withdraws outputs.');
    const manual=await context.newPage();await manual.goto(origin+"/index.html");
    await manual.waitForFunction(()=>!document.getElementById("go").disabled);await manual.check('input[name="program"][value="audit"]');
    await manual.setInputFiles("#patternFile",{name:"pattern.json",mimeType:"application/json",buffer:pattern});
    assert.ok((await round(manual)).equals(handed),"Handoff and ordinary file upload produce identical complete export archives");
    const probe='"transport_numeric_probe":{"integer":9007199254740993,"unsigned":18446744073709551615,"signed_zero":-0.0,"float_tag":1.0},';
    const numericPattern=pattern.toString('utf8').replace('"meta": {','"meta": {'+probe);
    assert.notEqual(numericPattern,pattern.toString('utf8'),'Actual source pattern receives synthetic metadata probe');
    let primary=null,fit=null;
    for(const mode of ['chart','file']) {
      await manual.setInputFiles("#patternFile",{name:"pattern.json",mimeType:"application/json",buffer:Buffer.from(numericPattern)});
      if(mode==='file') {
        await manual.check('input[name="bodyMode"][value="file"]');
        await manual.setInputFiles("#measFile",{name:"measurements.json",mimeType:"application/json",buffer:Buffer.from(primary)});
        await manual.setInputFiles("#fitFile",{name:"fit_spec.json",mimeType:"application/json",buffer:Buffer.from(fit)});
      }
      const body=mode==='chart'?'{"mode":"chart","stature_cm":92,"age_months":30}':'{"mode":"file","measurements":'+primary+'}';
      const request='{"body":'+body+',"pattern":'+numericPattern+(fit?',"fit_spec":'+fit:'')+'}';
      const expected=await manual.evaluate(async request=>{
        const ftx=await import('/pkg/ftx_wasm.js');await ftx.default({module_or_path:'/pkg/ftx_wasm_bg.wasm'});
        const result=ftx.run(request,'2026-10-29','2026-10-29T12:00');
        try {return {zip:Array.from(result.zip()),pattern:new TextDecoder().decode(result.file('pattern.json')),
          primary:new TextDecoder().decode(result.file('measurements.json')),fit:new TextDecoder().decode(result.file('fit_spec.json'))};}
        finally {result.free();}
      },request);
      assert.match(expected.pattern,/"integer": 9007199254740993/);assert.match(expected.pattern,/"signed_zero": -0\.0/);assert.match(expected.pattern,/"float_tag": 1\.0/);
      assert.match(expected.pattern,/"unsigned": 18446744073709551615/);
      assert.ok((await round(manual)).equals(Buffer.from(expected.zip)),mode+': browser complete archive equals direct raw-text Rust run');
      primary=expected.primary.replace('"wearer": {','"wearer": {'+probe);fit=expected.fit.replace('{','{'+probe);
    }
    for(const [buffer,error] of [[Buffer.from([0xc3,0x28]),/not valid UTF-8 JSON/],[Buffer.from('\ufeff{}'),/not valid UTF-8 JSON/],[Buffer.from('{'),/not valid UTF-8 JSON/],
      [Buffer.from(numericPattern.replace('18446744073709551615','18446744073709551616')),/64-bit range/],
      [Buffer.from(numericPattern.replace('18446744073709551615','-9223372036854775809')),/64-bit range/]]) {
      await manual.setInputFiles("#patternFile",{name:"invalid.json",mimeType:"application/json",buffer});
      assert.equal(await manual.locator("#resFiles a").count(),0,'Invalid replacement withdraws prior downloads');
      await manual.click("#go");await manual.waitForFunction(()=>!document.getElementById("errorBox").classList.contains("hidden"));
      assert.match(await manual.textContent("#errorText"),error);assert.equal(await manual.isVisible("#results"),false);
    }
    await manual.check('input[name="bodyMode"][value="chart"]');
    console.log("PASS JSON fidelity: uploaded pattern, primary measurement and fit files retain native-supported i64, signed zero and float semantics; complete archives equal direct raw-text WASM execution.");
    await bench.click("#clearTraceImport");assert.equal(await bench.locator("#resFiles a").count(),0);
    await bench.click("#go");await bench.waitForFunction(()=>!document.getElementById("errorBox").classList.contains("hidden"));
    assert.match(await bench.textContent("#errorText"),/choose a pattern.json file/);
    await bench.close();await manual.close();
    let release,arrived;const parked=new Promise(resolve=>{arrived=resolve;});
    const hold=route=>{release=route;arrived();},filter=/\/index\.html\?traceImport=/;
    await context.route(filter,hold);const waiting=page.waitForEvent("popup");await page.click("#openPattern");await parked;
    await page.fill("#names","{}");await release.continue();await context.unroute(filter,hold);const stale=await waiting;
    await stale.waitForFunction(()=>document.getElementById("traceImportText").textContent.startsWith("Tracing review changed"));
    assert.equal(await stale.isVisible("#results"),false);await stale.close();
    assert.equal(await page.evaluate(()=>window.traceStored),false,"Tracing handoff performs no storage writes");
    assert.deepEqual(await page.evaluate(()=>Object.fromEntries(Object.keys(localStorage).map(k=>[k,localStorage.getItem(k)]))),{"fab-wb-bodyMode":'"chart"',"fab-wb-program":'"audit"'},"Only existing Workbench preferences persist, never a pattern");
    assert.deepEqual(errors,[]);assert.ok(requests.every(r=>r.url.startsWith(origin+"/")&&r.method==="GET"));
    console.log("PASS trace-to-Workbench: explicit same-origin handoff, ordinary-upload archive equality, no automatic run, cleared/stale transfer controls and no pattern persistence.");
    console.log("PASS tracing drafts: actual compiled-WASM scaled-pattern replay, exact editor source, wrong image/settings, native JSON refusal, save-after-refusal, cancellation and local-only controls. Artifacts:",work);
  } finally {await context.close();}
}

async function measurementEvidence(page, settle, work, nativeDir) {
  const assert = require("node:assert/strict");
  const request = JSON.parse(fs.readFileSync(path.join(nativeDir, "request.json")));
  const source = fs.readFileSync(path.join(nativeDir, "photo-measurements.json"));
  await page.check('input[name=program][value=zouave_bolero]');
  await page.check('input[name=bodyMode][value=tape]');
  await page.fill("#stature", String(request.body.stature_cm));
  await page.fill("#age", String(request.body.age_months));
  await page.locator('#tapeRows input[data-name="chest"]').waitFor();
  for (const input of await page.locator("#tapeRows input").all()) await input.fill("");
  for (const [name, value] of Object.entries(request.body.tape)) {
    await page.fill(`#tapeRows input[data-name="${name}"]`, String(value));
  }
  await page.locator("summary").filter({hasText:"Add photo or other measurement evidence"}).click();
  const upload = async (name, buffer) => page.setInputFiles("#supplementalFiles", {name, mimeType:"application/json", buffer});
  const absent = async () => assert.equal(await page.locator("#results").evaluate(e=>e.classList.contains("hidden")), true);
  await upload("photo-measurements.json", source);
  assert.match(await settle("#go",120000), /same wearer/);await absent();
  await page.check("#supplementalConsent");assert.equal(await settle("#go",120000), "");
  const saveRound=async dir=>{
    fs.mkdirSync(dir);
    // JSON round documents are in the actual ZIP download, not individual export links.
    const waiting=page.waitForEvent("download");await page.click("#resFiles a[download$='.zip']");
    const download=await waiting;assert.equal(await download.failure(),null);
    fs.copyFileSync(await download.path(),path.join(dir,"round.zip"));
    execFileSync("unzip",["-q","round.zip"],{cwd:dir});
  };
  const dir=path.join(work,"measurement-evidence");await saveRound(dir);
  for(const name of ["measurements.json","pattern.json","readiness.json","fit_spec.json"]) {
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir,name))),JSON.parse(fs.readFileSync(path.join(nativeDir,name))),name);
  }
  const measured=JSON.parse(fs.readFileSync(path.join(dir,"measurements.json")));
  assert.equal(measured.measurement_imports[0],source.toString("utf8"));
  assert.equal(measured.fused.arm_length.primary_provenance,"PHOTO_ESTIMATED");assert.equal(measured.taken_on,null);
  assert.ok(measured.records.some(r=>r.provenance==="MEASURED_TAPE"));
  await page.click("#clearSupplemental");assert.equal(await settle("#go",120000),"");
  const baselineDir=path.join(work,"measurement-baseline");await saveRound(baselineDir);
  const baseline=JSON.parse(fs.readFileSync(path.join(baselineDir,"pattern.json")));
  const withPhoto=JSON.parse(fs.readFileSync(path.join(dir,"pattern.json")));
  assert.notDeepEqual(baseline.pieces.find(p=>p.id==="sleeve").vertices,
    withPhoto.pieces.find(p=>p.id==="sleeve").vertices,"photo arm length must change the actual sleeve vertices");
  await upload("photo-measurements.json",source);await page.check("#supplementalConsent");
  assert.equal(await settle("#go",120000),"");
  await page.evaluate(()=>{window.__holdEvidence=true;});await page.click("#go");
  await page.waitForFunction(()=>!!window.__heldEvidence,null,{timeout:120000});
  await upload("same-bytes-new-source.json",source);await absent();
  await page.evaluate(()=>{const h=window.__heldEvidence;delete window.__heldEvidence;
    h.worker.dispatchEvent(new MessageEvent("message",{data:h.data}));});
  await absent();assert.equal(await page.isChecked("#supplementalConsent"),false);
  await page.check("#supplementalConsent");assert.equal(await settle("#go",120000),"");
  await page.fill('#tapeRows input[data-name="chest"]',"55");await absent();
  assert.equal(await page.isChecked("#supplementalConsent"),false);
  assert.match(await settle("#go",120000),/same wearer/);
  await page.fill('#tapeRows input[data-name="chest"]',String(request.body.tape.chest));
  const invalid=JSON.parse(source);invalid.records[0].sigma_mm=-1;
  for(const [name,buffer,pattern] of [["invalid.json",Buffer.from(JSON.stringify(invalid)),/sigma/],
    ["invalid-utf8.json",Buffer.from([0xc3,0x28]),/encoded data|encoding/i]]) {
    await upload(name,buffer);await page.check("#supplementalConsent");
    assert.match(await settle("#go",120000),pattern);await absent();
  }
  await upload("photo-replacement.json",source);await page.check("#supplementalConsent");
  assert.equal(await settle("#go",120000),"");
  // A saved combined file retains source text but must not append those same records again.
  await page.click("#clearSupplemental");await absent();
  await page.check('input[name=bodyMode][value=file]');
  await page.setInputFiles("#measFile",path.join(dir,"measurements.json"));
  assert.equal(await settle("#go",120000),"");
  const replayDir=path.join(work,"measurement-replay");await saveRound(replayDir);
  const replay=JSON.parse(fs.readFileSync(path.join(replayDir,"measurements.json")));
  assert.deepEqual(replay,measured);
  const imageHash=JSON.parse(source).photo.image_input.sha256;
  assert.equal(await page.evaluate(hash=>Object.values(localStorage).some(s=>s.includes(hash)),imageHash),false);
  await page.check('input[name=bodyMode][value=tape]');
  for (const input of await page.locator("#tapeRows input").all()) await input.fill("");
  await page.check('input[name=bodyMode][value=file]');
  console.log("PASS measurement evidence: actual EXIF-photo records plus tape, full native measurements/pattern/readiness/fit equality, exact imported text, real ZIP download, source edit/refusal controls and saved-file replay. Artifacts:",dir);
}

async function standaloneBodyCheck(page, settle, work, nativeDir) {
  const assert=require("node:assert/strict");
  const request=JSON.parse(fs.readFileSync(path.join(nativeDir,"request.json")));
  const sourcePath=path.join(nativeDir,"accepted_fit.json");
  const source=fs.existsSync(sourcePath)?fs.readFileSync(sourcePath):null;
  const modelPath=path.join(nativeDir,"model.ftxbody"), modelHash=sha(fs.readFileSync(modelPath));
  await page.check(`input[name=program][value=${request.program}]`);
  await page.check(`input[name=bodyMode][value=${request.body.mode}]`);
  if(request.body.mode==="file") {
    await page.setInputFiles("#measFile",{name:"measurements.json",mimeType:"application/json",
      buffer:Buffer.from(JSON.stringify(request.body.measurements))});
  } else {
    await page.fill("#stature",String(request.body.stature_cm));
    await page.fill("#age",String(request.body.age_months));
    await page.selectOption("#sex",request.body.sex||"");
    await page.locator('#tapeRows input[data-name="head"]').waitFor();
    for(const el of await page.locator("#tapeRows input").all())await el.fill("");
    for(const [name,value] of Object.entries(request.body.tape))await page.fill(`#tapeRows input[data-name="${name}"]`,String(value));
  }
  assert.equal(await settle("#go",120000),"");
  assert.equal(await page.locator("#foldBox").evaluate(e=>e.classList.contains("hidden")),true);
  await page.setInputFiles("#bodyFile",modelPath);
  await page.waitForFunction(()=>!document.getElementById("bodyCheckGo").disabled,null,{timeout:120000});
  if(source) {
    await page.locator("summary").filter({hasText:"Reuse an accepted image-fit shape"}).click();
    await page.setInputFiles("#bodyFitFile",sourcePath);
    await page.waitForFunction(()=>/Fit report read/.test(document.getElementById("status").textContent));
    assert.equal(await page.isEnabled("#bodyCheckGo"),false);
    await page.check("#bodyFitConsent");
  }
  const gate=await page.locator("#resGate").innerHTML();
  assert.equal(await settle("#bodyCheckGo",120000),"");
  const dir=path.join(work,"standalone-body-check");fs.mkdirSync(dir);
  const download=async(selector,name)=>{
    const pending=page.waitForEvent("download");await page.click(selector);const file=await pending;
    assert.equal(await file.failure(),null);const bytes=fs.readFileSync(await file.path());
    fs.writeFileSync(path.join(dir,name),bytes,{flag:"wx"});return bytes;
  };
  const raw=await download('#bodyCheckFiles a[download="body_check.json"]',"body_check.json");
  const report=JSON.parse(raw), expected=JSON.parse(fs.readFileSync(path.join(nativeDir,"body_check.json")));
  await download("#resFiles a[download$='.zip']","round.zip");
  execFileSync("unzip",["-q","round.zip"],{cwd:dir});
  const measured=fs.readFileSync(path.join(dir,"measurements.json"));
  const nativeMeasurements=fs.readFileSync(path.join(nativeDir,"measurements.json"));
  // File uploads pass through JSON.parse/stringify: e.g. 1.0 becomes 1. Each digest must bind
  // its actual saved bytes, while every measurement and numerical body field must agree.
  assert.deepEqual(JSON.parse(measured),JSON.parse(nativeMeasurements));
  assert.equal(expected.measurements_sha256,sha(nativeMeasurements));
  assert.equal(report.measurements_sha256,sha(measured));
  assert.deepEqual(report,{...expected,measurements_sha256:sha(measured)});assert.equal(report.cloth_simulated,false);
  assert.equal(report.body.model_sha256,modelHash);assert.equal(report.body.provenance,"MODEL_FIT");
  assert.equal(await page.locator("#resGate").innerHTML(),gate);
  assert.equal(await page.locator("#bodyCheckReadings tr").count(),Object.keys(report.body.readings_mm).length+1);
  if(source)assert.ok((await download('#bodyCheckFiles a[download="body_fit.json"]',"body_fit.json")).equals(source));
  await page.evaluate(()=>{window.__holdBodyCheck=true;});await page.click("#bodyCheckGo");
  await page.waitForFunction(()=>!!window.__heldEvidence,null,{timeout:120000});
  if(source)await page.setInputFiles("#bodyFitFile",{name:"same-fit-new-source.json",mimeType:"application/json",buffer:source});
  else await page.fill('#tapeRows input[data-name="head"]',String(request.body.tape.head+1));
  await page.evaluate(()=>{const h=window.__heldEvidence;delete window.__heldEvidence;
    h.worker.dispatchEvent(new MessageEvent("message",{data:h.data}));});
  assert.equal(await page.locator("#bodyCheckFiles a").count(),0);
  assert.equal(await page.isEnabled("#bodyCheckGo"),false);
  if(source) {
    const rejected=JSON.parse(source);rejected.status="REJECTED";
    await page.setInputFiles("#bodyFitFile",{name:"rejected-fit.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify(rejected))});
    await page.waitForFunction(()=>/Fit report read/.test(document.getElementById("status").textContent));
    await page.check("#bodyFitConsent");assert.match(await settle("#bodyCheckGo",120000),/ACCEPTED|accepted/i);
    assert.equal(await page.locator("#bodyCheckFiles a").count(),0);
  } else {
    await page.fill('#tapeRows input[data-name="head"]',String(request.body.tape.head));
    assert.equal(await settle("#go",120000),"");assert.equal(await settle("#bodyCheckGo",120000),"");
    assert.deepEqual(JSON.parse(await download('#bodyCheckFiles a[download="body_check.json"]',"body_check-replayed.json")),report);
  }
  await page.setInputFiles("#bodyFitFile",[]);await page.setInputFiles("#bodyFile",[]);
  assert.equal(await page.locator("#bodyCheckFiles a").count(),0);
  console.log("PASS standalone body check: native-equal body/measurement values, exact per-file hashes, actual download, flat-only garment, unchanged readiness and source/stale/refusal controls; source:",source?"accepted shape":"production tape fit","artifacts:",dir);
}

async function printableReferences(browser, origin, expectedDir) {
  const assert = require("node:assert/strict");
  const context = await browser.newContext({ javaScriptEnabled: false, acceptDownloads: true, userAgent });
  const page = await context.newPage(), errors = [], requests = [];
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-calibration-downloads-"));
  page.on("pageerror", e => errors.push(String(e)));
  page.on("request", r => requests.push({ url: r.url(), method: r.method() }));
  try {
    await page.goto(origin);
    await page.locator("#calibrationSheets summary").click();
    const panel = page.locator("#calibrationSheets");
    assert.match(await panel.textContent(), /100% \/ Actual size/);
    assert.match(await panel.textContent(), /both axes with a ruler/);
    assert.equal(await panel.locator("a[download]").count(), 6);
    for (const [paper, size, rows, originPx] of [["letter", [2550, 3300], 6, [135, 94]], ["a4", [2480, 3508], 7, [100, 94]]]) {
      for (const ext of ["pdf", "png", "json"]) {
        const name = `calib-sheet-${paper}.${ext}`, link = panel.locator(`a[download="${name}"]`);
        assert.equal(await link.getAttribute("href"), `calibration/${name}`);
        const pending = page.waitForEvent("download");
        await link.click();
        const download = await pending;
        assert.equal(download.suggestedFilename(), name);
        assert.equal(await download.failure(), null);
        const actual = fs.readFileSync(await download.path());
        assert.ok(actual.equals(fs.readFileSync(path.join(expectedDir, name))), `${name} differs from native output`);
        if (ext === "png") {
          assert.equal(actual.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
          assert.deepEqual([actual.readUInt32BE(16), actual.readUInt32BE(20)], size);
          assert.equal(actual.toString("ascii", 37, 41), "pHYs");
          assert.deepEqual([actual.readUInt32BE(41), actual.readUInt32BE(45), actual[49]], [11811, 11811, 1]);
        } else if (ext === "pdf") {
          assert.equal(actual.subarray(0, 8).toString(), "%PDF-1.4");
          assert.ok(actual.includes(Buffer.from("/BitsPerComponent 1 /Interpolate false")));
          assert.ok(actual.includes(Buffer.from(`/MediaBox [0 0 ${(size[0] * 0.24).toFixed(6)} ${(size[1] * 0.24).toFixed(6)}]`)));
        } else {
          const sidecar = JSON.parse(actual);
          assert.equal(sidecar.schema, "fabric-calib-sheet/1"); assert.equal(sidecar.paper, paper);
          assert.equal(sidecar.dpi, 300); assert.deepEqual(sidecar.image_px, size);
          assert.deepEqual([sidecar.board.squares_x, sidecar.board.squares_y, sidecar.board.square_mm, sidecar.board.marker_mm], [8, rows, 22, 16]);
          assert.equal(sidecar.board.dictionary, "DICT_4X4_50"); assert.equal(sidecar.board.legacy_pattern, false);
          assert.deepEqual(sidecar.board.origin_px, originPx); assert.equal(sidecar.pdf_lossless, true);
          assert.deepEqual(sidecar.bars.map(b => b.nominal_mm), [100, 101.6, 100, 101.6]);
          assert.deepEqual(sidecar.test_squares.map(s => s.nominal_mm), [100, 25.4]);
          assert.deepEqual(sidecar.files, { png: `calib-sheet-${paper}.png`, pdf: `calib-sheet-${paper}.pdf` });
          assert.equal(Object.hasOwn(sidecar, "paths"), false);
        }
        fs.writeFileSync(path.join(work, name), actual, { flag: "wx" });
        console.log(`PASS ${name}: actual no-JavaScript browser download equals native bytes (${sha(actual)})`);
      }
    }
    assert.deepEqual(errors, []);
    assert.ok(requests.every(r => r.url.startsWith(origin + "/") && r.method === "GET"));
    await panel.screenshot({ path: path.join(work, "references.png") });
    console.log(`PASS: all six printable references work without page JavaScript; local GET only, no page errors. Digital interoperability only; physical print/measurement accuracy unproven. Artifacts: ${work}`);
  } finally { await context.close(); }
}

async function generatedReferences(browser, origin, expectedDir) {
  const assert = require("node:assert/strict");
  const cases = [
    ["default-letter", "letter", 22, 16, "DICT_4X4_50"],
    ["default-a4", "a4", 22, 16, "DICT_4X4_50"],
    ["aruco4-large", "letter", 23, 15, "DICT_4X4_1000"],
    ["aruco5", "letter", 23, 15, "DICT_5X5_50"],
    ["aruco6", "a4", 18, 11, "DICT_6X6_250"],
    ["aruco7", "letter", 23, 15, "DICT_7X7_1000"],
    ["original", "a4", 18, 11, "DICT_ARUCO_ORIGINAL"],
    ["tag16", "letter", 22, 16, "DICT_APRILTAG_16H5"],
    ["tag25", "a4", 18, 11, "DICT_APRILTAG_25h9"],
    ["tag36-10", "letter", 23, 15, "DICT_APRILTAG_36H10"],
    ["tag36-11", "a4", 18, 11, "DICT_APRILTAG_36h11"],
    ["mip36", "letter", 22, 16, "DICT_ARUCO_MIP_36h12"],
  ].map(([id, paper, square, marker, dictionary]) => ({ id, paper, square, marker, dictionary }));
  const context = await browser.newContext({ userAgent, acceptDownloads: true });
  // Hold an actual computed worker result, then deliver it after cancellation/source edits.
  // Numeric data still comes from the unmodified production WASM, never from a test double.
  await context.addInitScript(() => {
    const Original = window.Worker;
    window.Worker = class extends Original {
      constructor(...args) {
        super(...args);
        this.addEventListener("message", e => {
          if (window.__holdSheet && e.data.type === "result") {
            e.stopImmediatePropagation(); window.__holdSheet = false;
            window.__heldSheet = { worker: this, data: e.data };
          }
        });
      }
    };
  });
  const page = await context.newPage(), errors = [], requests = [];
  context.on("request", r => requests.push({ url: r.url(), method: r.method() }));
  page.on("pageerror", e => errors.push(String(e)));
  const panel = page.locator("#sheetDownloads");
  const absent = async () => assert.equal(await panel.locator("a").count(), 0);
  const configure = async c => {
    await page.selectOption("#sheetPaper", c.paper);
    await page.fill("#sheetSquare", String(c.square)); await page.fill("#sheetMarker", String(c.marker));
    await page.fill("#sheetDictionary", c.dictionary); await absent();
  };
  const complete = async () => {
    await page.click("#sheetGenerate");
    await page.waitForFunction(() => !document.getElementById("sheetGenerate").disabled, null, { timeout: 60000 });
  };
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-generated-references-"));
  try {
    await page.goto(origin); await page.waitForFunction(() => document.querySelectorAll("#sheetDictionaries option").length === 27);
    await page.locator("#calibrationSheets summary").click();
    for (const c of cases) {
      await configure(c); await complete(); assert.equal(await panel.locator("a").count(), 3, await page.textContent("#sheetStatus"));
      for (const ext of ["pdf", "png", "json"]) {
        const name = `calib-sheet-${c.paper}.${ext}`, event = page.waitForEvent("download");
        await panel.locator(`a[download="${name}"]`).click(); const d = await event;
        assert.equal(await d.failure(), null); const bytes = fs.readFileSync(await d.path());
        assert.ok(bytes.equals(fs.readFileSync(path.join(expectedDir, c.id, name))), `${c.id}/${name}: WASM/native bytes differ`);
        fs.writeFileSync(path.join(work, `${c.id}-${name}`), bytes, { flag: "wx" });
        // Chromium throttles more than ten rapid downloads from one page. Pace clicks;
        // keep each real download and byte comparison rather than bypassing the UI.
        await page.waitForTimeout(150);
      }
      console.log(`PASS generated ${c.id}: matching native PNG/PDF/sidecar bytes`);
    }
    for (const negative of [{ dictionary: "DICT_5X5_51" }, { marker: 22 }, { square: 22.5 }, { square: 24 }, { marker: 0 }]) {
      await configure({ paper: "letter", square: 22, marker: 16, dictionary: "DICT_4X4_50", ...negative });
      await complete(); await absent(); assert.match(await page.textContent("#sheetStatus"), /refused/);
      console.log(`PASS generated refusal ${JSON.stringify(negative)}: no stale/partial downloads`);
    }
    for (const change of ["cancel", "sheetPaper", "sheetSquare", "sheetMarker", "sheetDictionary"]) {
      await configure({ paper: "letter", square: 22, marker: 16, dictionary: "DICT_4X4_50" });
      await page.evaluate(() => { window.__heldSheet = null; window.__holdSheet = true; });
      await page.click("#sheetGenerate"); await page.waitForFunction(() => window.__heldSheet !== null, null, { timeout: 60000 });
      if (change === "cancel") await page.click("#sheetCancel");
      else if (change === "sheetPaper") await page.selectOption("#sheetPaper", "a4");
      else await page.fill("#" + change, change === "sheetDictionary" ? "DICT_5X5_50" : "18");
      await absent();
      await page.evaluate(() => { const h = window.__heldSheet; h.worker.dispatchEvent(new MessageEvent("message", { data: h.data })); window.__heldSheet = null; });
      await absent(); assert.equal(await page.isDisabled("#sheetCancel"), true);
      console.log(`PASS generated ${change}: late actual-WASM result cannot publish`);
    }
    assert.deepEqual(errors, []); assert.ok(requests.every(r => r.method === "GET" && r.url.startsWith(origin + "/")));
    console.log(`PASS:12 positive/5 refusal/5 late-result controls, no page errors or uploads/external requests. Digital software checks only; physical print accuracy unproven. Downloads: ${work}`);
  } finally { await context.close(); }
}

async function offlineViewer(browser, file) {
  const html = fs.readFileSync(file, "utf8");
  const imports = JSON.parse(html.split('<script type="importmap">')[1].split('</script>')[0]).imports;
  for (const [name, digest] of [
    ["three", "3e690ac7d180b0aadf0891bea39eec643e29e2d3e75c99b18689518665f69ba6"],
    ["three/addons/controls/OrbitControls.js", "5a44a9e86a2a0fb11933eed69bc2cd33c76a496854c1aed6ed776efa87d7b064"],
  ]) {
    const prefix = "data:text/javascript;base64,";
    if (!imports[name].startsWith(prefix)) throw Error(`${name} is not embedded in the viewer`);
    check(sha(Buffer.from(imports[name].slice(prefix.length), "base64")) === digest, `${name} matches its pinned upstream bytes`);
  }
  check(html.includes(fs.readFileSync(path.join(root, "assets/three-r160/LICENSE"), "utf8")), "the complete renderer license is distributed with the viewer");
  const context = await browser.newContext({ offline: true, userAgent, reducedMotion: "reduce" });
  const page = await context.newPage(), errors = [], external = [];
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-offline-viewer-"));
  page.on("pageerror", e => errors.push(String(e)));
  page.on("request", r => { if (/^https?:/.test(r.url())) external.push(r.url()); });
  try {
    await page.goto(pathToFileURL(path.resolve(file)).href);
    await page.waitForFunction(() => document.body.dataset.ready, null, { timeout: 20000 });
    check(await page.getAttribute("body", "data-ready") === "1", "offline viewer initializes from file://");
    if (await page.getAttribute("body", "data-ready") !== "1") throw Error(await page.textContent("#error"));
    check(await page.locator("#stage canvas").count() === 1, "the production renderer creates its canvas");
    const frames = await page.evaluate(() => window.__fold.frames);
    check(frames > 1, "the viewer contains multiple retained frames");
    const slider = page.locator("#frame"), canvas = page.locator("#stage canvas");
    await slider.focus(); await slider.press("Home");
    check(/Flat pattern/.test(await page.textContent("#phase")), "Home selects the board frame");
    const board = await canvas.screenshot({ path: path.join(work, "board.png") });
    await slider.press("End");
    check(/Draped/.test(await page.textContent("#phase")), "End selects the draped frame");
    check(await page.textContent("#count") === `${frames} / ${frames}`, "the frame counter reaches the final retained frame");
    const draped = await canvas.screenshot({ path: path.join(work, "draped.png") });
    check(sha(board) !== sha(draped), "different retained geometry changes the rendered pixels");
    await page.check("input[name=colour][value=strain_rgb]");
    await page.uncheck("#showBody");
    await page.click("#play");
    check(await page.textContent("#play") === "Pause", "playback resumes through the normal control");
    const blob = await page.evaluate(html => URL.createObjectURL(new Blob([html], { type: "text/html" })), html);
    await page.goto(blob);
    await page.waitForFunction(() => document.body.dataset.ready, null, { timeout: 20000 });
    check(await page.getAttribute("body", "data-ready") === "1" && await page.locator("#stage canvas").count() === 1,
      "the same HTML also renders as an offline blob preview");
    check(external.length === 0, `no HTTP(S) requests (${external.length})`);
    check(errors.length === 0, `no page errors${errors.length ? ": " + errors.join(" | ") : ""}`);
    console.log(`viewer sha256 ${sha(fs.readFileSync(file))}; screenshots in ${work}`);
  } finally { await context.close(); }
}

(async () => {
  if (standalone) {
    const browser = await chromium.launch();
    try { await offlineViewer(browser, standalone); } finally { await browser.close(); }
    process.exit(failed ? 1 : 0);
  }
  const port = 20000 + Math.floor(Math.random() * 20000);
  const srv = spawn("python3", ["-m", "http.server", String(port), "-d", webDir], { stdio: ["ignore", "ignore", "inherit"] });
  await new Promise((r) => setTimeout(r, 1200));
  const browser = await chromium.launch();
  if(traceEdits) {
    try {await tracingEdits(browser,`http://localhost:${port}`);}
    finally {await browser.close();srv.kill();}
    return;
  }
  if (calibrationSheets || generatedSheets) {
    try {
      if (generatedSheets) await generatedReferences(browser, `http://localhost:${port}`, path.resolve(generatedSheets));
      else await printableReferences(browser, `http://localhost:${port}`, path.resolve(calibrationSheets));
    }
    finally { await browser.close(); srv.kill(); }
    return;
  }
  const page = await browser.newPage({ userAgent });
  if (evidenceReplay || bodyCheckReplay) await page.route("**/worker.js",async route=>{
    const response=await route.fetch();
    // Freeze the caller's date, not numerical results, to the native page fixture's clock.
    await route.fulfill({response,body:"const NativeDate=Date; globalThis.Date=class extends NativeDate { constructor(...a) { super(...(a.length?a:[2026,9,13,12,0])); } };\n"+await response.text()});
  });
  if (evidenceReplay || bodyCheckReplay) await page.addInitScript(()=>{
    const Original=window.Worker;
    window.Worker=class extends Original {
      constructor(...args) {super(...args);this.addEventListener("message",e=>{
        if((window.__holdEvidence && e.data.type==="result") || (window.__holdBodyCheck && e.data.type==="bodyChecked")) {
          e.stopImmediatePropagation();window.__holdEvidence=false;window.__holdBodyCheck=false;window.__heldEvidence={worker:this,data:e.data};
        }
      });}
    };
  });
  const errors = [], requests = [];
  page.context().on("request", r => { if (/^https?:/.test(r.url())) requests.push({ url: r.url(), method: r.method() }); });
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  const runs = async () => +(await page.evaluate(() => document.body.dataset.runs || 0));
  const settle = async (click, timeout) => {
    const n = await runs();
    await page.click(click);
    await page.waitForFunction((n) => +(document.body.dataset.runs || 0) > n, n, { timeout });
    return page.$eval("#errorBox", (e) => (e.classList.contains("hidden") ? "" : e.textContent.trim()));
  };
  const bytes = async (sel) => Buffer.from(await page.$eval(sel, async (a) => {
    const b = new Uint8Array(await (await fetch(a.href)).arrayBuffer());
    let s = "";
    for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode.apply(null, b.subarray(i, i + 0x8000));
    return btoa(s);
  }), "base64");
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-check-web-"));
  const writeTimingFingerprint = () => {
    if (!timingRuns) return;
    const fingerprint = {
      lane: "browser_timing_only", requested_runs_per_garment: timingRuns, warmups_per_garment: 3,
      garments: ["zouave_bolero", "zouave_trousers"],
      duration_clock: "Node performance.now (monotonic milliseconds)",
      host: { cpu: os.cpus()[0].model, logical_cpus: os.cpus().length, ram_bytes: os.totalmem(),
        platform: os.platform(), arch: os.arch(), kernel: os.release(), node: process.version },
      browser: browser.version(), repository_head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
      declared_build_profile: process.env.FTX_WASM_PROFILE || "unspecified; bind the artifact hash to its build receipt",
      wasm_sha256: sha(fs.readFileSync(path.join(root, "web/pkg/ftx_wasm_bg.wasm"))),
      glue_sha256: sha(fs.readFileSync(path.join(root, "web/pkg/ftx_wasm.js"))),
      model_sha256: sha(fs.readFileSync(model)),
      sources: Object.fromEntries(["web/index.html", "web/worker.js", "tools/check_web.cjs"].map(n => [n, sha(fs.readFileSync(path.join(root, n)))])),
      conditions: "Headless Chromium on a shared host; no affinity, clock/governor changes or cache flush. Model retained in worker; each garment drafted once. Fold click to completed result, including tape fit and UI delivery; viewer checks outside timing. Sequential bolero then trousers.",
      started_utc: new Date().toISOString(), system_load: os.loadavg(),
    };
    fs.writeFileSync(path.join(work, "fingerprint.json"), JSON.stringify(fingerprint, null, 2), { flag: "wx" });
  };
  const timedFolds = async (dir, program) => {
    const rows = [];
    const inputs = Object.fromEntries(["pattern.json", "measurements.json", "fit_spec.json"].map(n => [n, sha(fs.readFileSync(path.join(dir, n)))]));
    let reference = null, viewerFile = null, verifiedOffline = false;
    try {
      await page.check("#selfContact");
      for (let i = 0; i < timingRuns + 3; i++) {
        const warmup = i < 3, name = `${warmup ? "warmup" : "sample"}-${warmup ? i + 1 : i - 2}`;
        const row = { name, warmup, started_utc: new Date().toISOString(), system_load_start: os.loadavg() };
        const t0 = performance.now();
        try {
          const err = await settle("#foldGo", 900000);
          row.elapsed_ms = performance.now() - t0;
          if (err) throw Error(err);
          const raw = await bytes("a[download='sim_report.json']"), report = JSON.parse(raw);
          fs.writeFileSync(path.join(dir, `${name}.report.json`), raw, { flag: "wx" });
          row.report_sha256 = sha(raw);
          row.numerical_settled = report.numerical?.settled === true;
          row.all_steps_converged = report.numerical?.sew?.unconverged === 0 && report.numerical?.drape?.unconverged === 0;
          row.gating = report.gating;
          row.self_contact = report.self_contact;
          row.vertices = report.vertices; row.faces = report.faces;
          reference ||= row.report_sha256;
          row.valid = row.numerical_settled && row.all_steps_converged && Array.isArray(row.gating) && row.gating.length === 0
            && row.self_contact?.enabled === true && row.self_contact.thickness_mm === 2
            && row.report_sha256 === reference && row.elapsed_ms < 300000;
          if (!row.valid) throw Error("numerical, contact, digest or declared 300-second browser budget failed");
          if (i === 3) {
            viewerFile = path.join(dir, "browser_fold_viewer.html");
            fs.writeFileSync(viewerFile, await bytes("a[download='fold_viewer.html']"), { flag: "wx" });
          }
        } catch (e) {
          row.elapsed_ms ??= performance.now() - t0;
          row.valid = false; row.error = String(e);
          throw e;
        } finally {
          row.system_load_end = os.loadavg(); rows.push(row);
          fs.appendFileSync(path.join(dir, "timings.jsonl"), JSON.stringify(row) + "\n");
          console.log(`${program} ${name}: ${row.elapsed_ms} ms, ${row.valid ? "valid" : row.error}`);
        }
      }
      const failuresBefore = failed;
      await offlineViewer(browser, viewerFile);
      verifiedOffline = failed === failuresBefore;
    } finally {
      const samples = rows.filter(r => !r.warmup), complete = verifiedOffline && rows.length === timingRuns + 3 && rows.every(r => r.valid);
      const times = samples.map(r => r.elapsed_ms).sort((a, b) => a - b);
      const quantile = q => times[Math.ceil(q * times.length) - 1];
      const result = { program, inputs, warmups_requested: 3, runs_requested: timingRuns,
        attempts: rows.length, retained_runs: samples.length, complete, offline_viewer_passed: verifiedOffline, report_sha256: reference,
        latency_ms: complete ? { min: times[0], p50: quantile(0.5), p95: quantile(0.95), max: times.at(-1) } : null,
        statistics: "Empirical nearest-rank quantiles; no tail confidence or hardware extrapolation. Failed/incomplete runs have no latency summary." };
      fs.writeFileSync(path.join(dir, "timing-summary.json"), JSON.stringify(result, null, 2), { flag: "wx" });
      console.log(JSON.stringify(result));
    }
  };
  try {
    writeTimingFingerprint();
    await page.goto(`http://localhost:${port}/`);
    await page.waitForFunction(() => /Ready|Could not/.test(document.getElementById("status").textContent), null, { timeout: 60000 });
    check(/Ready/.test(await page.textContent("#status")), "the module loads");
    for (const [program, status] of [["zouave_bolero", "CONVERGED"], ["zouave_trousers", "CONVERGED"], ["fez", "CONVERGED"]]) {
      await page.check(`input[name=program][value=${program}]`);
      const err = await settle("#go", 120000);
      check(!err && (await page.textContent("#resStatus")) === status, `${program} round ${status}`);
    }
    // Actual file uploads through the page/worker/WASM; saved fold inputs must follow records.
    const oracle = JSON.parse(fs.readFileSync(path.join(root, "tests/fixtures/oracle/web/webapp.json")));
    const source = oracle.cases.find(c => c.id === "file-pattern-spec").input.body.measurements;
    const stale = structuredClone(source);
    stale.fused.chest.value_mm = 1111; stale.fused.waist.value_mm = 1234;
    const measured = structuredClone(source); measured.wearer.reference = false;
    for (const r of measured.records) {
      r.provenance = "MEASURED_TAPE"; r.method = "test: synthetic independent tape control";
      r[Object.hasOwn(r, "sigma_deg") ? "sigma_deg" : "sigma_mm"] = 1;
    }
    await page.check("input[name=program][value=zouave_bolero]");
    await page.check("input[name=bodyMode][value=file]");
    let original, ready;
    for (const name of ["fresh", "stale", "measured", "modeled"]) {
      const input = name === "fresh" ? source : name === "stale" ? stale : name === "measured" ? measured : structuredClone(ready);
      if (name === "modeled") for (const r of input.records) {
        r.provenance = "MODEL_FIT"; r.method = "test: model estimate, not an independent measurement";
      }
      const upload = path.join(work, `upload-${name}.json`), raw = Buffer.from(JSON.stringify(input));
      fs.writeFileSync(upload, raw, { flag: "wx" });
      await page.setInputFiles("#measFile", upload);
      check(await page.locator("#results").evaluate(e => e.classList.contains("hidden")), `${name} upload withdraws the previous round`);
      check(!(await settle("#go", 120000)), `${name} file round executes`);
      const dir = path.join(work, `upload-${name}`); fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"), { flag: "wx" });
      execFileSync("unzip", ["-q", "round.zip"], { cwd: dir });
      const measurements = fs.readFileSync(path.join(dir, "measurements.json"));
      const pattern = fs.readFileSync(path.join(dir, "pattern.json"));
      const readiness = JSON.parse(fs.readFileSync(path.join(dir, "readiness.json")));
      if (name === "fresh") original = { measurements, pattern };
      if (name === "stale") check(measurements.equals(original.measurements) && pattern.equals(original.pattern), "poisoned cache leaves actual saved measurements/pattern bytes unchanged");
      if (name === "measured") {
        ready = JSON.parse(measurements);
        check(readiness.status === "READY" && (await page.textContent("#resStatus")) === "CONVERGED", "synthetic independent tape control has a READY cache and converged flat round");
      }
      if (name === "modeled") {
        check(JSON.parse(measurements).fused.chest.primary_provenance === "MODEL_FIT", "model records remain MODEL_FIT in actual saved fold input");
        check(readiness.status === "NOT_READY" && (await page.textContent("#resStatus")) === "MEASUREMENTS_NOT_READY", "formerly READY cache cannot promote model records through the browser measurement gate");
      }
      check(fs.readFileSync(upload).equals(raw), `${name} source upload remains unchanged`);
    }
    const invalid = structuredClone(source); invalid.records[0].sigma_mm = -1;
    await page.setInputFiles("#measFile", { name: "invalid.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(invalid)) });
    check(/sigma/.test(await settle("#go", 120000)), "valid cache cannot hide an invalid uploaded record");
    check(await page.locator("#results").evaluate(e => e.classList.contains("hidden")), "refused upload exposes no prior round downloads");
    if (evidenceReplay) await measurementEvidence(page, settle, work, path.resolve(evidenceReplay));
    if (bodyCheckReplay) await standaloneBodyCheck(page, settle, work, path.resolve(bodyCheckReplay));
    await page.check("input[name=program][value=zouave_trousers]");
    await page.check("input[name=bodyMode][value=tape]");
    await page.waitForFunction(() => document.querySelectorAll("#tapeRows input").length > 0, null, { timeout: 30000 });
    await settle("#go", 120000);
    check((await page.textContent("#resStatus")) === "MEASUREMENTS_NOT_READY", "an empty tape form is NOT_READY");
    await page.check("input[name=bodyMode][value=chart]");
    await page.fill("#stature", "9");
    check(/enter centimetres/.test(await settle("#go", 120000)), "a 9 cm stature is refused");
    await page.fill("#stature", "92");
    if (model) {
      await page.setInputFiles("#bodyFile", model);
      await page.waitForFunction(() => /Body model loaded/.test(document.getElementById("status").textContent), null, { timeout: 120000 });
      for (const program of ["zouave_bolero", "zouave_trousers"]) {
        await page.check(`input[name=program][value=${program}]`);
        await settle("#go", 120000);
        const dir = path.join(work, program);
        fs.mkdirSync(dir);
        fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"));
        execFileSync("unzip", ["-q", "round.zip"], { cwd: dir });
        if (timingRuns) { await timedFolds(dir, program); continue; }
        const t0 = Date.now();
        const err = await settle("#foldGo", 900000);
        const s = ((Date.now() - t0) / 1000).toFixed(1);
        check(!err && /no gating flag/.test(await page.textContent("#foldSummary")), `${program} folds up in the page (${s} s), settled, no gating flag`);
        if (err) continue;
        const viewer = await bytes("a[download='fold_viewer.html']");
        const viewerFile = path.join(dir, "browser_fold_viewer.html");
        fs.writeFileSync(viewerFile, viewer, { flag: "wx" });
        const report = JSON.parse(await bytes("a[download='sim_report.json']"));
        check(report.body?.provenance === "MODEL_FIT" && Object.keys(report.body.readings_mm).length === 21,
          `${program} retains all body predictions as MODEL_FIT`);
        check(report.body.model_sha256 === sha(fs.readFileSync(model))
          && report.measurements_sha256 === sha(fs.readFileSync(path.join(dir, "measurements.json"))),
          `${program} diagnostics identify the exact model and saved measurements`);
        const tape = Object.entries(report.body.fit.tape);
        check(tape.length > 0 && tape.every(([name, r]) => r.residual_mm === r.body_mm - r.target_mm
          && r.body_mm === report.body.readings_mm[name]),
          `${program} retains every signed unrounded body-target residual`);
        check((await page.locator("#foldSummary table tr").count()) === tape.length + 1
          && /MODEL_FIT/.test(await page.textContent("#foldSummary")), `${program} shows the actual body residual rows`);
        check((await page.textContent("#resStatus")) === "CONVERGED", `${program} fold diagnostics preserve the separate round verdict`);
        await offlineViewer(browser, viewerFile);
        execFileSync(ftx, ["drape", "pattern.json", "--measurements", "measurements.json", "--fit", "fit_spec.json",
          "--self-contact-mm", "2", "--model", model, "--out", "native"], { cwd: dir, stdio: ["ignore", "pipe", "inherit"] });
        const native = fs.readFileSync(path.join(dir, "native/fold_viewer.html"));
        const nativeBody = JSON.parse(fs.readFileSync(path.join(dir, "native/sim_report.json"))).body;
        check(tape.every(([name, r]) => r.target_mm === nativeBody.fit.tape[name].target_mm
          && Math.round(r.body_mm * 10) / 10 === nativeBody.fit.tape[name].body_mm),
          `${program} fitted targets/readings agree with native at its documented 0.1mm report rounding`);
        check(sha(viewer) === sha(native), `${program} fold-up equals ftx drape's, byte for byte (${sha(native).slice(0, 12)})`);
        if (program === "zouave_bolero") {
          await page.locator("#twoStart").check();
          check((await page.textContent("#foldSummary")) === "" && (await page.locator("#foldSummary a").count()) === 0,
            "two-path policy edits withdraw the prior single-path evidence");
          const pairErr = await settle("#foldGo", 900000);
          check(!pairErr, "browser two-path sensitivity completes");
          if (!pairErr) {
            const pairedReport = JSON.parse(await bytes("a[download='sim_report.json']"));
            check(pairedReport.two_start?.schema === "ftx-two-start/1"
              && pairedReport.two_start.sampled_paths === 2
              && pairedReport.two_start.primary.sew_steps === 12
              && pairedReport.two_start.alternate.sew_steps === 18,
              "browser two-path report retains the shared 12-vs-18 policy");
            const pairedViewer = await bytes("a[download='fold_viewer.html']");
            execFileSync(ftx, ["drape", "pattern.json", "--measurements", "measurements.json", "--fit", "fit_spec.json",
              "--self-contact-mm", "2", "--two-start-sew", "18", "--model", model, "--out", "native-paired"],
              { cwd: dir, stdio: ["ignore", "pipe", "inherit"] });
            check(sha(pairedViewer) === sha(fs.readFileSync(path.join(dir, "native-paired/fold_viewer.html"))),
              "browser two-path fold equals native paired drape byte for byte");
            check(/Two sewing paths:/.test(await page.textContent("#foldSummary")),
              "browser reports paired-path status beside ordinary drape gates");
          }
          await page.locator("#twoStart").uncheck();
        }
        await page.locator("#selfContact").uncheck();
        check((await page.textContent("#foldSummary")) === "" && (await page.locator("#foldSummary a").count()) === 0,
          `${program} edits withdraw residuals and downloads`);
        await page.locator("#selfContact").check();
      }
    }
    if (acceptedFit) {
      const assert = require("node:assert/strict"), raw = fs.readFileSync(acceptedFit);
      const input = fs.readFileSync(acceptedMeasurements), saved = JSON.parse(raw);
      await page.check("input[name=program][value=zouave_bolero]");
      await page.check("input[name=bodyMode][value=file]");
      await page.setInputFiles("#measFile", acceptedMeasurements);
      assert.equal(await settle("#go", 120000), "");
      const status = await page.textContent("#resStatus"), dir = path.join(work, "accepted-fit");
      fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, "round.zip"), await bytes("a[download$='.zip']"), {flag:"wx"});
      execFileSync("unzip", ["-q", "round.zip"], {cwd:dir});
      await page.locator("details").filter({has:page.locator("#bodyFitFile")}).locator("summary").click();
      await page.setInputFiles("#bodyFitFile", acceptedFit);
      await page.waitForFunction(() => /Fit report read/.test(document.getElementById("status").textContent));
      assert.equal(await page.locator("#foldGo").isDisabled(),true);
      await page.check("#bodyFitConsent");await page.fill("#bodyArmDeg","50");
      assert.equal(await settle("#foldGo",900000),"");
      const report = JSON.parse(await bytes("a[download='sim_report.json']"));
      assert.deepEqual(report.body.source_fit,saved);
      assert.deepEqual(report.body.phenotype,saved.solution.anny_parameters.phenotype);
      assert.equal(report.body.shape_source.refitted,false);
      assert.equal(report.body.shape_source.image_poses_reused,false);
      assert.equal(report.body.shape_source.report_sha256,sha(raw));
      assert.ok((await bytes("a[download='body_fit.json']")).equals(raw));
      assert.equal(await page.textContent("#resStatus"),status);
      assert.match(await page.textContent("#foldSummary"),/without refitting/);
      const viewer = await bytes("a[download='fold_viewer.html']");
      fs.writeFileSync(path.join(dir,"body_fit.json"),raw,{flag:"wx"});
      const nativeRun = spawnSync(ftx,["drape","pattern.json","--measurements","measurements.json","--fit","fit_spec.json",
        "--body-fit","body_fit.json","--arm-deg","50","--self-contact-mm","2","--model",model,"--out","native"],
        {cwd:dir,stdio:["ignore","pipe","inherit"]});
      assert.ok([0,2].includes(nativeRun.status),String(nativeRun.stdout)); // retain any ordinary drape gating flags
      const native = JSON.parse(fs.readFileSync(path.join(dir,"native/sim_report.json")));
      delete native.pattern;delete native.wall_s;
      assert.deepEqual(report,native);
      assert.ok(viewer.equals(fs.readFileSync(path.join(dir,"native/fold_viewer.html"))));
      check(true,"accepted fit: complete native/browser report and viewer equality, exact original report bytes, no refit or readiness promotion");
      const material = await page.context().newPage(); material.on("pageerror", e => errors.push(String(e)));
      const materialBytes = async name => {
        const event = material.waitForEvent("download"); await material.click(`#outputs a[download='${name}']`);
        const file = await event; assert.equal(await file.failure(), null); return fs.readFileSync(await file.path());
      };
      await material.goto(`http://localhost:${port}/materials.html`);
      await material.waitForFunction(() => document.getElementById("status").textContent.startsWith("Ready."));
      await material.setInputFiles("#patternFile",path.join(dir,"pattern.json"));
      await material.waitForFunction(() => !document.getElementById("bind").disabled);
      await material.click("#bind"); await material.waitForFunction(() => document.getElementById("status").textContent.startsWith("bind complete"));
      const boundPattern = await materialBytes("pattern.json");
      fs.writeFileSync(path.join(dir,"bound-pattern.json"),boundPattern,{flag:"wx"});
      await material.setInputFiles("#measurements",path.join(dir,"measurements.json"));
      await material.setInputFiles("#fit",path.join(dir,"fit_spec.json"));
      await material.setInputFiles("#body",model);
      await material.locator("details").filter({has:material.locator("#bodyFitFile")}).locator("summary").click();
      await material.setInputFiles("#bodyFitFile",acceptedFit);
      assert.equal(await material.isEnabled("#fold"),false);
      await material.check("#bodyFitConsent"); await material.fill("#bodyArmDeg","50");
      await material.click("#fold"); await material.waitForFunction(() => /^fold (complete|refused)/.test(document.getElementById("status").textContent),null,{timeout:900000});
      assert.equal(await material.isVisible("#error"),false,await material.textContent("#error"));
      const materialReport = JSON.parse(await materialBytes("sim_report.json"));
      assert.deepEqual(materialReport.body,report.body);
      assert.ok((await materialBytes("body_fit.json")).equals(raw));
      const materialNative = spawnSync(ftx,["drape","bound-pattern.json","--measurements","measurements.json","--fit","fit_spec.json",
        "--body-fit","body_fit.json","--arm-deg","50","--self-contact-mm","2","--model",model,"--out","material-native"],
        {cwd:dir,stdio:["ignore","pipe","inherit"]});
      assert.ok([0,2].includes(materialNative.status),String(materialNative.stdout));
      const expectedMaterial = JSON.parse(fs.readFileSync(path.join(dir,"material-native/sim_report.json")));
      delete expectedMaterial.pattern; delete expectedMaterial.wall_s;
      assert.deepEqual(materialReport,expectedMaterial);
      assert.ok((await materialBytes("fold_viewer.html")).equals(fs.readFileSync(path.join(dir,"material-native/fold_viewer.html"))));
      await material.setInputFiles("#bodyFitFile",{name:"rejected.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify({...saved,status:"REJECTED"}))});
      assert.equal(await material.locator("#outputs a[download='sim_report.json']").count(),0);
      assert.equal(await material.isEnabled("#fold"),false); await material.check("#bodyFitConsent");
      await material.click("#fold"); await material.waitForFunction(() => document.getElementById("status").textContent.startsWith("fold refused"),null,{timeout:120000});
      assert.match(await material.textContent("#error"),/accepted Anny shape/);
      assert.equal(await material.locator("#outputs a[download='sim_report.json']").count(),0);
      await material.close();
      check(true,"material preview: exact accepted shape and complete native report/viewer equality; refused replacement cannot tape-refit");
      const rejected = {...saved,status:"REJECTED"};
      await page.setInputFiles("#bodyFitFile",{name:"rejected.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify(rejected))});
      await page.waitForFunction(() => /Fit report read/.test(document.getElementById("status").textContent));
      assert.equal(await page.locator("#foldSummary a").count(),0);
      assert.equal(await page.locator("#foldGo").isDisabled(),true);await page.check("#bodyFitConsent");
      assert.match(await settle("#foldGo",120000),/accepted Anny shape/);
      assert.equal(await page.locator("#foldSummary a").count(),0);
      assert.ok(fs.readFileSync(acceptedFit).equals(raw));assert.ok(fs.readFileSync(acceptedMeasurements).equals(input));
      check(true,"rejected replacement refuses without stale downloads or tape fallback; source inputs unchanged");
    }
    check(errors.length === 0, `no console errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
    check(requests.every(r => r.method === "GET" && r.url.startsWith(`http://localhost:${port}/`)), "actual browser rounds make only local GET requests, with no upload or external request");
  } finally {
    await browser.close();
    srv.kill();
    console.log(`work files in ${work}`);
  }
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error("FAILED", e); process.exit(1); });

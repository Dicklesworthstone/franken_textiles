// Actual local photo controls through compiled WASM. Native captures are made by ftx measure.
// FTX_PLAYWRIGHT_MODULE=playwright-core NODE_PATH=... node tools/check_photo_web.cjs NATIVE_DIR
const {chromium}=require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const assert=require("node:assert/strict"), fs=require("node:fs"), path=require("node:path"), http=require("node:http"), os=require("node:os");
const sha256=bytes=>require("node:crypto").createHash("sha256").update(bytes).digest("hex");
const root=path.resolve(__dirname,".."), web=path.join(root,"web"), native=process.argv[2];
const pkg=process.env.FTX_WASM_PKG?path.resolve(process.env.FTX_WASM_PKG):path.join(web,"pkg");
if(!native) throw Error("Supply the directory containing native photo scale and measurement captures");
const work=fs.mkdtempSync(path.join(os.tmpdir(),"ftx-photo-browser-"));
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
  const browser=await chromium.launch(), page=await browser.newPage({userAgent:"OpenAI File Downloader, XaiImageApiFetch/1.0"});
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
    await page.goto(origin+"/photo.html");
    for(const [file,tag,width,height,end,knownLength] of [["photo.png","png",720,960,630,400],["photo.jpg","jpeg",600,800,530,250]]) {
      await upload(file); await absent(); assert.equal(await page.isDisabled("#measure"),true);
      await page.selectOption("#paper","letter"); await action("analyze");
      assert.equal(await page.textContent("#error"),"");
      assert.deepEqual(await page.locator("#image").evaluate(c=>[c.width,c.height]),[width,height]);
      const scale=JSON.parse(await download("photo-scale.json"));
      assert.deepEqual(scale,expected(`native-scale-${tag}.json`,false));
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
    for(const change of ["cancel","options","points","source","paper","removePair","importOptions","useFormOptions"]) {
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
  } finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);server.close();process.exitCode=1;});

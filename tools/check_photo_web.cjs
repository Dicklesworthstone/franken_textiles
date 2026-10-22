// Actual local photo controls through compiled WASM. Native captures are made by ftx measure.
// FTX_PLAYWRIGHT_MODULE=playwright-core NODE_PATH=... node tools/check_photo_web.cjs NATIVE_DIR
const {chromium}=require(process.env.FTX_PLAYWRIGHT_MODULE || "playwright");
const assert=require("node:assert/strict"), fs=require("node:fs"), path=require("node:path"), http=require("node:http"), os=require("node:os");
const root=path.resolve(__dirname,".."), web=path.join(root,"web"), native=process.argv[2];
if(!native) throw Error("Supply the directory containing native photo scale and measurement captures");
const work=fs.mkdtempSync(path.join(os.tmpdir(),"ftx-photo-browser-"));
const server=http.createServer((req,res)=>{
  let file;
  try {file=path.resolve(web,"."+decodeURIComponent(new URL(req.url,"http://localhost").pathname));} catch {res.writeHead(400).end();return;}
  if(req.method!=="GET" || !file.startsWith(web+path.sep)) {res.writeHead(403).end();return;}
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
      assert.equal(scale.ok,true); if(tag==="jpeg")assert.equal(scale.image_input.exif_orientation,6);
      const box=await page.locator("#image").boundingBox();
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
    for(const change of ["cancel","options","points","source","paper"]) {
      if(await page.isDisabled("#measure")) {await upload("photo.jpg");await page.selectOption("#paper","letter");await action("analyze");await page.fill("#points",fs.readFileSync(path.join(native,"points-jpeg.json"),"utf8"));}
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
      await absent(); assert.equal(await page.isDisabled("#cancel"),true);
      await release.continue().catch(()=>{}); await page.context().unroute("**/ftx_wasm_bg.wasm",route);
      await absent();
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

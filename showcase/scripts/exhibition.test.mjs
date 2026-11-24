// Test the served static export, including real media and native specimen reports.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { gunzipSync, gzipSync } from "node:zlib";
import { after, before, test } from "node:test";
import { chromium } from "playwright-core";

const site = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
let browser;
before(async () => {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
});
after(async () => { await browser?.close(); });
const opts = { userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0", viewport: { width: 1120, height: 820 } };

test("every construction study pairs a full-size cut with its three film chapters",async()=>{
  const page=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try{
    await page.goto(site);
    // The opening offers five garments by name; the others are on the wardrobe and drape-room pages.
    const stage=page.locator("#construction"),picker=stage.getByRole("group",{name:"Choose a garment"});
    const names=await picker.getByRole("button").allTextContents();
    assert.deepEqual(names,["Sundress","Hussar jacket","Flared skirt","Bolero & trousers","Zouave bolero"]);
    assert.equal(await picker.getByRole("button",{name:"Sundress"}).getAttribute("aria-pressed"),"true");
    assert.equal(await stage.getByRole("button",{name:"Sewn article",exact:true}).getAttribute("aria-pressed"),"true");
    assert.equal(await stage.getByRole("link",{name:"Open this garment"}).getAttribute("href"),"/wardrobe/sundress/#sewn");
    await picker.getByRole("button",{name:"Zouave bolero"}).click();
    assert.equal(await stage.getByRole("link",{name:"Turn it in 3D"}).getAttribute("href"),"/fittings/bolero-full-hand-replay");
    assert.equal(await stage.getByRole("link",{name:"Edit the draft"}).getAttribute("href"),"/workbench/?sample=full-hand-bolero");
    assert.equal(await stage.getByRole("link",{name:"Pattern (PDF)"}).getAttribute("href"),"/fittings/bolero-full-hand-A4.pdf");
    const choices=[["Sundress","sundress"],["Hussar jacket","blue"],["Flared skirt","flare"],["Bolero & trousers","ensemble"],["Zouave bolero","bolero-full-hand"]];
    for(const [name,garment] of choices){
      await picker.getByRole("button",{name,exact:true}).click();
      const motion=stage.locator(`[data-garment="${garment}"]`);
      for(const chapter of ["Flat pattern","Match the edges","Sewn article"]){
        await motion.getByRole("button",{name:chapter,exact:true}).click();
        await motion.locator(".construction-still").evaluate(img=>img.decode());
        assert.equal(await motion.locator("video").evaluate(v=>v.paused),true);
      }
      await motion.getByRole("button",{name:"Play construction film"}).click();
      await page.waitForFunction(()=>document.querySelector("#construction video")?.currentTime>0.05);
      assert.equal(await motion.locator("video").evaluate(v=>Math.round(v.duration*16)),96);
      for(const url of await stage.locator("a[download]").evaluateAll(links=>links.map(l=>l.href))){
        const pdf=await page.request.get(url);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
      }
    }
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  }finally{await page.close();}
});

test("the skirt atelier displays the selected native trajectory and its matching cut", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.goto(new URL("/studies/skirt/?cut=column&waist=800",site).href,{waitUntil:"domcontentloaded"});
    await page.locator('canvas[data-run="column-800"]').waitFor();
    assert.ok(await page.getByRole("button",{name:"Flared",exact:true}).isEnabled());
    const data=await (await page.request.get(new URL("/atelier/study.json",site).href)).json();
    for(const cut of ["column","flare"]){
      await page.getByRole("button",{name:"68 centimetre form waist"}).click();
      await page.getByRole("button",{name:cut==="column"?"Column":"Flared",exact:true}).click();
      for(const [key,run] of Object.entries(data.runs).filter(([k])=>k.startsWith(cut+"-"))){
        await page.getByRole("button",{name:run.waist/10+" centimetre form waist"}).click();
        await page.locator('canvas[data-run="'+key+'"]').waitFor();
        await page.locator(".sa-intro").getByRole("button",{name:"Flat pattern"}).click();
        await page.locator('canvas[data-run="'+key+'"][data-native-frame="0"]').waitFor();
        await page.getByRole("button",{name:"Join gores"}).click();
        await page.locator('canvas[data-run="'+key+'"][data-native-frame="87"]').waitFor();
        await page.getByRole("button",{name:"Sewn skirt"}).click();
        const mm=Object.values(run.hemHeights);
        assert.equal(await page.locator(".sa-reading strong").nth(1).innerText(),(Math.max(...mm)-Math.min(...mm)).toFixed(1)+" mm");
        const pattern=await page.locator(".sa-pdf").getAttribute("href");
        assert.equal(pattern,"/atelier/"+run.patternStem+".pdf");
        const pdf=await page.request.get(new URL(pattern,site).href);
        assert.equal(pdf.status(),200);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
        assert.match(await page.locator(".sa-section").innerText(),/Centre-front gap/);
      }
    }
    await page.getByRole("button",{name:"Compare hems"}).click();
    await page.locator('canvas[data-surface="Hems"]').waitFor();
    await page.getByRole("button",{name:"Strain",exact:true}).click();
    await page.locator('canvas[data-surface="Strain"][data-step="100"]').waitFor();
    await page.getByRole("button",{name:"Replay drape"}).click();
    await page.waitForFunction(()=>Number(document.querySelector(".sa-canvas canvas").dataset.step)<95);
    await page.setViewportSize({width:375,height:812});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  } finally {await page.close();}
});

test("the drape room pairs each body and pose with its native cloth and endpoint loads", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.goto(new URL("/sections/studies/",site).href);
    const reports=await (await page.request.get(new URL("/exhibits/poses/runs.json",site).href)).json();
    for(const chest of [48,52,54])for(const angle of [35,75,90]){
      const id=chest*10+"-"+angle,run=reports.cases[id];
      await page.getByRole("button",{name:chest+" centimetre chest"}).click();
      await page.getByRole("button",{name:angle+" degree arm pose"}).click();
      await page.locator('canvas[data-run="'+id+'"]').waitFor({timeout:30000});
      assert.equal(await page.locator(".dr-readings strong").nth(1).textContent(),run.chestClearance.toFixed(1)+" mm");
      assert.equal(await page.locator(".dr-method summary span").textContent(),
        !run.settled?"Cloth still moving":run.flags.length?"Review solver flags":"Settled");
      const method=page.locator(".dr-method");
      const tapes=await method.locator('a[download][href*="/chest-"]').getAttribute("href");
      assert.equal(tapes,`/exhibits/poses/chest-${chest*10}.json`);
      if(angle===35){
        const input=await (await page.request.get(new URL(tapes,site).href)).json();
        assert.equal(input.records.find(r=>r.name==="chest").value_mm,chest*10);
        assert.equal(input.records.find(r=>r.name==="hand_pass").value_mm,145);
      }
      const command=await method.locator("pre").textContent();
      assert.ok(command.includes(`--measurements chest-${chest*10}.json --fit fit-spec.json`));
      assert.ok(command.includes(`--arm-deg ${angle} `) && command.includes(`ftx viewer bolero-${id} --check-contact`));
      assert.ok((await method.textContent()).includes(run.sourcePc2Sha256));
      await page.getByRole("button",{name:"Contact",exact:true}).click();
      await page.locator(`canvas[data-mode="contact"][data-frame="${run.sourceFrames-1}"]`).waitFor();
      assert.equal(await page.locator(".dr-readings strong").last().textContent(),run.peakBodyForceN.toFixed(2)+" N");
    }
    await page.getByRole("button",{name:"Lay the five cut pieces flat"}).click();
    await page.locator('canvas[data-mode="construction"][data-frame="0"]').waitFor();
    const timeline=page.getByRole("slider",{name:"Sewing replay frame"});
    await timeline.fill("86");
    await page.locator('canvas[data-frame="86"][data-native-frame="69"]').waitFor();
    await timeline.fill("87");
    await page.locator('canvas[data-frame="87"][data-native-frame="87"]').waitFor();
    await timeline.fill("88");
    await page.locator('canvas[data-frame="88"][data-native-frame="88"]').waitFor();
    await timeline.press("Home");
    await page.getByRole("button",{name:"Play sewing replay"}).click();
    await page.waitForFunction(()=>Number(document.querySelector(".dr-stage canvas")?.dataset.frame)>2);
    await page.getByRole("button",{name:"Pause sewing replay"}).click();
    await page.setViewportSize({width:390,height:844});
    await page.getByRole("button",{name:"52 centimetre chest"}).click();
    await page.getByRole("button",{name:"35 degree arm pose"}).click();
    await page.getByRole("button",{name:"Coupled outfit"}).click();
    await page.locator('canvas[data-run="520-35"][data-coupled="true"][data-native-frame="135"]').waitFor();
    assert.equal(await page.locator(".dr-readings strong").nth(1).textContent(),"-2.2 mm");
    await page.getByRole("slider",{name:"Sewing replay frame"}).press("Home");
    await page.locator('canvas[data-coupled="true"][data-native-frame="0"]').waitFor();
    assert.ok(await page.locator(".dr-settings a[download]").count()===2);
    assert.equal(await page.locator('.dr-method a[href*="/chest-"]').count(),0);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  } finally {await page.close();}
});

test("the photo workbench detects the demo sheet, measures the marks and clears the demo on upload", async () => {
  const page=await browser.newPage({...opts,viewport:{width:390,height:844}});
  try {
    await page.goto(new URL("/workbench/photo.html",site).href);
    await page.locator("#values tr").first().waitFor({timeout:60000});
    const cells=await page.locator("#values tr").first().locator("td").allTextContents();
    assert.equal(cells[0],"chalk_span");
    assert.ok(Math.abs(Number(cells[1])-200)<.5);
    assert.ok(Number(cells[2])>0);
    assert.equal(await page.locator("#image").evaluate(canvas=>canvas.width),1200);
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    const source=await page.request.get(new URL("/workbench/atelier-assets/photo-example-sleeve.jpg",site).href);
    await page.locator("#photo").setInputFiles({name:"own-sleeve.jpg",mimeType:"image/jpeg",buffer:await source.body()});
    assert.ok(await page.locator("#result").isHidden());
    assert.equal(await page.locator("#points").inputValue(),"{}");
    await page.locator("#analyze").click();
    await page.locator("#preview").waitFor();
    assert.equal(await page.locator("#points").inputValue(),"{}");
  } finally { await page.close(); }
});

test("each gallery garment has a downloadable full-size sewing PDF", async () => {
  const page=await browser.newPage(opts);
  try {
    for (const [route,expected] of [
      ["/sections/wardrobe/",["/sundress/pattern-A4.pdf","/patterns/hussar-blue.pdf","/patterns/hussar-yellow.pdf"]],
      ["/sections/studies/",["/patterns/bolero.pdf","/patterns/trousers.pdf"]],
      ["/sections/tracing/",["/patterns/bolero.pdf"]],
    ]) {
      await page.goto(new URL(route,site).href);
      for(const path of expected) {
        assert.ok(await page.locator('a[download][href="'+path+'"]').first().isVisible());
        const pdf=await page.request.get(new URL(path,site).href);
        assert.equal(pdf.status(),200);
        assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
      }
    }
    await page.goto(new URL("/sections/films/",site).href);
    assert.ok(await page.locator('.screening a[href="/fittings/bolero-full-hand-A4.pdf"]').isVisible());
    await page.getByRole("button",{name:/Lightly eased sundress/}).click();
    assert.ok(await page.locator('.screening a[href="/exhibits/sundress/light-A4.pdf"]').isVisible());
    await page.getByRole("button",{name:/The hussar’s cabinet/}).click();
    assert.ok(await page.locator('.screening a[href="/patterns/hussar-yellow.pdf"]').isVisible());
    await page.getByRole("button",{name:/Anatomy of a seam/}).click();
    assert.ok(await page.locator('.screening a[href="/patterns/bolero.pdf"]').isVisible());
  } finally {await page.close();}
});

test("the full-hand bolero links to the matching editable draft and keeps strain at the endpoint", async () => {
  const page = await browser.newPage({ ...opts, reducedMotion: "reduce" });
  const walkthrough = new URL("/how-it-works/#full-hand-bolero", site).href;
  try {
    await page.goto(walkthrough);
    await page.getByRole("link", { name: "Replay the cut in 3D" }).click();
    await page.locator("#stage canvas").waitFor();
    assert.match(await page.locator("#phase").innerText(), /Flat pattern/);
    const strain = page.getByRole("radio", { name: "strain at the end" });
    const seams = page.getByRole("radio", { name: "seam colours" });
    const frame = page.getByRole("slider", { name: "frame" });
    await strain.check();
    assert.match(await page.locator("#phase").innerText(), /Draped/);
    assert.equal(await frame.inputValue(), await frame.getAttribute("max"));
    assert.equal(await page.locator("#play").innerText(), "Play");
    await frame.press("Home");
    assert.ok(await seams.isChecked());
    await strain.check();
    await page.locator("#play").click();
    assert.ok(await seams.isChecked());

    await page.goto(walkthrough);
    await page.evaluate(() => {
      localStorage.setItem("fab-wb-program", JSON.stringify("fez"));
      localStorage.setItem("fab-wb-stature", JSON.stringify("116"));
      localStorage.setItem("fab-wb-bodyMode", JSON.stringify("tape"));
    });
    await page.getByRole("link", { name: "Open this draft in Pattern Workbench" }).click();
    await page.locator("#draftOverview").waitFor({ timeout: 60000 });
    assert.equal(await page.locator("#resStatus").innerText(), "CONVERGED");
    assert.equal(await page.locator("#stature").inputValue(), "92");
    assert.equal(await page.locator("#drapeHands").inputValue(), "full");
    assert.ok(await page.locator("#splitTimeSteps").isChecked());
    assert.deepEqual(JSON.parse(await page.locator("#drapeThumbPose").inputValue()), {
      left_rotation_vector_rad: [0.11641167, -0.010614, -0.12960467],
      right_rotation_vector_rad: [0.11641167, 0.010614, 0.12960467],
    });
    const sleeve = page.getByRole("row", { name: /Sleeve length over arm length/ });
    const cuff = page.getByRole("row", { name: /Sleeve hem passes the hand/ });
    assert.equal(await sleeve.locator("td").nth(2).innerText(), "355");
    assert.equal(await cuff.locator("td").nth(2).innerText(), "201.7");
    assert.ok(await page.locator("#draftPdf").isVisible());
    assert.deepEqual(await page.evaluate(() => ["program", "stature", "bodyMode"]
      .map(key => JSON.parse(localStorage.getItem("fab-wb-" + key)))), ["fez", "116", "tape"]);
  } finally { await page.close(); }
});

const annyModel = process.env.FTX_TEST_ANNY_MODEL;
test("the full-hand preset sews and drapes on the pinned Anny model", {
  skip: annyModel ? false : "requires FTX_TEST_ANNY_MODEL",
  timeout: 480000,
}, async () => {
  const page = await browser.newPage({ ...opts, reducedMotion: "reduce" });
  try {
    await page.goto(new URL("/workbench/?sample=full-hand-bolero", site).href);
    await page.locator("#draftOverview").waitFor({ timeout: 60000 });
    await page.getByText("Use a fitted body (optional)", { exact: true }).click();
    await page.locator("#bodyFile").setInputFiles(annyModel);
    await page.locator("#foldGo:enabled").waitFor({ timeout: 120000 });
    await page.locator("#foldGo").click();
    await page.locator('a[download="sim_report.json"], #errorBox:not(.hidden)').first().waitFor({ timeout: 360000 });
    assert.ok(await page.locator("#errorBox").isHidden(), await page.locator("#errorText").innerText());
    const report = await page.locator('a[download="sim_report.json"]').evaluate(async link => (await fetch(link.href)).json());
    assert.equal(report.hand_collider, "full");
    assert.deepEqual(report.body.thumb_pose, JSON.parse(await page.locator("#drapeThumbPose").inputValue()));
    assert.ok(report.body.authored_hand_surface_pairs_tested > 0);
    assert.equal(report.self_contact.audit, "passed");
    assert.equal(report.settled, true);
    assert.deepEqual(report.gating, []);
    assert.deepEqual(report.frames, { sew: 12, drape: 36 });
    assert.equal(report.animation.frames - report.animation.first_physical_frame, 49);
    const fold = page.frameLocator("#foldView");
    await fold.getByRole("radio", { name: "strain at the end" }).check();
    assert.match(await fold.locator("#phase").innerText(), /Draped/);
  } finally { await page.close(); }
});

test("visible preview plays, preserves a deliberate pause, and leaves the screen quiet", async () => {
  const p = await browser.newPage({ ...opts, reducedMotion:"no-preference" });
  try {
    // The materials page shows two preview films side by side, above a long run of cards.
    await p.goto(new URL("/sections/materials/",site).href);
    await p.locator(".material-cover").first().scrollIntoViewIfNeeded();
    await p.waitForFunction(()=>document.querySelector(".material-cover video")?.currentTime>.1);
    await p.getByRole("button",{name:"Pause The torsion rig preview"}).click();
    await p.locator(".ft-foot").scrollIntoViewIfNeeded();
    await p.locator(".material-cover").first().scrollIntoViewIfNeeded();
    assert.equal(await p.evaluate(()=>document.querySelector(".material-cover video").paused),true);
    await p.getByRole("button",{name:"Play The torsion rig preview"}).click();
    await p.waitForFunction(()=>document.querySelector(".material-cover video")?.paused===false);
    // the film beside it plays on its own while it is on screen
    await p.waitForFunction(()=>Array.from(document.querySelectorAll(".material-cover video")).slice(1).some(v=>v.currentTime>.1));
    await p.locator(".ft-foot").scrollIntoViewIfNeeded();
    await p.waitForFunction(()=>Array.from(document.querySelectorAll(".material-cover video")).every(v=>v.paused));
  } finally { await p.close(); }
});

test("reduced motion leaves the film paused until requested", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(new URL("/sections/materials/",site).href);await p.getByRole("button",{name:"Play The torsion rig preview"}).waitFor();
    const film=p.locator(".material-cover video").first();
    assert.equal(await film.evaluate(v=>v.paused && v.getAttribute("src")===null),true);
    await p.getByRole("button",{name:"Play The torsion rig preview"}).click();
    await p.waitForFunction(()=>document.querySelector(".material-cover video")?.currentTime>.1);
    assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth),390);
  } finally {await p.close();}
});

test("native Play on the skirt film survives scrolling away under reduced motion", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.goto(new URL("/studies/skirt/#skirt-film",site).href,{waitUntil:"domcontentloaded"});
    const film=page.locator("#skirt-film video");
    await film.scrollIntoViewIfNeeded();
    await page.locator("#skirt-film video[src]").waitFor();
    assert.ok(await film.evaluate(video=>video.controls && video.paused));
    // The play event comes from the video element, as it does with the browser's controls.
    await film.evaluate(video=>video.play());
    await page.evaluate(()=>window.scrollTo({top:0,behavior:"instant"}));
    await page.waitForFunction(()=>document.querySelector("#skirt-film video")?.paused);
    const at=await film.evaluate(video=>video.currentTime);
    await film.scrollIntoViewIfNeeded();
    await page.waitForFunction(time=>{
      const video=document.querySelector("#skirt-film video");
      return video && !video.paused && video.currentTime>time+.1;
    },at,{timeout:5000});
    await film.evaluate(video=>new Promise(resolve=>{
      video.addEventListener("pause",resolve,{once:true});
      video.pause();
    }));
    await page.evaluate(()=>window.scrollTo({top:0,behavior:"instant"}));
    await film.scrollIntoViewIfNeeded();
    await page.waitForTimeout(250);
    assert.ok(await film.evaluate(video=>video.paused));
  } finally {await page.close();}
});

test("a failed 4K film request resumes in 1080p", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.addInitScript(() => {
      navigator.mediaCapabilities.decodingInfo=async()=>({supported:true,smooth:true,powerEfficient:true});
    });
    let attempted=false;
    await page.route(url=>url.pathname==="/media/bolero.mp4", route=>{
      attempted=true;
      return route.abort();
    });
    await page.goto(new URL("/?film=bolero",site).href,{waitUntil:"domcontentloaded"});
    await page.waitForFunction(()=>{
      const video=document.querySelector("dialog[open] video");
      return video?.videoWidth===1920 && video.currentTime>.1 && !video.paused;
    },null,{timeout:30000});
    assert.ok(attempted);
    const source=await page.locator("dialog[open] video").evaluate(video=>video.currentSrc);
    assert.equal(new URL(source).pathname,"/media/bolero-1080.mp4");
  } finally {await page.close();}
});

test("a stuttering 4K film keeps its playhead in 1080p", async () => {
  const page=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await page.addInitScript(() => {
      navigator.mediaCapabilities.decodingInfo=async()=>({supported:true,smooth:true,powerEfficient:true});
      HTMLVideoElement.prototype.getVideoPlaybackQuality=()=>({totalVideoFrames:0,droppedVideoFrames:0});
    });
    await page.goto(new URL("/?film=bolero",site).href,{waitUntil:"domcontentloaded"});
    await page.waitForFunction(()=>{
      const video=document.querySelector("dialog[open] video");
      return video?.videoWidth===3840 && video.currentTime>1;
    });
    const film=page.locator("dialog[open] video");
    // One window with 70 of 180 frames dropped: a busy page can do that to a device that plays 4K60 well.
    await film.evaluate(video=>{
      video.addEventListener("playing",()=>{
        if (video.videoWidth===1920 && video.dataset.resumedAt===undefined) video.dataset.resumedAt=String(video.currentTime);
      });
      window.__filmQuality={totalVideoFrames:180,droppedVideoFrames:70};
      video.getVideoPlaybackQuality=()=>window.__filmQuality;
      video.dispatchEvent(new Event("timeupdate"));
    });
    await page.waitForTimeout(600);
    assert.deepEqual(await film.evaluate(video=>[video.videoWidth,new URL(video.currentSrc).pathname]),[3840,"/media/bolero.mp4"]);
    // A second such window in a row is a device that cannot keep up.
    const at=await film.evaluate(video=>{
      window.__filmQuality={totalVideoFrames:360,droppedVideoFrames:140};
      video.dispatchEvent(new Event("timeupdate"));
      return video.currentTime;
    });
    await page.waitForFunction(time=>{
      const video=document.querySelector("dialog[open] video");
      return video?.videoWidth===1920 && !video.paused && video.dataset.resumedAt && video.currentTime>time+.1;
    },at,{timeout:10000});
    const resumedAt=Number(await film.getAttribute("data-resumed-at"));
    assert.ok(Math.abs(resumedAt-at)<.3, `Expected to resume near ${at}s, began at ${resumedAt}s`);
  } finally {await page.close();}
});

test("strip controls and readouts agree with the exported Rust states", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await p.goto(new URL("/experiments/strip/",site).href);
    await p.getByRole("button",{name:"4% pull"}).click();
    const rows=await p.locator("tbody tr td:first-of-type").allTextContents();
    const expected=await p.evaluate(async()=>{
      const samples=await (await fetch("/materials/strip.json")).json();
      return samples.map(({report})=>report.states.find(s=>s.extension_fraction===.04).force_n.toFixed(3)+" N");
    });
    assert.deepEqual(rows,expected);
    await p.locator(".rig-scene canvas[data-step='4']").waitFor();
    assert.deepEqual(await p.locator(".rig-measured strong").allTextContents(),["0.12 N","-0.01 N","0.04 N"]);
    await p.getByRole("button",{name:"Weft",exact:true}).click();
    await p.locator('canvas[data-specimen="Weft"]').waitFor();
    await p.goto(new URL("/experiments/cylinder/",site).href);
    await p.getByRole("button",{name:"Compressed",exact:true}).click();
    assert.deepEqual(await p.locator(".rig-refinement b").allTextContents(),["-0.63 N","-0.97 N"]);
    await p.getByText("Solver notes and residuals",{exact:true}).click();
    assert.ok(await p.getByText("48 × 12 mesh: the solve did not converge.",{exact:true}).isVisible());
    await p.getByRole("button",{name:"Mid-section",exact:true}).click();
    assert.equal(await p.getByRole("button",{name:"Mid-section",exact:true}).getAttribute("aria-pressed"),"true");
  } finally {await p.close();}
});

test("sundress ease uses the fitted Anny tapes and preserves the fixed cut", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    await p.goto(new URL("/wardrobe/sundress/",site).href);
    await p.locator(".sd-scene canvas[data-ready]").waitFor();
    const [cases,body]=await p.evaluate(()=>Promise.all(["/sundress/fit-comparison.json","/fitted-bodies/sundress.json"].map(async url=>(await fetch(url)).json())));
    const reference=body.samples.find(sample=>sample.name==="reference");
    for(const item of cases.runs) {
      await p.getByRole("button",{name:item.form,exact:true}).click();
      for(const point of item.points){
        let tape=reference.outcomes[point.id];
        for(const target of item.points){
          const sample=body.samples.find(s=>s.name===target.id+"High");
          const t=(target.body_mm/10-reference.targets[target.id])/(sample.targets[target.id]-reference.targets[target.id]);
          tape+=t*(sample.outcomes[point.id]-reference.outcomes[point.id]);
        }
        const row=p.locator(".sd-table>div").filter({hasText:point.id});
        await p.waitForFunction(({id,v})=>document.querySelector('[data-fitted="'+id+'"]').textContent===v,{id:point.id,v:tape.toFixed(1)});
        assert.equal(Number(await row.locator("span").first().textContent()),point.finished_mm/10);
        assert.equal(Number(await row.locator("b").textContent()),Number((point.finished_mm/10-tape).toFixed(1)));
      }
    }
    await p.getByRole("button",{name:"Reset",exact:true}).click();
    await p.getByRole("slider",{name:"Body mass target"}).press("End");
    const fittedWeight=body.samples.find(s=>s.name==="weightHigh").outcomes.weight.toFixed(1);
    await p.locator('[data-model-mass="'+fittedWeight+'"]').waitFor();
    assert.notEqual(Number(fittedWeight),body.domains.weight[1]);
    const stage=p.locator(".sd-scene canvas"),before=await stage.screenshot();
    await p.getByRole("button",{name:"Raised",exact:true}).click();
    assert.equal(await p.getByRole("button",{name:"Raised",exact:true}).getAttribute("aria-pressed"),"true");
    assert.notDeepEqual(await stage.screenshot(),before);
    assert.equal(await p.getByRole("link",{name:"Three native flat checks"}).getAttribute("href"),"/sundress/fit-comparison.json");
    // The solver's drape of the same pattern replays the packaged frames: the settled endpoint first, the flat cut at frame zero.
    const sewn=p.locator(".sd-making canvas[data-ready]"),summary=await (await p.request.get(new URL("/exhibits/sundress/dress-summary.json",site).href)).json();
    await sewn.waitFor();
    // The band can shorten while each skirt stays held at the waist.
    assert.equal(summary.settled,true);
    assert.deepEqual(summary.gating,[]);
    for(const side of ["skirt_front","skirt_back"])assert.ok(summary.flags.some(flag=>flag.startsWith(side)&&flag.includes("did not slide")));
    const endpoint=await sewn.getAttribute("data-frame");
    assert.equal(Number(endpoint),Object.values(summary.phases).reduce((a,b)=>a+b)-1);
    await p.getByRole("slider",{name:"Sewing replay frame"}).fill("0");
    await p.waitForFunction(()=>document.querySelector(".sd-making canvas").dataset.frame==="0");
    // The flat pattern is shown under the stage, one outline per cut piece, and the four stages are named.
    assert.equal(await p.locator(".sd-making .mk-pattern path").count(),summary.instances.length);
    assert.deepEqual(await p.locator(".sd-making .mk-steps button").allTextContents(),["1Cut flat","2Shape the panels","3Seams sewn","4Hung under gravity"]);
    assert.equal(await p.locator(".sd-making .mk-steps .is-now").textContent(),"1Cut flat");
    const cutSelector=p.getByRole("group",{name:"Skirt waist cut"});
    const sections=[];
    for(const [id,title] of [["light","Lightly eased waist"],["dress","Gathered waist"]]){
      await cutSelector.getByRole("button",{name:new RegExp(title)}).click();
      const base=new URL("/exhibits/sundress/"+id,site).href;
      const native=await (await p.request.get(base+"-summary.json")).json();
      await sewn.waitFor(); await p.locator(".sd-making .mk-steps button").nth(3).click();
      await p.waitForFunction(frame=>document.querySelector(".sd-making canvas")?.dataset.frame===String(frame),
        Object.values(native.phases).reduce((a,b)=>a+b)-1);
      assert.equal(await p.locator("#sewn h3").textContent(),title);
      const section=await (await p.request.get(base+"-section.json")).json();
      assert.equal(native.settled,true);
      assert.deepEqual(native.gating,[]);
      for(const flag of native.flags){
        const reading=flag.startsWith("the body rides up ")
          ? `The front hem settled ${section.hemRiseMm.front} mm above its starting height, and the back hem settled ${section.hemRiseMm.back} mm above its starting height.`
          : flag;
        await p.locator(".sd-sewn .sd-flag",{hasText:reading}).waitFor();
      }
      sections.push(section); assert.ok((await p.locator(".sd-slice").textContent()).includes(section.frontGapMm+" mm"));
      const href=await p.getByRole("link",{name:"Full-size A4 pattern for this cut"}).getAttribute("href");
      const pdf=await p.request.get(new URL(href,site).href);
      assert.equal((await pdf.body()).subarray(0,5).toString(),"%PDF-");
    }
    assert.equal(sections[0].heightMm,sections[1].heightMm);
    assert.deepEqual(sections[0].body,sections[1].body);
    assert.ok(sections[0].frontGapMm<sections[1].frontGapMm);
    assert.notDeepEqual(sections[0].cloth,sections[1].cloth);
    assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth),390);
  } finally {await p.close();}
});

test("a sewist can compare two muslins privately and retrieve the instructions for the selected cut", async () => {
  const page = await browser.newPage({...opts, reducedMotion:"reduce", viewport:{width:390,height:844}});
  try {
    await page.goto(new URL("/wardrobe/sundress/?garment=eased#muslin", site).href);
    const trial = page.locator("#muslin"), notes = page.getByLabel("Form, cloth, measuring uncertainty and discrepancies");
    const slice = trial.getByRole("img", {name:/Both waist cuts/});
    assert.equal(await slice.locator("path").count(), 3);
    const position=trial.getByRole("combobox",{name:"Measure on the section"});
    for (const cut of ["dress","light"]) {
      const section=await (await page.request.get(new URL(`/exhibits/sundress/${cut}-section.json`,site).href)).json();
      for(const side of ["front","back"]){
        await position.selectOption(cut+"-"+side);
        const span=await slice.locator("[data-ruler]").evaluate(line=>[line.x1.baseVal.value,line.y1.baseVal.value,line.y2.baseVal.value]);
        assert.ok(Math.abs(span[0]-section.rulerMm.x)<0.001);
        assert.ok(Math.abs(span[1]-section.rulerMm[side].bodyY)<0.001);
        assert.ok(Math.abs(span[2]-section.rulerMm[side].clothY)<0.001);
        const gap=(span[2]-span[1])*(side==="front"?-1:1);
        assert.ok(Math.abs(gap-section[side+"GapMm"])<0.051);
      }
    }
    const save = trial.getByRole("button", {name:"Save worksheet"});
    const download = async () => {
      const pending = page.waitForEvent("download");
      await save.click();
      return readFile(await (await pending).path(), "utf8");
    };
    assert.match(await download(), /observed ____ mm/);
    const front = trial.getByRole("group", {name:"Centre front"});
    const [gathered, eased] = await front.getByRole("textbox").all();
    await gathered.fill("0"); await eased.fill("7,5");
    assert.equal(await position.inputValue(),"light-front");
    assert.match(await front.locator("output").innerText(), /predicted -31.9 mm · observed \+7.5 mm · residual \+39.4 mm/);
    await notes.fill("Synthetic browser check only.");
    const sheet = await download();
    assert.match(sheet, /predicted 83.9 mm, observed 0 mm, residual -83.9 mm/);
    assert.match(sheet, /predicted -31.9 mm, observed \+7.5 mm, residual \+39.4 mm/);
    assert.match(sheet, /Synthetic browser check only/);
    assert.match(sheet, /Native trajectory SHA-256 [a-f0-9]{64}/);
    await gathered.fill("0\n12\n6");
    await eased.fill("7,5\n8,5");
    assert.match(await front.innerText(), /3 readings · 0–12 mm · mean 6.0 mm/);
    assert.match(await front.locator("output").innerText(), /observed \+2.0 mm · residual \+33.9 mm/);
    const repeats = await download();
    assert.match(repeats, /Readings \(mm\): 7,5 \/ 8,5\. Count 2\. Range 7.5–8.5 mm/);
    assert.match(repeats, /predicted 83.9 mm, observed 6 mm, residual -77.9 mm/);
    assert.match(repeats, /Centre back\n[^\n]*observed ____ mm/);
    await eased.fill("7,5\n-1"); assert.equal(await save.isDisabled(), true);
    assert.equal(await eased.getAttribute("aria-invalid"), "true");
    assert.doesNotMatch(await front.locator("output").innerText(), /observed/);
    await eased.fill(""); assert.equal(await save.isEnabled(), true);
    assert.doesNotMatch(await front.locator("output").innerText(), /observed/);
    const instructions = page.locator("details").filter({has:page.getByText("Cut and sew the dress", {exact:true})});
    await instructions.locator("summary").click();
    const replay = page.locator("#sundress-replay");
    await replay.locator("summary").click();
    for (const [id, name, tiles, edge, svg] of [["light","Lightly eased waist",36,"409.5","/exhibits/sundress/light-master.svg"],["dress","Gathered waist",37,"507","/sundress/master.svg"]]) {
      await page.getByRole("group",{name:"Skirt waist cut"}).getByRole("button",{name:new RegExp(name)}).click();
      const body = await instructions.innerText();
      assert.ok(body.includes(`${tiles} tiles`) && body.includes(`${edge} mm skirt waist`));
      assert.equal(await page.getByRole("link", {name:"Download FrankenTextiles pattern"}).getAttribute("href"), `/exhibits/sundress/${id}-pattern.json`);
      assert.equal(await page.getByRole("link", {name:"Download full-scale pattern SVG"}).getAttribute("href"), svg);
      const drawing = await page.request.get(new URL(svg,site).href);
      assert.match(await drawing.text(), /fp:schema="fabric-pattern\/1"/);
      const summary = await (await page.request.get(new URL(`/exhibits/sundress/${id}-summary.json`,site).href)).json();
      const recipe = await replay.innerText();
      const revision = replay.getByRole("link", {name:"native solver at " + summary.solverRevision.slice(0,8)});
      assert.ok((await revision.getAttribute("href")).endsWith("/tree/"+summary.solverRevision));
      assert.ok((await page.locator("details").filter({has:page.getByText("What the solver did, and what is drawn",{exact:true})}).innerText())
        .includes(summary.finalResidualN.toLocaleString("en", {maximumSignificantDigits:3})+" N"));
      assert.ok(recipe.includes(`ftx drape ${id}-pattern.json --measurements ${id}-measures.json --fit ${id}-fit.json ${summary.solverArguments.join(" ")} --out ${id}-drape`));
      assert.ok(recipe.includes(`ftx viewer ${id}-drape --check-contact`) && recipe.includes(summary.sourcePc2Sha256));
      for (const [label, suffix] of [["starting pattern","pattern"],["synthetic body measurements","measures"],["fit targets","fit"]]) {
        const href = await replay.getByRole("link", {name:label, exact:true}).getAttribute("href");
        assert.equal(href, `/exhibits/sundress/${id}-${suffix}.json`);
        const download = await page.request.get(new URL(href,site).href);
        assert.equal(download.status(),200);
        assert.deepEqual(await download.body(), await readFile(new URL("../public"+href,import.meta.url)));
      }
    }
    await page.reload();
    assert.equal(await notes.inputValue(), ""); assert.equal(await gathered.inputValue(), "");
  } finally { await page.close(); }
});

test("a swapped bolero body is refused and a matching pose can still be opened", async () => {
  const page = await browser.newPage({...opts, reducedMotion:"reduce"});
  try {
    const other = await readFile(new URL("../public/exhibits/poses/480-35.bin.gz", import.meta.url));
    await page.route("**/520-35.bin.gz*", route => route.fulfill({body:other, contentType:"application/octet-stream"}));
    await page.goto(new URL("/sections/studies/#drape-room",site).href);
    const room = page.locator("#drape-room");
    await room.getByRole("status").filter({hasText:"3D unavailable"}).waitFor();
    assert.equal(await room.locator("canvas[data-run]").count(),0);
    await room.getByRole("button",{name:"48 centimetre chest"}).click();
    await room.locator('canvas[data-run="480-35"]').waitFor();
  } finally { await page.close(); }
});

test("the skirt and trouser geometry must match the cut and pose selected in the study", async () => {
  const page = await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    const bust = JSON.parse(gunzipSync(await readFile(new URL("../public/atelier/bust-680.json.gz",import.meta.url))));
    // Preserve every index and array length, but move the form 2 cm.
    for (let i=0;i<bust.xyz.length;i+=3) bust.xyz[i]+=200;
    await page.route("**/atelier/bust-680.json.gz*", route=>route.fulfill({body:gzipSync(JSON.stringify(bust)),contentType:"application/gzip"}));
    await page.goto(new URL("/studies/skirt/?cut=column&waist=680",site).href);
    await page.getByRole("link",{name:"Watch the 68 cm study film"}).waitFor();
    assert.equal(await page.locator('canvas[data-run="column-680"]').count(),0);
    await page.getByRole("button",{name:"80 centimetre form waist"}).click();
    await page.locator('canvas[data-run="column-800"]').waitFor();
    const seated = await readFile(new URL("../public/exhibits/trouser-study/sit-12.gz",import.meta.url));
    await page.route("**/trouser-study/standing.gz*",route=>route.fulfill({body:seated,contentType:"application/gzip"}));
    await page.goto(new URL("/studies/trousers/",site).href);
    await page.getByRole("status").filter({hasText:"surface could not be loaded"}).waitFor();
    assert.equal(await page.locator('canvas[data-run="standing"]').count(),0);
    await page.getByRole("button",{name:"Shallow seat",exact:true}).click();
    await page.locator('canvas[data-run="sit-12"][data-cloth="true"]').waitFor();
  } finally { await page.close(); }
});

test("a failed 3D package plays the selected cut's chapters and can retry the live replay", async () => {
  const page = await browser.newPage({...opts,reducedMotion:"reduce",viewport:{width:390,height:844}});
  try {
    // Keep the recording ID and dimensions, but change a triangle in the eased cut.
    const meta = JSON.parse(await readFile(new URL("../public/exhibits/sundress/light.json",import.meta.url),"utf8"));
    [meta.triangles[0], meta.triangles[1]] = [meta.triangles[1], meta.triangles[0]];
    await page.route("**/sundress/light.json*", route => route.fulfill({json:meta}));
    await page.goto(new URL("/wardrobe/sundress/?garment=eased#sewn",site).href);
    const dress = page.locator(".sd-sewn-board"), eased = dress.locator('[data-garment="eased"]');
    await eased.getByRole("button",{name:"Match the edges"}).click();
    assert.match(await eased.locator(".construction-still").getAttribute("src"),/construction-eased-join\.jpg/);
    await eased.getByRole("button",{name:"Play construction film"}).click();
    await page.waitForFunction(()=>document.querySelector(".sd-sewn-board video")?.currentTime>0.1);
    assert.match(await eased.locator("video").getAttribute("src"),/construction-eased\.mp4/);
    assert.equal(await dress.getByRole("button",{name:"Waist profile"}).count(),0);
    await page.unroute("**/sundress/light.json*");
    await dress.getByRole("button",{name:"Retry 3D"}).click();
    await dress.locator("canvas[data-ready]").waitFor();
    await page.route("**/hussar/dolman.gz*", route=>route.abort());
    await page.goto(new URL("/wardrobe/hussar/#native-dolman",site).href);
    const jacket=page.locator("#native-dolman");
    await jacket.locator('[data-garment="blue"]').waitFor();
    await jacket.getByRole("button",{name:"Yellow",exact:true}).click();
    const yellow=jacket.locator('[data-garment="yellow"]');
    await yellow.getByRole("button",{name:"Sewn article"}).click();
    assert.match(await yellow.locator(".construction-still").getAttribute("src"),/construction-yellow\.jpg/);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.unroute("**/hussar/dolman.gz*");
    await jacket.getByRole("button",{name:"Retry 3D"}).click();
    await jacket.locator('canvas[data-ready="Yellow"]').waitFor();
  } finally { await page.close(); }
});

test("the hussar wardrobe connects the dressed figure, pattern front and native sewing replay", async () => {
  const p=await browser.newPage({...opts,reducedMotion:"reduce"});
  try {
    await p.goto(new URL("/wardrobe/hussar/",site).href);
    const recipe = p.locator("details.hu-flags").filter({ has: p.getByText("Replay this jacket with FrankenTextiles", { exact: true }) });
    await recipe.locator("summary").click();
    const summary = await (await p.request.get(new URL("/exhibits/hussar/dolman-summary.json", site).href)).json();
    const measurements = recipe.getByRole("link", { name: "synthetic body measurements" });
    const tapes = await (await p.request.get(new URL(await measurements.getAttribute("href"), site).href)).json();
    assert.equal(tapes.fused.chest.value_mm, 1000);
    const command = await recipe.textContent();
    assert.ok(command.includes(summary.solverArguments.join(" ")) && command.includes(summary.sourcePc2Sha256));
    assert.ok(command.includes("ftx viewer dolman --check-contact"));
    assert.ok((await p.locator(".hu-readings").textContent()).includes(`${summary.bodyOffsetMm.achieved.toFixed(3)} / ${summary.bodyOffsetMm.target} mm`));
    const book=p.locator(".regimental-cuts");
    await book.scrollIntoViewIfNeeded();
    for(const regiment of ["blue","yellow"]){
      await book.getByRole("combobox",{name:"Regiment",exact:true}).selectOption(regiment);
      await book.getByRole("button",{name:"Cap",exact:true}).click();
      await book.locator('option[value="'+(regiment==="blue"?"mirliton_wall":"busby_wall")+'"]').waitFor({state:"attached"});
      const seen=new Set();
      for(const article of await book.locator(".regimental-items button").all()){
        await article.click();
        for(const id of await book.getByLabel("Flat piece").locator("option").evaluateAll(o=>o.map(x=>x.value)))seen.add(id);
        assert.ok(await book.locator(".regimental-cut svg path.dl-cut").count()>0);
      }
      assert.equal(seen.size,49);
    }
    const cabinet=p.locator("#dress-study");
    await cabinet.scrollIntoViewIfNeeded();
    await cabinet.locator("canvas[data-ready='Compare-Dressed']").waitFor({timeout:90000});
    await cabinet.getByRole("radio",{name:"Yellow",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Side",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Pelisse",exact:true}).check();
    await cabinet.getByRole("radio",{name:"Flat",exact:true}).check();
    await cabinet.locator("canvas[data-ready='Yellow-Pelisse'][data-piece='pelisse_front']").waitFor();
    const pattern=await (await p.request.get(new URL("/patterns/hussar-yellow.json",site).href)).json();
    const front=pattern.pieces.find(piece=>piece.id==="pelisse_front");
    assert.equal(await cabinet.locator("dt",{hasText:"Cord paths per cut"}).locator("..").locator("dd").textContent(),
      String(front.decor.filter(row=>row.kind==="soutache").length));
    await cabinet.getByRole("link",{name:"Watch the dolman sew together"}).click();
    const replay=p.locator("#native-dolman");
    await replay.getByRole("button",{name:"Yellow",exact:true}).click();
    await replay.getByRole("slider",{name:"Sewing replay frame"}).press("Home");
    await replay.locator("canvas[data-ready='Yellow'][data-frame='0']").waitFor();
    const loaded = await p.evaluate(() => performance.getEntriesByType("resource").map(entry => entry.name));
    for (const [ext, integrity] of [["json", summary.layoutIntegrity], ["gz", summary.geometryIntegrity]]) {
      assert.ok(loaded.includes(new URL(`/exhibits/hussar/dolman.${ext}?v=${encodeURIComponent(integrity)}`, site).href));
    }
    for (const width of [1120,375]) {
      await p.setViewportSize({width,height:820});
      const stage=replay.locator(".mk-stage");
      const [controls, canvas, caption]=await Promise.all([".mk-steps","canvas",".hu-tag"].map(selector=>stage.locator(selector).boundingBox()));
      assert.ok(controls.y+controls.height<=canvas.y+1 && canvas.y+canvas.height<=caption.y+1,
        "The entire cut has its own viewport between the controls and the caption");
    }
    const file=p.getByRole("link",{name:"Download DST"});
    const dst=await file.evaluate(async a=>new Uint8Array(await (await fetch(a.href)).arrayBuffer()).slice(0,3).join(","));
    assert.equal(dst,"76,65,58"); // Tajima header begins with "LA:".
  } finally {await p.close();}
});

test("the menu lands the keyboard on a section of the home page, and the atelier's form spans a phone", async () => {
  const phone = await browser.newPage({ ...opts, viewport: { width: 390, height: 844 }, reducedMotion: "reduce" });
  try {
    await phone.goto(site);
    const sheet = phone.locator("dialog.ft-sheet");
    const settled = id => phone.waitForFunction(target =>
      !document.querySelector("dialog.ft-sheet[open]") && document.activeElement?.id === target, id);
    await phone.getByRole("button", { name: "Menu" }).click();
    await sheet.getByRole("navigation", { name: "On this page" }).getByRole("link", { name: "Garments made this way" }).press("Enter");
    await settled("garments");
    await phone.keyboard.press("Tab");
    assert.equal(await phone.evaluate(() => !!document.activeElement?.closest("#garments")), true,
      "After an entry that points into this page, the next Tab continues inside that section");
    // The click usually lands on a child of the link, which once left the sheet open over the scrolled page.
    await phone.getByRole("button", { name: "Menu" }).click();
    await sheet.getByRole("navigation", { name: "On this page" }).getByRole("link", { name: "Three steps" }).click();
    await settled("steps");
    // The home page is a short introduction: every part of the catalogue is reached from it by a link.
    const sections = await phone.locator("main > section").count();
    assert.ok(sections <= 7, `${sections} sections on the home page`);
    for (const route of ["/sections/wardrobe/", "/sections/studies/", "/sections/studio/", "/sections/materials/",
      "/sections/tracing/", "/sections/films/", "/how-it-works/", "/get-started/"])
      assert.ok(await phone.locator(`main a[href="${route}"]`).count() >= 1, `the home page links to ${route}`);
    // The form once shared a grid row with three control groups and was squeezed to 135 px.
    await phone.goto(new URL("/studies/skirt/", site).href);
    await phone.locator(".sa-display").waitFor();
    const form = await phone.locator(".sa-display").boundingBox();
    assert.ok(form.width >= 380, `The fitted form spans the phone (${form.width} px)`);
  } finally { await phone.close(); }
});
